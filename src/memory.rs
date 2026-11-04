mod commands;
mod evidence;
pub mod extract;
pub mod facts;
mod scope;
pub mod tools;
use crate::{
    Result,
    store::{Mutation, Store},
};
pub use commands::route;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;

#[derive(clap::Subcommand, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Query as the original execution. Evidence requires its own source authorization.
    Read {
        execution: String,
        actor: String,
        #[arg(long)]
        fact: Option<String>,
        #[arg(long, default_value = "")]
        cursor: String,
        #[arg(long, default_value_t = 20)]
        limit: i64,
        #[arg(long)]
        evidence: bool,
        #[arg(long)]
        admin: bool,
    },
    /// Apply one exact original operation. Supply the revision and proof returned by read.
    Edit {
        execution: String,
        actor: String,
        key: String,
        body: String,
        #[arg(long)]
        admin: bool,
    },
}
pub enum Change {
    Tool {
        exec: String,
        actor: String,
        op: String,
    },
    Edit {
        exec: String,
        actor: String,
        key: String,
        edit: Box<facts::Edit>,
        admin: bool,
    },
    Reported(i64, String),
}
pub async fn invoke(store: &Store, action: Action) -> Result<Value> {
    match action {
        Action::Read {
            execution,
            actor,
            fact,
            cursor,
            limit,
            evidence,
            admin,
        } => {
            let mut tx = store.readers().begin().await?;
            let reader = scope::Reader::at(&mut tx, &execution, &actor, admin, false).await?;
            facts::read(
                &mut tx,
                &reader,
                fact.as_deref(),
                &cursor,
                limit,
                evidence,
                false,
            )
            .await
        }
        Action::Edit {
            execution,
            actor,
            key,
            body,
            admin,
        } => {
            let edit = serde_json::from_str(&body)?;
            store
                .write(Mutation::Memory(Box::new(Change::Edit {
                    exec: execution,
                    actor,
                    key,
                    edit,
                    admin,
                })))
                .await
        }
    }
}
pub async fn apply(conn: &mut SqliteConnection, change: &Change) -> Result<Value> {
    match change {
        Change::Tool { exec, actor, op } => tools::apply(conn, exec, actor, op).await,
        Change::Edit {
            exec,
            actor,
            key,
            edit,
            admin,
        } => {
            let reader = scope::Reader::at(conn, exec, actor, *admin, true).await?;
            facts::edit(conn, &reader, exec, actor, key, edit, false).await
        }
        Change::Reported(event, state) => {
            sqlx::query("UPDATE memory_command_events SET reply_state=? WHERE event=? AND reply_state IS NULL").bind(state).bind(event).execute(conn).await?;
            Ok(json!({"event":event,"reply":state}))
        }
    }
}
pub fn start(store: Store, qq: crate::gateway::qq::Hub) -> tokio::task::JoinHandle<()> {
    tokio::spawn(commands::deliver(store, qq))
}
