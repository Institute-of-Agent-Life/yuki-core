use crate::Result;
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Connection, Row, SqliteConnection, SqlitePool};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};

#[derive(Debug)]
struct EmbeddedMigrations;
type MigrationFuture =
    std::pin::Pin<Box<dyn Future<Output = Result<Vec<sqlx::migrate::Migration>>> + Send>>;
impl sqlx::migrate::MigrationSource<'static> for EmbeddedMigrations {
    fn resolve(self) -> MigrationFuture {
        Box::pin(async { Ok(include!(concat!(env!("OUT_DIR"), "/migrations.rs"))) })
    }
}

fn options(data: &Path) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(data.join("core.sqlite"))
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5))
}

pub async fn bootstrap(data: &Path) -> Result<i64> {
    let mut connection = SqliteConnection::connect_with(
        &options(data)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full),
    )
    .await?;
    sqlx::migrate::Migrator::new(EmbeddedMigrations)
        .await?
        .run(&mut connection)
        .await?;
    let version = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations WHERE success")
        .fetch_one(&mut connection)
        .await?;
    connection.close().await?;
    Ok(version)
}

async fn read_pool(data: &Path) -> Result<SqlitePool> {
    Ok(SqlitePoolOptions::new()
        .max_connections(2)
        .connect_with(options(data).read_only(true))
        .await?)
}

pub enum Mutation {
    Person(crate::identity::person::Change),
    SetLabel(String),
    #[cfg(test)]
    Invalid,
}

trait Driver: Send + 'static {
    fn begin(&mut self) -> impl Future<Output = Result<()>> + Send;
    fn apply(&mut self, action: &Mutation) -> impl Future<Output = Result<Value>> + Send;
    fn commit(&mut self) -> impl Future<Output = Result<()>> + Send;
    fn rollback(&mut self) -> impl Future<Output = Result<()>> + Send;
    fn close(&mut self) -> impl Future<Output = Result<()>> + Send;
    fn reopen(&mut self) -> impl Future<Output = Result<()>> + Send;
}

struct SqliteDriver {
    data: PathBuf,
    connection: Option<SqliteConnection>,
    close_uncertain: bool,
}
impl SqliteDriver {
    async fn open(data: &Path) -> Result<Self> {
        let connection = SqliteConnection::connect_with(
            &options(data)
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Full),
        )
        .await?;
        Ok(Self {
            data: data.into(),
            connection: Some(connection),
            close_uncertain: false,
        })
    }
    fn connection(&mut self) -> &mut SqliteConnection {
        self.connection.as_mut().expect("open writer")
    }
}
impl Driver for SqliteDriver {
    async fn begin(&mut self) -> Result<()> {
        sqlx::query("BEGIN IMMEDIATE")
            .execute(self.connection())
            .await?;
        Ok(())
    }
    async fn apply(&mut self, action: &Mutation) -> Result<Value> {
        let label = match action {
            Mutation::Person(change) => {
                return crate::identity::person::apply(self.connection(), change).await;
            }
            Mutation::SetLabel(value) => value.as_str(),
            #[cfg(test)]
            Mutation::Invalid => "must rollback",
        };
        sqlx::query("SAVEPOINT apply_metadata")
            .execute(self.connection())
            .await?;
        let row = sqlx::query(
            "INSERT INTO core_meta(key,value,revision) VALUES('label',?,1) \
            ON CONFLICT(key) DO UPDATE SET value=excluded.value,revision=core_meta.revision+1 \
            RETURNING value,revision",
        )
        .bind(label)
        .fetch_one(self.connection())
        .await?;
        sqlx::query("RELEASE SAVEPOINT apply_metadata")
            .execute(self.connection())
            .await?;
        #[cfg(test)]
        if matches!(action, Mutation::Invalid) {
            sqlx::query("INSERT INTO deliberately_missing_table VALUES(1)")
                .execute(self.connection())
                .await?;
        }
        Ok(
            json!({"value": row.try_get::<String,_>("value")?, "revision": row.try_get::<i64,_>("revision")?}),
        )
    }
    async fn commit(&mut self) -> Result<()> {
        sqlx::query("COMMIT").execute(self.connection()).await?;
        Ok(())
    }
    async fn rollback(&mut self) -> Result<()> {
        sqlx::query("ROLLBACK").execute(self.connection()).await?;
        Ok(())
    }
    async fn close(&mut self) -> Result<()> {
        if self.close_uncertain {
            return Err("physical close remains unknown".into());
        }
        if let Some(connection) = self.connection.take() {
            self.close_uncertain = true;
            connection.close().await?;
            self.close_uncertain = false;
        }
        Ok(())
    }
    async fn reopen(&mut self) -> Result<()> {
        if self.close_uncertain {
            return Err("cannot reopen after unknown physical close".into());
        }
        *self = Self::open(&self.data).await?;
        Ok(())
    }
}

struct Signals {
    accepting: AtomicBool,
    active: AtomicBool,
    commits: AtomicU64,
    hints: broadcast::Sender<u64>,
}

enum Command {
    Write(Mutation, oneshot::Sender<Result<Value>>),
    Shutdown(oneshot::Sender<Result<()>>),
}

#[derive(Clone)]
pub struct Store {
    commands: mpsc::Sender<Command>,
    readers: SqlitePool,
    signals: Arc<Signals>,
}

#[derive(Serialize)]
pub struct Health {
    pub reader_ok: bool,
    pub writer_accepting: bool,
    pub commits: u64,
}

impl Store {
    pub async fn open(data: &Path) -> Result<Self> {
        if !data.join("core.sqlite").is_file() {
            return Err("database missing; use init/migrate".into());
        }
        bootstrap(data).await?;
        Ok(Self::start(
            SqliteDriver::open(data).await?,
            read_pool(data).await?,
        ))
    }
    fn start(mut driver: impl Driver, readers: SqlitePool) -> Self {
        let (commands, mut queue) = mpsc::channel(64);
        let signals = Arc::new(Signals {
            accepting: AtomicBool::new(true),
            active: AtomicBool::new(false),
            commits: AtomicU64::new(0),
            hints: broadcast::channel(16).0,
        });
        let state = signals.clone();
        tokio::spawn(async move {
            while let Some(command) = queue.recv().await {
                match command {
                    Command::Shutdown(reply) => {
                        state.accepting.store(false, Ordering::SeqCst);
                        let _ = reply.send(driver.close().await);
                        return;
                    }
                    Command::Write(action, reply) => {
                        if reply.is_closed() {
                            continue;
                        }
                        if !state.accepting.load(Ordering::SeqCst) {
                            let _ = reply.send(Err("writer unavailable".into()));
                            continue;
                        }
                        state.active.store(true, Ordering::SeqCst);
                        let result = async {
                            driver.begin().await?;
                            let value = driver.apply(&action).await?;
                            if reply.is_closed() {
                                return Err("caller canceled before commit".into());
                            }
                            driver.commit().await?;
                            Ok(value)
                        }
                        .await;
                        if result.is_err() && driver.rollback().await.is_err() {
                            state.accepting.store(false, Ordering::SeqCst);
                            if driver.close().await.is_ok() && driver.reopen().await.is_ok() {
                                state.accepting.store(true, Ordering::SeqCst);
                            }
                        }
                        state.active.store(false, Ordering::SeqCst);
                        if result.is_ok() {
                            let revision = state.commits.fetch_add(1, Ordering::SeqCst) + 1;
                            let _ = state.hints.send(revision);
                        }
                        let _ = reply.send(result);
                    }
                }
            }
            state.accepting.store(false, Ordering::SeqCst);
            let _ = driver.close().await;
        });
        Self {
            commands,
            readers,
            signals,
        }
    }
    pub async fn write(&self, action: Mutation) -> Result<Value> {
        if !self.signals.accepting.load(Ordering::SeqCst) {
            return Err("writer unavailable".into());
        }
        let (reply, result) = oneshot::channel();
        self.commands
            .send(Command::Write(action, reply))
            .await
            .map_err(|_| "writer stopped")?;
        result.await.map_err(|_| "writer stopped before response")?
    }
    pub(crate) fn readers(&self) -> &SqlitePool {
        &self.readers
    }

    pub async fn label(&self) -> Result<Value> {
        let row = sqlx::query("SELECT value,revision FROM core_meta WHERE key='label'")
            .fetch_optional(&self.readers)
            .await?;
        Ok(match row {
            Some(row) => json!({"value": row.try_get::<String,_>("value")?,
            "revision": row.try_get::<i64,_>("revision")?}),
            None => Value::Null,
        })
    }
    pub async fn health(&self) -> Health {
        Health {
            reader_ok: sqlx::query("SELECT 1")
                .fetch_one(&self.readers)
                .await
                .is_ok(),
            writer_accepting: self.signals.accepting.load(Ordering::SeqCst),
            commits: self.signals.commits.load(Ordering::SeqCst),
        }
    }
    pub async fn database_info(&self) -> Result<Value> {
        let version: String = sqlx::query_scalar("SELECT sqlite_version()")
            .fetch_one(&self.readers)
            .await?;
        let compile_options: Vec<String> = sqlx::query_scalar("PRAGMA compile_options")
            .fetch_all(&self.readers)
            .await?;
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&self.readers)
            .await?;
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&self.readers)
            .await?;
        let schema_version: Option<i64> =
            sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
                .fetch_one(&self.readers)
                .await?;
        Ok(
            json!({"sqlite_version":version,"compile_options":compile_options,"foreign_keys":foreign_keys,
            "journal_mode":journal_mode,"schema_version":schema_version,"minimum_reader":schema_version}),
        )
    }
    pub async fn shutdown(&self) -> Result<()> {
        let (reply, result) = oneshot::channel();
        self.commands
            .send(Command::Shutdown(reply))
            .await
            .map_err(|_| "writer stopped")?;
        let outcome = result.await.map_err(|_| "writer close not acknowledged")?;
        self.readers.close().await;
        outcome
    }
}

#[cfg(test)]
mod tests;
