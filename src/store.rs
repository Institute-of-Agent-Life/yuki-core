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
    crate::memory::index::register()?;
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
    Control(
        Arc<crate::operator::domain::Service>,
        Box<crate::operator::domain::Request>,
    ),
    ProfileConfig(
        Arc<crate::model::profile_store::Service>,
        Box<crate::model::profile_store::Request>,
    ),
    ParticipationQq(Vec<String>, Vec<(String, String, String, bool)>),
    ToolBudget(String, String, String),
    Personal(Box<crate::host::personal::Request>),
    Host(Box<crate::host::Change>),
    Tool(Box<crate::tools::Change>),
    Work(Box<crate::work::Call>),
    WorkSignal(crate::work::signal::Action),
    Vector(Box<crate::memory::embedding::Change>),
    Media(Box<crate::media::Change>),
    Extraction(Box<crate::memory::extract::Change>),
    Memory(Box<crate::memory::Change>),
    History(Box<crate::history::Change>),
    Social(Box<crate::social::Change>),
    Main(
        Arc<crate::runtime::main::Config>,
        Box<crate::runtime::main::Change>,
    ),
    SeedPausedGroup(String, String, String),
    RecoverGroup(Box<crate::ingress::group_recovery::Recovery>),
    Privacy(crate::privacy::Action),
    Search(Box<crate::model::search::Change>),
    Model(Box<crate::model::attempt::Action>),
    Effect(crate::runtime::effects::Action),
    ObserveQqAccount(String, String),
    Ingress(Box<crate::ingress::admit::Admission>),
    Claim(i64),
    Entry(crate::identity::entry::Action),
    Assignment(crate::identity::assignment::Action),
    Person(crate::identity::person::Change),
    SetLabel(String),
    #[cfg(test)]
    Invalid,
}

trait Driver: Send + 'static {
    fn info(&mut self) -> impl Future<Output = Result<Value>> + Send;
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
    has_reference_index: bool,
}
impl SqliteDriver {
    async fn open(data: &Path) -> Result<Self> {
        let mut connection = SqliteConnection::connect_with(
            &options(data)
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Full),
        )
        .await?;
        let has_reference_index =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM retrieval_profiles)")
                .fetch_one(&mut connection)
                .await?;
        Ok(Self {
            has_reference_index,
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
    async fn info(&mut self) -> Result<Value> {
        let version: String = sqlx::query_scalar("SELECT sqlite_version()")
            .fetch_one(&mut *self.connection())
            .await?;
        let compile_options: Vec<String> = sqlx::query_scalar("PRAGMA compile_options")
            .fetch_all(&mut *self.connection())
            .await?;
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&mut *self.connection())
            .await?;
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut *self.connection())
            .await?;
        let schema_version: Option<i64> =
            sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
                .fetch_one(&mut *self.connection())
                .await?;
        let synchronous: i64 = sqlx::query_scalar("PRAGMA synchronous")
            .fetch_one(self.connection())
            .await?;
        let busy_timeout: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
            .fetch_one(self.connection())
            .await?;
        Ok(
            json!({"sqlite_version":version,"compile_options":compile_options,"foreign_keys":foreign_keys,
            "journal_mode":journal_mode,"synchronous":synchronous,"busy_timeout_ms":busy_timeout,"observed_connection":"writer","schema_version":schema_version,"minimum_reader":schema_version}),
        )
    }

    async fn begin(&mut self) -> Result<()> {
        sqlx::query("BEGIN IMMEDIATE")
            .execute(self.connection())
            .await?;
        Ok(())
    }
    async fn apply(&mut self, action: &Mutation) -> Result<Value> {
        let label = match action {
            Mutation::ParticipationQq(receivers, groups) => {
                sqlx::query("DELETE FROM participation_qq_receivers")
                    .execute(self.connection())
                    .await?;
                sqlx::query("DELETE FROM participation_qq_groups")
                    .execute(self.connection())
                    .await?;
                for stage in receivers {
                    sqlx::query("INSERT INTO participation_qq_receivers VALUES(?)")
                        .bind(stage)
                        .execute(self.connection())
                        .await?;
                }
                for (namespace, group, stage, enabled) in groups {
                    sqlx::query("INSERT INTO participation_qq_groups VALUES(?,?,?,?)")
                        .bind(namespace)
                        .bind(group)
                        .bind(stage)
                        .bind(enabled)
                        .execute(self.connection())
                        .await?;
                }
                return Ok(Value::Null);
            }
            Mutation::ToolBudget(exec, actor, op) => {
                return crate::tools::budget::charge(self.connection(), exec, actor, op).await;
            }
            Mutation::SeedPausedGroup(namespace, group, stage) => {
                sqlx::query("INSERT INTO recovered_groups(namespace,native_group,stage,enabled,revision) VALUES(?,?,?,0,1) ON CONFLICT DO NOTHING")
                    .bind(namespace).bind(group).bind(stage).execute(self.connection()).await?;
                return Ok(Value::Null);
            }
            Mutation::ProfileConfig(service, request) => {
                return crate::model::profile_store::apply(self.connection(), service, request)
                    .await;
            }
            Mutation::Control(service, request) => {
                return crate::operator::domain::apply(self.connection(), service, request).await;
            }
            Mutation::Personal(change) => {
                return crate::host::personal::apply(self.connection(), change).await;
            }
            Mutation::Host(change) => {
                return crate::host::domain::apply(self.connection(), change).await;
            }
            Mutation::Vector(change) => {
                if matches!(**change, crate::memory::embedding::Change::OpenReference(_)) {
                    self.has_reference_index = true;
                }

                return crate::memory::embedding::apply(self.connection(), change).await;
            }
            Mutation::Extraction(change) => {
                return crate::memory::extract::apply(self.connection(), change).await;
            }
            Mutation::Memory(change) => {
                return crate::memory::apply(self.connection(), change).await;
            }
            Mutation::Media(change) => return crate::media::apply(self.connection(), change).await,
            Mutation::WorkSignal(action) => {
                return crate::work::signal::apply(self.connection(), action).await;
            }
            Mutation::Work(call) => return crate::work::apply(self.connection(), call).await,
            Mutation::Tool(change) => return crate::tools::apply(self.connection(), change).await,
            Mutation::History(change) => {
                return crate::history::apply(self.connection(), change).await;
            }
            Mutation::Social(change) => {
                return crate::social::apply(self.connection(), change).await;
            }
            Mutation::Main(cfg, change) => {
                return crate::runtime::main::apply(self.connection(), cfg, change).await;
            }
            Mutation::RecoverGroup(request) => {
                return crate::ingress::group_recovery::apply(self.connection(), request).await;
            }
            Mutation::Person(change) => {
                return crate::identity::person::apply(self.connection(), change).await;
            }
            Mutation::Assignment(action) => {
                return crate::identity::assignment::apply(self.connection(), action).await;
            }
            Mutation::Entry(action) => {
                return crate::identity::entry::apply(self.connection(), action).await;
            }
            Mutation::Ingress(request) => {
                return crate::ingress::admit::apply(self.connection(), request).await;
            }
            Mutation::Claim(event) => {
                return crate::ingress::admit::claim(self.connection(), *event).await;
            }
            Mutation::ObserveQqAccount(namespace, native) => {
                let conn = self.connection();
                let id = crate::identity::new_id(conn).await?;
                sqlx::query(
                    "INSERT INTO accounts(id,platform,namespace,native) VALUES(?,'qq',?,?)
                    ON CONFLICT(platform,namespace,native) DO NOTHING",
                )
                .bind(id)
                .bind(namespace)
                .bind(native)
                .execute(&mut *conn)
                .await?;
                let original: String = sqlx::query_scalar(
                    "SELECT id FROM accounts WHERE platform='qq' AND namespace=? AND native=?",
                )
                .bind(namespace)
                .bind(native)
                .fetch_one(conn)
                .await?;
                return Ok(json!(original));
            }
            Mutation::Effect(action) => {
                return crate::runtime::effects::apply(self.connection(), action).await;
            }
            Mutation::Privacy(action) => {
                return crate::privacy::apply(self.connection(), action).await;
            }
            Mutation::Search(c) => return crate::model::search::apply(self.connection(), c).await,
            Mutation::Model(action) => {
                return crate::model::attempt::apply(self.connection(), action).await;
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
        if self.has_reference_index {
            crate::memory::index::sync(self.connection()).await?;
        }
        #[cfg(test)]
        tests::commit_barrier("before").await;
        sqlx::query("COMMIT").execute(self.connection()).await?;
        #[cfg(test)]
        tests::commit_barrier("after").await;
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
    Inspect(oneshot::Sender<Result<Value>>),
    Write(Mutation, oneshot::Sender<Result<Value>>),
    Shutdown(oneshot::Sender<Result<()>>),
}

#[derive(Clone)]
pub struct Store {
    pub(crate) profile_files: Option<Arc<crate::model::profile_store::Service>>,
    pub(crate) control: Option<Arc<crate::operator::domain::Service>>,
    pub media: Option<Arc<crate::media::Runtime>>,
    pub host: Option<Arc<crate::host::Hub>>,
    pub vectors: Option<Arc<crate::memory::embedding::Engine>>,
    pub search: Option<Arc<crate::model::search::Engine>>,
    pub main: Option<Arc<crate::runtime::main::Config>>,
    pub work_segments: crate::runtime::main::segment::Config,
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
    pub fn subscribe_commits(&self) -> broadcast::Receiver<u64> {
        self.signals.hints.subscribe()
    }
    pub async fn open(data: &Path) -> Result<Self> {
        if !data.join("core.sqlite").is_file() {
            return Err("database missing; use init/migrate".into());
        }
        bootstrap(data).await?;
        let mut driver = SqliteDriver::open(data).await?;
        crate::model::attempt::recover_unanswered(driver.connection()).await?;
        crate::social::send::recover(driver.connection()).await?;
        crate::work::management::recover(driver.connection()).await?;
        Ok(Self::start(driver, read_pool(data).await?))
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
                    Command::Inspect(reply) => {
                        if !reply.is_closed() {
                            let _ = reply.send(driver.info().await);
                        }
                    }
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
            profile_files: None,
            control: None,
            vectors: None,
            media: None,
            search: None,
            main: None,
            work_segments: Default::default(),
            host: None,
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
        let (reply, result) = oneshot::channel();
        self.commands
            .send(Command::Inspect(reply))
            .await
            .map_err(|_| "writer stopped")?;
        result.await?
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
pub(crate) mod tests;
