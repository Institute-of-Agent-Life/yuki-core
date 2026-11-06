//! A configured processor consumes the original model operation and selected event/slot.
//! Its output follows the same receipt and presentation checks as the ordinary Host wire.
use crate::{Result, media::handles, store::Store};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
const TOOL: &str = "inspect_conversation_attachment";
pub(crate) async fn pin(db: &mut SqliteConnection, exec: &str, name: Option<&str>) -> Result<()> {
    if let Some(name) = name {
        sqlx::query("INSERT INTO main_processor_bindings SELECT ?1,i.plugin,i.revision,json_extract(a.value,'$.deployment') FROM host_installations i JOIN core_meta a ON a.key='plugin-approval:'||i.plugin WHERE i.plugin=?2 AND i.enabled AND EXISTS(SELECT 1 FROM json_each(i.grants) WHERE value='media.process') AND EXISTS(SELECT 1 FROM json_each(i.grants) WHERE value='media.open')").bind(exec).bind(name).execute(db).await?;
    }
    Ok(())
}
pub(crate) async fn configured(db: &mut SqliteConnection, exec: &str) -> Result<bool> {
    Ok(
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM main_processor_bindings WHERE execution=?)",
        )
        .bind(exec)
        .fetch_one(db)
        .await?,
    )
}
async fn live(db: &mut SqliteConnection, exec: &str) -> Result<(String, String, String)> {
    let (id,revision,generation):(String,String,String)=sqlx::query_as("SELECT b.plugin,b.revision,i.generation FROM main_processor_bindings b JOIN host_installations i ON i.plugin=b.plugin AND i.revision=b.revision AND i.enabled AND i.online JOIN core_meta a ON a.key='plugin-approval:'||b.plugin AND json_extract(a.value,'$.deployment')=b.deployment WHERE b.execution=?").bind(exec).fetch_optional(&mut *db).await?.ok_or("original selected processor unavailable")?;
    super::domain::active(db, &id, &generation, "media.process").await?;
    super::domain::active(db, &id, &generation, "media.open").await?;
    Ok((id, revision, generation))
}
fn position(media: &Value) -> Result<(i64, i64)> {
    Ok((
        media["event"].as_i64().ok_or("missing selected event")?,
        media["slot"].as_i64().ok_or("missing selected slot")?,
    ))
}
pub(super) fn selected(r: &super::calls::Request) -> bool {
    r.method == "media.process" && r.name == TOOL
}
pub(super) async fn original_payload(
    db: &mut SqliteConnection,
    r: &super::calls::Request,
) -> Result<String> {
    let (plugin, revision, _) = live(db, &r.execution).await?;
    if (plugin.as_str(), revision.as_str()) != (r.plugin.as_str(), r.revision.as_str()) {
        return Err("selected processor deployment changed".into());
    }
    let row=sqlx::query("SELECT payload,parent,ordinal FROM operations WHERE namespace=? AND operation_id=? AND target=? AND action='external'").bind(&r.execution).bind(&r.operation).bind(TOOL).fetch_one(db).await?;
    let raw: String = row.get("payload");
    let model: Value = serde_json::from_str(&raw)?;
    let (event, slot) = position(&r.args["media"])?;
    if model["event_id"] != event
        || model["attachment_index"] != slot
        || row.get::<Option<String>, _>("parent") != r.parent
        || row.get::<Option<i64>, _>("ordinal") != r.ordinal
    {
        return Err("processor source differs from original tool call".into());
    }
    Ok(raw)
}
pub(super) async fn record(
    db: &mut SqliteConnection,
    r: &super::calls::Request,
    h: &str,
) -> Result<()> {
    let (event, slot) = position(&r.args["media"])?;
    sqlx::query("INSERT INTO main_processor_calls VALUES(?,?,?) ON CONFLICT(handle) DO NOTHING")
        .bind(h)
        .bind(event)
        .bind(slot)
        .execute(db)
        .await?;
    Ok(())
}
pub(crate) async fn check(db: &mut SqliteConnection, h: &str) -> Result<()> {
    let row=sqlx::query("SELECT h.execution,h.plugin,h.generation,p.event,p.slot,coalesce(x.original_actor,x.origin) AS actor,s.digest,s.version FROM main_processor_calls p JOIN host_calls h USING(handle) JOIN executions x ON x.id=h.execution LEFT JOIN host_media_selected s USING(handle) WHERE h.handle=?").bind(h).fetch_optional(&mut *db).await?;
    if let Some(r) = row {
        let exec: &str = r.get("execution");
        let (id, _, generation) = live(db, exec).await?;
        if id != r.get::<&str, _>("plugin") || generation != r.get::<&str, _>("generation") {
            return Err("selected processor handle invalidated".into());
        }
        let proof =
            handles::original(db, exec, r.get("actor"), r.get("event"), r.get("slot")).await?;
        if proof["state"] != "cached"
            || proof["digest"].as_str().is_none()
            || proof["digest"].as_str() != r.get::<Option<&str>, _>("digest")
        {
            return Err("selected processor input expired or replaced".into());
        }
    }
    Ok(())
}
pub(crate) async fn presentable(
    db: &mut SqliteConnection,
    exec: &str,
    operation: &str,
) -> Result<bool> {
    let h:Option<String>=sqlx::query_scalar("SELECT h.handle FROM host_calls h JOIN main_processor_calls p USING(handle) WHERE h.execution=? AND h.operation=?").bind(exec).bind(operation).fetch_optional(&mut *db).await?;
    if let Some(h) = h {
        return Ok(check(db, &h).await.is_ok());
    }
    Ok(true)
}
pub(crate) async fn process(
    store: &Store,
    exec: &str,
    op: &str,
    source: (i64, i64),
    purpose: &str,
    vision: bool,
) -> Result<()> {
    let mut tx = store.readers().begin().await?;
    let (id, revision, _) = live(&mut tx, exec).await?;
    let (parent,ordinal)=sqlx::query_as("SELECT parent,ordinal FROM operations WHERE namespace=? AND operation_id=? AND target=? AND state='intent'").bind(exec).bind(op).bind(TOOL).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    let hub = store.host.as_ref().ok_or("selected processor offline")?;
    let r = hub
        .execute(
            store,
            super::calls::Request {
                vision,
                plugin: id,
                revision,
                execution: exec.into(),
                operation: op.into(),
                method: "media.process".into(),
                name: TOOL.into(),
                args: json!({"media":{"event":source.0,"slot":source.1},"purpose":purpose}),
                parent,
                ordinal,
            },
        )
        .await?;
    if !matches!(
        r["state"].as_str(),
        Some("confirmed" | "failed" | "unknown")
    ) {
        return Err("original media call pending".into());
    }
    Ok(())
}

pub(crate) async fn receipt<'a>(
    db: &mut SqliteConnection,
    exec: &str,
    op: &str,
    original: &'a str,
) -> Result<std::borrow::Cow<'a, str>> {
    if !presentable(db, exec, op).await? {
        return Ok(r#"{"unavailable":true,"reason":"selected_processor_scope_changed"}"#.into());
    }
    if let Some(text) = crate::social::model_receipt::confirmed_text(db, exec, op).await? {
        return Ok(text.into());
    }
    if let Some(receipt) = crate::social::model_receipt::confirmed_parts(db, exec, op).await? {
        return Ok(receipt.into());
    }
    Ok(original.into())
}
