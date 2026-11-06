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

/// Only fully acknowledged file + caption sends have this original aggregate shape.
pub(crate) async fn file_and_caption(
    db: &mut SqliteConnection,
    exec: &str,
    op: &str,
) -> Result<Option<String>> {
    let rows=sqlx::query(
        "SELECT p.actor,p.entry,p.stage,p.scope,p.window,s.ordinal,s.child,s.wire_action,
         CASE p.scope WHEN 'private' THEN dst.original_person ELSE space.id END AS subject,
         d.event,d.body,v.id AS artifact,v.filename,v.digest,v.size,v.kind
         FROM social_plans p
         JOIN operations o ON o.namespace=p.namespace AND o.operation_id=p.operation_id
         JOIN social_parts s ON s.namespace=p.namespace AND s.operation_id=p.operation_id
         JOIN personal_live_deliveries d ON d.namespace=s.namespace AND d.child=s.child AND d.entry=p.entry AND d.stage=p.stage
         JOIN history_public_events h ON h.event=d.event AND h.outbound=d.id AND h.entry=p.entry
         JOIN entries e ON e.id=p.entry AND e.closed_at IS NULL
         JOIN assignments bot ON bot.id=p.stage AND bot.status='active'
         LEFT JOIN main_windows w ON w.conversation=e.id
         LEFT JOIN assignments dst ON dst.id=p.recipient_stage
         LEFT JOIN contact_spaces space ON space.space_key=e.space
         LEFT JOIN personal_artifact_events v ON v.event=d.event AND v.namespace=d.namespace
              AND EXISTS(SELECT 1 FROM media_version_readers r WHERE r.version=v.id AND r.reader=p.namespace)
         WHERE p.namespace=? AND p.operation_id=? AND o.action='external' AND o.target='send'
           AND o.state='confirmed' AND s.state='confirmed' AND coalesce(w.generation,1)=p.window
           AND (SELECT count(*) FROM social_parts all_parts WHERE all_parts.namespace=p.namespace AND all_parts.operation_id=p.operation_id)=2
         ORDER BY s.ordinal"
    ).bind(exec).bind(op).fetch_all(&mut *db).await?;
    let [file, caption] = rows.as_slice() else {
        return Ok(None);
    };
    if file.try_get::<i64, _>("ordinal")? != 0
        || caption.try_get::<i64, _>("ordinal")? != 1
        || !matches!(
            file.try_get::<&str, _>("wire_action")?,
            "upload_private_file" | "upload_group_file"
        )
        || !matches!(
            caption.try_get::<&str, _>("wire_action")?,
            "send_private_msg" | "send_group_msg"
        )
        || file.try_get::<Option<&str>, _>("kind")? != Some("file")
    {
        return Ok(None);
    }
    let entry: &str = file.try_get("entry")?;
    let scope = crate::history::query::members::scope(db, exec, file.try_get("actor")?).await?;
    let Some(subject) = file.try_get::<Option<&str>, _>("subject")? else {
        return Ok(None);
    };
    if !scope.entries.iter().any(|e| e == entry) {
        return Ok(None);
    }
    let body: Value = serde_json::from_str(caption.try_get("body")?)?;
    let Some([segment]) = body["message"].as_array().map(Vec::as_slice) else {
        return Ok(None);
    };
    let Some(text) = segment["data"]["text"]
        .as_str()
        .filter(|_| segment["type"] == "text")
    else {
        return Ok(None);
    };
    let target = json!({"kind":if file.try_get::<&str,_>("scope")?=="private"{"person"}else{"space"},"id":subject});
    let file_meta = json!({"artifact_id":file.try_get::<&str,_>("artifact")?,
        "name":file.try_get::<&str,_>("filename")?, "filename":file.try_get::<&str,_>("filename")?,
        "version":file.try_get::<&str,_>("digest")?, "size":file.try_get::<i64,_>("size")?,"attachment_kind":"file"});
    let mut parts = Vec::new();
    for (row, is_caption) in [(file, false), (caption, true)] {
        let event: i64 = row.try_get("event")?;
        let mut child = json!({"operation_id":row.try_get::<&str,_>("child")?,"action":"send_message",
            "status":"succeeded","target":target,"presence_id":row.try_get::<&str,_>("stage")?,
            "event_id":event,"original_event_id":event,"original_conversation_id":entry,"error_category":null,
            "delivered_text_available":true,"text_planned":is_caption,"delivered_text":if is_caption{text}else{""}});
        if !is_caption {
            child
                .as_object_mut()
                .unwrap()
                .extend(file_meta.as_object().unwrap().clone());
        }
        parts.push(child);
        sqlx::query("INSERT OR IGNORE INTO main_dependencies(execution,event) VALUES(?,?)")
            .bind(exec)
            .bind(event)
            .execute(&mut *db)
            .await?;
    }
    let mut data = file_meta;
    data.as_object_mut().unwrap().extend(json!({"operation_id":op,"status":"succeeded","target":target,
        "event_id":null,"original_event_id":null,"original_conversation_id":null,"error_category":null,
        "planned_messages":2,"sent_messages":2,"parts":parts}).as_object().unwrap().clone());
    Ok(Some(
        json!({"ok":true,"data":data,"retryable":false,"mutation_committed":true,
        "provider_id":"core","tool_name":"send"})
        .to_string(),
    ))
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
            WHERE l.id=a.id AND l.event=d.event AND coalesce(w.generation,1)=p.window
            AND (s.wire_action NOT IN ('upload_private_file','upload_group_file') OR EXISTS(
                SELECT 1 FROM personal_artifact_events v JOIN media_version_readers reader ON reader.version=v.id
                WHERE v.event=l.event AND v.namespace=l.namespace AND reader.reader=d.execution))) AS visible
        FROM main_dependencies d JOIN personal_deliveries key ON key.event=d.event
        JOIN social_outbound a ON a.id=key.outbound JOIN social_parts s ON s.namespace=a.namespace AND s.child=a.child
        JOIN social_plans p ON p.namespace=s.namespace AND p.operation_id=s.operation_id
        WHERE d.execution=? AND (a.namespace=d.execution OR EXISTS(
            SELECT 1 FROM work_runs r JOIN work_runs sender ON sender.work=r.work
            JOIN work_mail m ON m.work=r.work AND m.kind='human_answer'
            JOIN work_wait_mail link ON link.mail=m.id AND link.source_event=d.event
            WHERE r.execution=d.execution AND sender.execution=a.namespace))").bind(exec).fetch_all(&mut *db).await?;
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
