pub(crate) mod index;
pub(crate) use index::outbound as outbound_text;
pub(crate) mod normalize;
mod opencc;
pub mod query;

use crate::{
    Result,
    runtime::effects,
    store::{Mutation, Store},
};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
#[derive(Serialize, Deserialize, Subcommand)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    ChatQuery {
        execution: String,
        actor: String,
        args: String,
        #[arg(long)]
        operation: Option<String>,
    },
    Query {
        execution: String,
        actor: String,
        args: String,
        #[arg(long)]
        operation: Option<String>,
    },
    Directory {
        execution: String,
        actor: String,
    },
    Transcribe {
        execution: String,
        actor: String,
        source: i64,
        segments: String,
    },
}
pub enum Change {
    PublicAnchor {
        exec: String,
        actor: String,
        key: String,
        args: String,
        result: Value,
    },
    Index(index::Prepared),
    Transcript(String, String, i64, String),
    Anchor {
        exec: String,
        actor: String,
        key: String,
        args: String,
        rows: Vec<Value>,
        cursor: Option<i64>,
    },
}

pub(crate) async fn authorize_audio_source(
    conn: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    source: i64,
) -> Result<()> {
    effects::authorize(conn, exec, actor, true).await?;
    let owns_source: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM executions x JOIN events e ON e.id=?2
         WHERE x.id=?1 AND e.raw_actor=?3
         AND (x.source=e.id OR EXISTS(SELECT 1 FROM main_inputs i
              WHERE i.execution=x.id AND i.event=e.id)))",
    )
    .bind(exec)
    .bind(source)
    .bind(actor)
    .fetch_one(&mut *conn)
    .await?;
    if !owns_source {
        return Err("transcription requires original active source authority".into());
    }
    crate::ingress::admit::validate_claim(conn, source).await?;
    if crate::privacy::source::execution_erased(conn, exec, Some(source)).await? {
        return Err("transcription source erased".into());
    }
    Ok(())
}

async fn original_audio_segments(
    conn: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    source: i64,
) -> Result<Vec<i64>> {
    authorize_audio_source(conn, exec, actor, source).await?;
    Ok(sqlx::query_scalar(
        "SELECT CAST(s.key AS INTEGER) FROM events e JOIN json_each(e.envelope,'$.segments') s
         WHERE e.id=? AND json_extract(s.value,'$.type')='media'
         AND json_extract(s.value,'$.kind') IN ('record','audio')",
    )
    .bind(source)
    .fetch_all(conn)
    .await?)
}

async fn revise_transcript_index(conn: &mut SqliteConnection, source: i64) -> Result<()> {
    sqlx::query("UPDATE history_items SET revision=revision+1 WHERE source_event=? AND visible")
        .bind(source)
        .execute(conn)
        .await?;
    Ok(())
}

pub(crate) async fn project_transcript(
    conn: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    source: i64,
    segment: i64,
    text: &str,
) -> Result<bool> {
    if text.is_empty() || text.len() > 32768 {
        return Ok(false);
    }
    if !original_audio_segments(conn, exec, actor, source)
        .await?
        .contains(&segment)
    {
        return Err("transcript must refer to an original audio segment".into());
    }
    let inserted = sqlx::query(
        "INSERT INTO history_transcripts(source,segment,text) VALUES(?,?,?)
         ON CONFLICT(source,segment) DO NOTHING",
    )
    .bind(source)
    .bind(segment)
    .bind(text)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if inserted > 0 {
        revise_transcript_index(conn, source).await?;
        return Ok(true);
    }
    let current: String =
        sqlx::query_scalar("SELECT text FROM history_transcripts WHERE source=? AND segment=?")
            .bind(source)
            .bind(segment)
            .fetch_one(conn)
            .await?;
    Ok(current == text)
}

pub async fn apply(conn: &mut SqliteConnection, change: &Change) -> Result<Value> {
    match change {
        Change::PublicAnchor {
            exec,
            actor,
            key,
            args,
            result,
        } => query::original::anchor(conn, exec, actor, key, args, result).await,
        Change::Index(p) => index::publish(conn, p).await,
        Change::Transcript(exec, actor, source, segments) => {
            let attachments = original_audio_segments(conn, exec, actor, *source).await?;
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Transcript {
                source: String,
                segment_index: i64,
                text: String,
            }
            let fields: Vec<Transcript> = serde_json::from_str(segments)?;
            if fields.is_empty()
                || fields.iter().any(|f| {
                    !matches!(f.source.as_str(), "current" | "reply")
                        || f.segment_index < 0
                        || f.source == "current" && !attachments.contains(&f.segment_index)
                        || f.text.is_empty()
                })
            {
                return Err("transcript must refer to an original audio segment".into());
            }
            sqlx::query("DELETE FROM history_transcripts WHERE source=?")
                .bind(source)
                .execute(&mut *conn)
                .await?;
            for f in fields.into_iter().filter(|f| f.source == "current") {
                sqlx::query("INSERT INTO history_transcripts VALUES(?,?,?)")
                    .bind(source)
                    .bind(f.segment_index)
                    .bind(f.text)
                    .execute(&mut *conn)
                    .await?;
            }
            revise_transcript_index(conn, *source).await?;
            Ok(json!({"source":source}))
        }
        Change::Anchor {
            exec,
            actor,
            key,
            args,
            rows,
            cursor,
        } => query::anchor(conn, exec, actor, key, args, rows, *cursor).await,
    }
}
pub async fn invoke(store: &Store, action: Action) -> Result<Value> {
    match action {
        Action::ChatQuery {
            execution,
            actor,
            args,
            operation,
        } => query::original::run(store, &execution, &actor, &args, operation.as_deref()).await,
        Action::Query {
            execution,
            actor,
            args,
            operation,
        } => query::run(store, &execution, &actor, &args, operation.as_deref()).await,
        Action::Directory { execution, actor } => {
            let mut tx = store.readers().begin().await?;
            query::directory(&mut tx, &execution, &actor).await
        }
        Action::Transcribe {
            execution,
            actor,
            source,
            segments,
        } => {
            store
                .write(Mutation::History(Box::new(Change::Transcript(
                    execution, actor, source, segments,
                ))))
                .await
        }
    }
}

pub fn start(store: Store) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut hints = store.subscribe_commits();
        loop {
            match index::prepare_batch(&store, i64::MAX).await {
                Ok(0) => {
                    let _ =
                        tokio::time::timeout(std::time::Duration::from_secs(2), hints.recv()).await;
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("history indexing paused: {error}");
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
    })
}
