use super::{checkpoint, hash};
use crate::{Result, runtime::effects};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

#[derive(Clone)]
pub(super) struct Fragment {
    pub item: i64,
    pub revision: i64,
    pub key: String,
    pub digest: String,
    pub message: Value,
    pub event: Option<i64>,
    pub producer: Option<String>,
}
pub(super) async fn current(
    conn: &mut SqliteConnection,
    item: i64,
    zone: &str,
) -> Result<Option<Fragment>> {
    let row=sqlx::query("SELECT h.id,h.revision,h.source_event,h.outbound,e.envelope,o.body,o.namespace,o.child FROM history_items h LEFT JOIN events e ON e.id=h.source_event LEFT JOIN social_outbound o ON o.id=h.outbound
      WHERE h.id=? AND h.visible AND ((h.source_event IS NOT NULL AND json_extract(e.envelope,'$.kind')='message' AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)) OR (h.outbound IS NOT NULL AND o.body!='{}' AND NOT o.recalled))")
        .bind(item).fetch_optional(&mut *conn).await?;
    let Some(row) = row else { return Ok(None) };
    let event = row.try_get("source_event")?;
    let producer: Option<String> = row.try_get("namespace")?;
    let (key, source, message) = if let Some(event) = event {
        let raw: String = row.try_get("envelope")?;
        let source: Value = serde_json::from_str(&raw)?;
        (
            format!("event:{event}"),
            source,
            json!({"role":"user","content":crate::context::event::line(conn,event,zone).await?}),
        )
    } else {
        let raw: Value = serde_json::from_str(row.try_get("body")?)?;
        let text = crate::history::outbound_text(row.try_get("body")?)?;
        (
            json!(["outbound", producer, row.try_get::<String, _>("child")?]).to_string(),
            raw,
            json!({"role":"assistant","content":text}),
        )
    };
    if message["content"].as_str().is_none_or(str::is_empty) {
        return Ok(None);
    }
    Ok(Some(Fragment {
        item,
        revision: row.try_get("revision")?,
        key,
        digest: hash(&source),
        message,
        event,
        producer,
    }))
}
pub(super) async fn bind(conn: &mut SqliteConnection, exec: &str, f: &Fragment) -> Result<()> {
    if let Some(event) = f.event {
        sqlx::query("INSERT OR IGNORE INTO main_dependencies VALUES(?,?)")
            .bind(exec)
            .bind(event)
            .execute(&mut *conn)
            .await?;
    }
    if let Some(producer) = &f.producer {
        sqlx::query("INSERT OR IGNORE INTO main_dependencies SELECT ?1,e.id FROM events e WHERE e.id IN(SELECT source FROM executions WHERE id=?2 UNION SELECT event FROM main_dependencies WHERE execution=?2)").bind(exec).bind(producer).execute(conn).await?;
    }
    Ok(())
}
pub(super) async fn head(
    conn: &mut SqliteConnection,
    entry: &str,
    generation: i64,
) -> Result<Option<(String, i64, i64, String)>> {
    Ok(sqlx::query_as("SELECT id,last_item,coverage,summary FROM compact_jobs WHERE entry=? AND generation=? AND state='published' AND summary IS NOT NULL ORDER BY last_item DESC,rowid DESC LIMIT 1").bind(entry).bind(generation).fetch_optional(conn).await?)
}
pub(super) async fn active_checkpoint(
    conn: &mut SqliteConnection,
    id: Option<&str>,
) -> Result<Vec<Value>> {
    let Some(id) = id else {
        return Ok(Vec::new());
    };
    let (coverage,summary):(i64,String)=sqlx::query_as("SELECT coverage,summary FROM compact_jobs WHERE id=? AND state='published' AND summary IS NOT NULL").bind(id).fetch_one(conn).await?;
    Ok(vec![checkpoint(&summary, coverage)])
}
/// A continuation still carries its previously paid bytes. Hold it if its frozen
/// source or adopted checkpoint was withdrawn before the next physical request.
pub async fn stale(conn: &mut SqliteConnection, exec: &str) -> Result<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM compact_views v JOIN compact_jobs j ON j.id=v.checkpoint WHERE v.execution=?1 AND j.state!='published')
         OR EXISTS(SELECT 1 FROM compact_view_items f JOIN history_items h ON h.id=f.item LEFT JOIN social_outbound o ON o.id=h.outbound
          WHERE f.execution=?1 AND (f.message IS NULL OR NOT h.visible OR h.revision!=f.revision OR o.recalled))")
        .bind(exec).fetch_one(conn).await?)
}
pub async fn prepare(
    conn: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    static_program: &str,
    zone: &str,
) -> Result<Vec<Value>> {
    effects::authorize(conn, exec, actor, true).await?;
    let row=sqlx::query("SELECT t.conversation,t.window,x.source FROM main_turns t JOIN executions x ON x.id=t.execution WHERE t.execution=?").bind(exec).fetch_one(&mut *conn).await?;
    let entry: String = row.try_get("conversation")?;
    let generation: i64 = row.try_get("window")?;
    // A registered non-human source has only the material explicitly granted at registration.
    if row.try_get::<Option<i64>, _>("source")?.is_none() {
        return crate::work::questions::resume_history(conn, exec).await;
    }
    let saved: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM compact_views WHERE execution=?)")
            .bind(exec)
            .fetch_one(&mut *conn)
            .await?;
    if !saved {
        let previous = super::job::current_published(conn, &entry, generation).await?;
        let since = previous.as_ref().map(|p| p.1).unwrap_or(0);
        let prior = previous.as_ref().map(|p| p.0.as_str());
        sqlx::query("INSERT INTO compact_views VALUES(?,?,?,?,?)")
            .bind(exec)
            .bind(&entry)
            .bind(generation)
            .bind(prior)
            .bind(static_program)
            .execute(&mut *conn)
            .await?;
        if let Some(prior) = prior {
            sqlx::query("INSERT OR IGNORE INTO main_dependencies SELECT ?,d.event FROM compact_jobs j JOIN main_dependencies d ON d.execution=j.execution WHERE j.id=?").bind(exec).bind(prior).execute(&mut *conn).await?;
        }
        let ceiling:i64=sqlx::query_scalar("SELECT coalesce((SELECT h.id FROM history_items h JOIN executions x ON x.source=h.source_event WHERE x.id=?),0)").bind(exec).fetch_one(&mut *conn).await?;
        let ids:Vec<i64>=sqlx::query_scalar("SELECT h.id FROM history_items h LEFT JOIN events e ON e.id=h.source_event LEFT JOIN social_outbound o ON o.id=h.outbound LEFT JOIN main_turns t ON t.execution=o.namespace
          WHERE h.entry=?1 AND h.visible AND h.id>?2 AND h.id<?3 AND (
           (e.id IS NOT NULL AND EXISTS(SELECT 1 FROM accepted_sources WHERE event_id=e.id) AND e.id>coalesce((SELECT max(c.event) FROM main_controls c JOIN events ctrl ON ctrl.id=c.event WHERE ctrl.entry_id=?1 AND c.command='/ai new' AND c.generation=?4),0)
            AND NOT EXISTS(SELECT 1 FROM main_controls WHERE event=e.id) AND NOT EXISTS(SELECT 1 FROM host_command_events WHERE event=e.id) AND NOT EXISTS(SELECT 1 FROM memory_command_events WHERE event=e.id) AND NOT EXISTS(SELECT 1 FROM compact_manual WHERE event=e.id) AND NOT EXISTS(SELECT 1 FROM work_explicit_answers WHERE event=e.id) AND NOT EXISTS(SELECT 1 FROM work_wait_routed WHERE event=e.id))
           OR (o.id IS NOT NULL AND t.window=?4)) ORDER BY h.id")
            .bind(&entry).bind(since).bind(ceiling).bind(generation).fetch_all(&mut *conn).await?;
        for item in ids {
            if let Some(f) = current(conn, item, zone).await? {
                bind(conn, exec, &f).await?;
                sqlx::query("INSERT INTO compact_view_items VALUES(?,?,?,?,?,?,?)")
                    .bind(exec)
                    .bind(item)
                    .bind(f.item)
                    .bind(f.revision)
                    .bind(f.key)
                    .bind(f.digest)
                    .bind(f.message.to_string())
                    .execute(&mut *conn)
                    .await?;
            }
        }
    }
    messages(conn, exec).await
}
pub(super) async fn messages(conn: &mut SqliteConnection, exec: &str) -> Result<Vec<Value>> {
    let prior: Option<String> =
        sqlx::query_scalar("SELECT checkpoint FROM compact_views WHERE execution=?")
            .bind(exec)
            .fetch_optional(&mut *conn)
            .await?
            .flatten();
    let mut out = active_checkpoint(conn, prior.as_deref()).await?;
    let rows:Vec<String>=sqlx::query_scalar("SELECT message FROM compact_view_items WHERE execution=? AND message IS NOT NULL ORDER BY ordinal").bind(exec).fetch_all(conn).await?;
    for row in rows {
        out.push(serde_json::from_str(&row)?);
    }
    Ok(out)
}
