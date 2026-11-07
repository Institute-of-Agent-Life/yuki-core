use super::scope::{self, Reader};
use crate::{Result, runtime::effects};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    Event {
        id: i64,
        quote: String,
    },
    Tool {
        execution: String,
        operation: String,
        quote: String,
    },
    Run {
        execution: String,
        quote: String,
    },
}
pub struct Proof {
    pub kind: &'static str,
    pub key: String,
    pub version: String,
    pub author: String,
    pub entry: String,
    pub event: Option<i64>,
    pub execution: Option<String>,
    pub quote: String,
}

fn texts(value: &Value, strings: &mut Vec<String>) {
    match value {
        Value::String(s) => strings.push(s.clone()),
        Value::Array(items) => {
            for item in items {
                texts(item, strings)
            }
        }
        Value::Object(props) => {
            for item in props.values() {
                texts(item, strings)
            }
        }
        _ => {}
    }
}
/// Preserve existing text evidence versions. Transcript evidence binds the
/// exact original segment as well as its event. A referenced message is not
/// treated as speech authored by the sender of the reply.
pub async fn event_proof(conn: &mut SqliteConnection, id: i64, quote: &str) -> Result<Proof> {
    if quote.trim().is_empty() {
        return Err("empty event quote".into());
    }
    let row=sqlx::query("SELECT e.entry_id,e.raw_actor,e.envelope FROM events e WHERE e.id=? AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)").bind(id).fetch_one(&mut *conn).await?;
    let raw: Value = serde_json::from_str(row.try_get("envelope")?)?;
    let texts: Vec<&str> = raw["segments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["type"] == "text")
        .filter_map(|s| s["text"].as_str())
        .collect();
    let text = texts.join("");
    let source = if raw["text"].as_str().is_some_and(|s| s.contains(quote)) || text.contains(quote)
    {
        raw
    } else {
        let speech: Vec<(i64, String)> = sqlx::query_as(
            "SELECT segment,text FROM history_transcripts WHERE source=? ORDER BY segment",
        )
        .bind(id)
        .fetch_all(&mut *conn)
        .await?;
        let combined = std::iter::once(text.as_str())
            .chain(speech.iter().map(|(_, s)| s.as_str()))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        if speech.is_empty() || !combined.contains(quote) {
            return Err("quote is absent from original event text or transcript".into());
        }
        json!([raw, speech])
    };
    Ok(Proof {
        kind: "event",
        key: id.to_string(),
        version: scope::hash(&source),
        author: row.try_get("raw_actor")?,
        entry: row.try_get("entry_id")?,
        event: Some(id),
        execution: None,
        quote: quote.into(),
    })
}
/// Read retained receipts under a separately installed maintenance scope. Action dispatch
/// continues to use effects::authorize and the live original route.
pub(crate) async fn execution_scope(
    conn: &mut SqliteConnection,
    r: &Reader,
    execution: &str,
    actor: &str,
) -> Result<crate::history::query::Scope> {
    if !r.archived_evidence {
        effects::authorize(conn, execution, actor, false).await?;
        return crate::history::query::scope(conn, execution, actor).await;
    }
    let (subject, entry): (String, String) = sqlx::query_as(
        "SELECT actor,entry FROM maintenance_execution_access WHERE execution=? AND entry=?",
    )
    .bind(execution)
    .bind(&r.base.home)
    .fetch_one(conn)
    .await?;
    if !r.admits_entry(&entry) {
        return Err("original evidence unavailable".into());
    }
    Ok(crate::history::query::Scope {
        subject,
        home: entry.clone(),
        entries: vec![entry],
    })
}

pub async fn resolve(
    conn: &mut SqliteConnection,
    r: &Reader,
    actor: &str,
    source: &Source,
) -> Result<Proof> {
    let (kind, key, value, author, entry, event, execution, quote) = match source {
        Source::Event { id, quote } => {
            let original = event_proof(conn, *id, quote).await?;
            if !r.admits_entry(&original.entry) {
                return Err("original evidence unavailable".into());
            }
            return Ok(original);
        }
        Source::Tool {
            execution,
            operation,
            quote,
        } => {
            let origin = execution_scope(conn, r, execution, actor).await?;
            let raw:String=sqlx::query_scalar("SELECT result FROM operations WHERE namespace=? AND operation_id=? AND state='confirmed' AND result IS NOT NULL").bind(execution).bind(operation).fetch_one(&mut *conn).await?;
            (
                "tool",
                json!([execution, operation]).to_string(),
                serde_json::from_str(&raw)?,
                origin.subject,
                origin.home,
                None,
                Some(execution.clone()),
                quote,
            )
        }
        Source::Run { execution, quote } => {
            let origin = execution_scope(conn, r, execution, actor).await?;
            let raw:String=sqlx::query_scalar("SELECT t.result FROM main_turns t JOIN executions x ON x.id=t.execution WHERE t.execution=? AND x.state='completed' AND t.result IS NOT NULL").bind(execution).fetch_one(&mut *conn).await?;
            (
                "run",
                execution.clone(),
                serde_json::from_str(&raw)?,
                origin.subject,
                origin.home,
                None,
                Some(execution.clone()),
                quote,
            )
        }
    };
    if quote.trim().is_empty() || !r.admits_entry(&entry) {
        return Err("original evidence unavailable".into());
    }
    if kind != "event" {
        let mut text = Vec::new();
        texts(&value, &mut text);
        if !text.iter().any(|t| t.contains(quote)) {
            return Err("quote is absent from original result".into());
        }
    }
    Ok(Proof {
        kind,
        key,
        version: scope::hash(&value),
        author,
        entry,
        event,
        execution,
        quote: quote.clone(),
    })
}
pub async fn attach(conn: &mut SqliteConnection, fact: &str, proof: Proof) -> Result<()> {
    sqlx::query("INSERT INTO memory_evidence(fact,kind,source_key,source_version,entry,author,origin_execution,event,quote) VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(fact,kind,source_key,source_version,quote) DO NOTHING")
        .bind(fact).bind(proof.kind).bind(proof.key).bind(proof.version).bind(proof.entry).bind(proof.author).bind(proof.execution).bind(proof.event).bind(proof.quote).execute(conn).await?;
    Ok(())
}

/// Evidence version and visible count share the same source checks as the actual detail rows.
pub async fn readable(
    conn: &mut SqliteConnection,
    r: &Reader,
    actor: &str,
    fact: &str,
    only_home: bool,
) -> Result<Vec<Value>> {
    let entries = serde_json::to_string(&r.base.entries)?;
    let rows=sqlx::query("SELECT * FROM memory_evidence WHERE fact=? AND quote IS NOT NULL AND entry IN(SELECT value FROM json_each(?)) AND (NOT ? OR entry=?) ORDER BY id")
        .bind(fact).bind(entries).bind(only_home).bind(&r.base.home).fetch_all(&mut *conn).await?;
    let mut allowed = Vec::new();
    for row in rows {
        let kind: String = row.try_get("kind")?;
        let key: String = row.try_get("source_key")?;
        let quote: String = row.try_get("quote")?;
        let src = match kind.as_str() {
            "event" => Source::Event {
                id: row.try_get("event")?,
                quote,
            },
            "tool" => {
                let (execution, operation) = serde_json::from_str(&key)?;
                Source::Tool {
                    execution,
                    operation,
                    quote,
                }
            }
            "run" => Source::Run {
                execution: key.clone(),
                quote,
            },
            _ => return Err("invalid stored evidence kind".into()),
        };
        let proof = match resolve(conn, r, actor, &src).await {
            Ok(proof) => proof,
            Err(e)
                if e.downcast_ref::<sqlx::Error>()
                    .is_some_and(|e| !matches!(e, sqlx::Error::RowNotFound))
                    || e.downcast_ref::<serde_json::Error>().is_some() =>
            {
                return Err(e);
            }
            Err(_) => continue,
        };
        if proof.version != row.try_get::<String, _>("source_version")? {
            continue;
        }
        let mut item = json!({"id":row.try_get::<i64,_>("id")?,"kind":kind,"key":key,"version":proof.version,"entry":proof.entry,"author":proof.author,"quote":proof.quote});
        if let Some(authority) = row.try_get::<Option<String>, _>("attribution")? {
            item["authority"] = json!(authority);
        }
        if row.try_get::<Option<&str>, _>("relation")? == Some("group_statement") {
            item["relation"] = json!("group_statement");
        }
        allowed.push(item);
    }
    Ok(allowed)
}
