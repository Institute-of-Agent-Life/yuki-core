//! Read the model's text-send result from the same acknowledged child and event.
//! Transport locators and planned text cannot substitute for that event.
use crate::Result;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub(crate) const CHANGED: &str = "send_receipt_source_changed";
#[derive(Debug)]
pub(crate) struct SourceChanged;
impl std::fmt::Display for SourceChanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(CHANGED)
    }
}
impl std::error::Error for SourceChanged {}

pub(crate) async fn confirmed_text(
    db: &mut SqliteConnection,
    exec: &str,
    op: &str,
) -> Result<Option<String>> {
    let Some(r)=sqlx::query(
        "SELECT p.actor,p.entry,p.stage,p.scope,p.window,anchor.event,
                CASE p.scope WHEN 'private' THEN dst.original_person ELSE space.id END AS subject
         FROM social_plans p JOIN operations o ON o.namespace=p.namespace AND o.operation_id=p.operation_id
         JOIN social_parts s ON s.namespace=p.namespace AND s.operation_id=p.operation_id AND s.ordinal=0
         JOIN social_outbound ack ON ack.namespace=s.namespace AND ack.child=s.child AND ack.entry=p.entry AND ack.stage=p.stage
         JOIN personal_deliveries anchor ON anchor.outbound=ack.id
         JOIN entries e ON e.id=p.entry LEFT JOIN contact_spaces space ON space.space_key=e.space
         LEFT JOIN assignments dst ON dst.id=p.recipient_stage
         WHERE p.namespace=? AND p.operation_id=? AND o.action='external' AND o.target='send' AND o.state='confirmed'
         AND s.state='confirmed' AND s.wire_action IN('send_private_msg','send_group_msg')
         AND json_array_length(s.params,'$.message')=1 AND json_extract(s.params,'$.message[0].type')='text'
         AND NOT EXISTS(SELECT 1 FROM social_parts n WHERE n.namespace=p.namespace AND n.operation_id=p.operation_id AND n.ordinal>0)"
    ).bind(exec).bind(op).fetch_optional(&mut *db).await? else {return Ok(None)};
    let Some(subject) = r.try_get::<Option<String>, _>("subject")? else {
        return Ok(None);
    };
    let actor: &str = r.try_get("actor")?;
    let entry: &str = r.try_get("entry")?;
    let event: i64 = r.try_get("event")?;
    let stage: &str = r.try_get("stage")?;
    let scope = crate::history::query::members::scope(db, exec, actor).await?;
    let live: Option<String> = if scope.entries.iter().any(|e| e == entry) {
        sqlx::query_scalar(
            "SELECT json_extract(l.body,'$.message[0].data.text') FROM personal_live_deliveries l
             JOIN history_public_events h ON h.event=l.event AND h.outbound=l.id AND h.entry=l.entry
             JOIN social_parts s ON s.child=l.child AND s.namespace=l.namespace
             JOIN entries e ON e.id=l.entry AND e.closed_at IS NULL
             JOIN assignments b ON b.id=l.stage AND b.status='active'
             LEFT JOIN main_windows w ON w.conversation=l.entry
             WHERE l.namespace=? AND s.operation_id=? AND l.event=? AND l.entry=? AND l.stage=?
             AND coalesce(w.generation,1)=? AND json_array_length(l.body,'$.message')=1
             AND json_extract(l.body,'$.message[0].type')='text'
             AND json_type(l.body,'$.message[0].data.text')='text'",
        )
        .bind(exec)
        .bind(op)
        .bind(event)
        .bind(entry)
        .bind(stage)
        .bind(r.try_get::<i64, _>("window")?)
        .fetch_optional(&mut *db)
        .await?
    } else {
        None
    };
    let mut data = json!({"operation_id":op,"action":"send_message","status":"succeeded",
      "target":{"kind":if r.try_get::<&str,_>("scope")?=="private" {"person"} else {"space"},"id":subject},
      "presence_id":stage,"event_id":live.as_ref().map(|_|event),"original_event_id":event,
      "original_conversation_id":entry,"error_category":Value::Null,"text_planned":true,"delivered_text_available":live.is_some()});
    if let Some(text) = live {
        sqlx::query("INSERT OR IGNORE INTO main_dependencies(execution,event) VALUES(?,?)")
            .bind(exec)
            .bind(event)
            .execute(&mut *db)
            .await?;
        data["delivered_text"] = json!(text);
    }
    Ok(Some(json!({"ok":true,"data":data,"retryable":false,"mutation_committed":true,"provider_id":"core","tool_name":"send"}).to_string()))
}

/// Previously sent provider bodies are immutable: once any included delivery loses
/// its read proof, stop this execution instead of sending copied text or a derived reply.
pub(crate) async fn stale(db: &mut SqliteConnection, exec: &str, actor: &str) -> Result<bool> {
    let rows=sqlx::query("SELECT a.entry,
        EXISTS(SELECT 1 FROM personal_live_deliveries l
            JOIN history_public_events h ON h.event=l.event AND h.outbound=l.id AND h.entry=l.entry
            JOIN entries e ON e.id=l.entry AND e.closed_at IS NULL
            JOIN assignments b ON b.id=l.stage AND b.status='active'
            LEFT JOIN main_windows w ON w.conversation=l.entry
            WHERE l.id=a.id AND l.event=d.event AND coalesce(w.generation,1)=p.window) AS visible
        FROM main_dependencies d JOIN personal_deliveries key ON key.event=d.event
        JOIN social_outbound a ON a.id=key.outbound JOIN social_parts s ON s.namespace=a.namespace AND s.child=a.child
        JOIN social_plans p ON p.namespace=s.namespace AND p.operation_id=s.operation_id
        WHERE d.execution=? AND a.namespace=d.execution").bind(exec).fetch_all(&mut *db).await?;
    if rows.is_empty() {
        return Ok(false);
    }
    let scope = crate::history::query::members::scope(db, exec, actor).await?;
    for row in rows {
        if !row.try_get::<bool, _>("visible")?
            || !scope.entries.contains(&row.try_get::<String, _>("entry")?)
        {
            return Ok(true);
        }
    }
    Ok(false)
}
