mod owner_erasure;
pub mod source;
use crate::{
    Result,
    store::{Mutation, Store},
};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;

#[derive(Subcommand, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Owner {
        id: String,
        #[arg(action=clap::ArgAction::Set)]
        enabled: bool,
    },
    Acquire {
        owner: String,
        item: String,
        source: i64,
    },
    Read {
        owner: String,
        item: String,
        source: i64,
    },
    Erase {
        key: String,
        person: String,
        sources: String,
    },
    Pending {
        owner: String,
    },
    Ack {
        owner: String,
        key: String,
        watermark: i64,
        proof: String,
    },
}

pub async fn invoke(store: &Store, action: Action) -> Result<Value> {
    match action {
        Action::Read {
            owner,
            item,
            source,
        } => {
            let mut tx = store.readers().begin().await?;
            source::read(&mut tx, &owner, &item, source).await
        }
        Action::Pending { owner } => {
            let mut tx = store.readers().begin().await?;
            owner_erasure::pending(&mut tx, &owner).await
        }
        update => store.write(Mutation::Privacy(update)).await,
    }
}

pub async fn apply(conn: &mut SqliteConnection, action: &Action) -> Result<Value> {
    match action {
        Action::Owner { id, enabled } => {
            sqlx::query("INSERT INTO source_owners(id,enabled) VALUES(?,?) ON CONFLICT(id) DO UPDATE SET enabled=excluded.enabled")
                .bind(id).bind(enabled).execute(conn).await?;
            Ok(json!({"owner":id,"enabled":enabled}))
        }
        Action::Acquire {
            owner,
            item,
            source,
        } => source::acquire(conn, owner, item, *source).await,
        Action::Erase {
            key,
            person,
            sources,
        } => owner_erasure::erase(conn, key, person, sources).await,
        Action::Ack {
            owner,
            key,
            watermark,
            proof,
        } => owner_erasure::ack(conn, owner, key, *watermark, proof).await,
        Action::Read { .. } | Action::Pending { .. } => {
            Err("read command belongs on snapshot reader".into())
        }
    }
}
