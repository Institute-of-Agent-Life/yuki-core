use super::{
    Change,
    normalize::{self, Field},
};
use crate::{
    Result,
    store::{Mutation, Store},
};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::collections::BTreeSet;

pub struct Prepared {
    pub id: i64,
    pub revision: i64,
    pub fields: Vec<Field>,
    pub terms: String,
}

fn inbound(raw: &str) -> Result<String> {
    let frame: Value = serde_json::from_str(raw)?;
    if let Some(text) = frame["text"].as_str() {
        return Ok(text.into());
    }
    Ok(frame["segments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["text"].as_str())
        .collect())
}
pub(crate) fn outbound(raw: &str) -> Result<String> {
    let body: Value = serde_json::from_str(raw)?;
    if let Some(text) = body["text"].as_str() {
        return Ok(text.to_owned());
    }
    let mut text = String::new();
    for seg in body["message"].as_array().into_iter().flatten() {
        match seg["type"].as_str() {
            Some("text") => text.push_str(seg["data"]["text"].as_str().unwrap_or("")),
            Some("at") => {
                text.push('@');
                if let Some(target) = seg["data"]["qq"].as_str() {
                    text.push_str(target);
                }
            }
            _ => {}
        }
    }
    if text.is_empty()
        && let Some(name) = body["name"].as_str()
    {
        text.push_str(name);
    }
    Ok(text)
}

pub async fn prepare_batch(store: &Store, cutoff: i64) -> Result<usize> {
    let mut read = store.readers().begin().await?;
    let rows=sqlx::query("SELECT h.id,h.revision,h.entry,h.actor,e.envelope,h.source_event,o.body FROM history_items h
      LEFT JOIN events e ON e.id=h.source_event LEFT JOIN social_outbound o ON o.id=h.outbound
      WHERE h.id<=? AND h.visible AND h.revision!=h.indexed ORDER BY h.id LIMIT 32").bind(cutoff).fetch_all(&mut *read).await?;
    let mut inputs = Vec::new();
    for r in rows {
        let mut fields = Vec::new();
        let text = if let Some(raw) = r.try_get::<Option<String>, _>("envelope")? {
            inbound(&raw)?
        } else {
            outbound(r.try_get("body")?)?
        };
        fields.push(("content".to_string(), text));
        if let Some(source) = r.try_get::<Option<i64>, _>("source_event")? {
            let parts: Vec<(i64, String)> = sqlx::query_as(
                "SELECT segment,text FROM history_transcripts WHERE source=? ORDER BY segment",
            )
            .bind(source)
            .fetch_all(&mut *read)
            .await?;
            fields.extend(
                parts
                    .into_iter()
                    .map(|(n, text)| (format!("audio_transcript:{n}"), text)),
            );
        }
        inputs.push((
            r.try_get::<i64, _>("id")?,
            r.try_get::<i64, _>("revision")?,
            r.try_get::<String, _>("entry")?,
            fields,
        ));
    }
    drop(read);
    let size = inputs.len();
    let batch = tokio::task::spawn_blocking(move || -> Result<Vec<Prepared>> {
        inputs
            .into_iter()
            .map(|(id, revision, entry, raw)| {
                let fields = raw
                    .into_iter()
                    .map(|(label, text)| normalize::field(label, text))
                    .collect::<Result<Vec<_>>>()?;
                let terms = scoped_terms(&entry, normalize::grams(&fields));
                Ok(Prepared {
                    id,
                    revision,
                    fields,
                    terms,
                })
            })
            .collect()
    })
    .await??;
    for p in batch {
        store
            .write(Mutation::History(Box::new(Change::Index(p))))
            .await?;
    }
    Ok(size)
}
pub fn prefix(entry: &str) -> String {
    format!("c{}", entry.replace('-', ""))
}
fn scoped_terms(entry: &str, grams: BTreeSet<String>) -> String {
    let scope = prefix(entry);
    grams
        .into_iter()
        .map(|g| format!("{scope}{g}"))
        .collect::<Vec<_>>()
        .join(" ")
}
pub async fn publish(conn: &mut SqliteConnection, p: &Prepared) -> Result<Value> {
    let n=sqlx::query("UPDATE history_items SET indexed=revision WHERE id=? AND revision=? AND visible AND indexed!=revision")
        .bind(p.id).bind(p.revision).execute(&mut *conn).await?.rows_affected();
    if n == 0 {
        return Ok(json!({"published":false}));
    }
    sqlx::query("DELETE FROM history_fts WHERE rowid=?")
        .bind(p.id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM history_fields WHERE item=?")
        .bind(p.id)
        .execute(&mut *conn)
        .await?;
    for f in &p.fields {
        sqlx::query("INSERT INTO history_fields VALUES(?,?,?,?,?)")
            .bind(p.id)
            .bind(&f.label)
            .bind(&f.raw)
            .bind(&f.base)
            .bind(&f.simple)
            .execute(&mut *conn)
            .await?;
    }
    sqlx::query("INSERT INTO history_fts(rowid,terms) VALUES(?,?)")
        .bind(p.id)
        .bind(&p.terms)
        .execute(conn)
        .await?;
    Ok(json!({"published":true}))
}
