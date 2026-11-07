//! Original outbound text is paged through the current call handle without enlarging frames.
use crate::{Result, privacy::source, runtime::effects};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextRange {
    start: usize,
    length: usize,
    sha256: String,
}

fn selected<'a>(text: &'a str, range: &TextRange) -> Result<&'a str> {
    let end = range
        .start
        .checked_add(range.length)
        .ok_or("text range overflow")?;
    let piece = text
        .get(range.start..end)
        .filter(|s| !s.is_empty())
        .ok_or("invalid text range")?;
    if format!("{:x}", Sha256::digest(piece.as_bytes())) != range.sha256 {
        return Err("original text range digest differs".into());
    }
    Ok(piece)
}

pub(super) fn validate(parts: &[Value], original: Option<&str>) -> Result<()> {
    if !parts.iter().any(|part| part.get("text_range").is_some()) {
        return Ok(());
    }
    let text = original.ok_or("text plan has no original text")?;
    let mut offset = 0;
    for part in parts {
        if let Some(value) = part.get("text_range") {
            let range: TextRange = serde_json::from_value(value.clone())?;
            if range.start != offset
                || part.get("text").is_some()
                || part.get("media").is_some()
                || part.get("prepared_part").is_some()
                || part.get("preparation").is_some()
            {
                return Err("text plan overlaps or changes original content".into());
            }
            offset += selected(text, &range)?.len();
        } else if part["preparation"] != true && part.get("prepared_part").is_none() {
            return Err("text plan has an unbound visible part".into());
        }
    }
    if offset != text.len() {
        return Err("text plan omits original content".into());
    }
    Ok(())
}

pub(super) fn project(method: &str, params: &mut Value) -> Result<()> {
    if method != "gateway.plan_send"
        || params.to_string().len() < yuki_plugin_sdk::MAX_FRAME_BYTES - 2048
    {
        return Ok(());
    }
    if super::grants::methods()["methods"]
        .get("gateway.read_text")
        .is_none()
    {
        return Err("selected Host cannot page original gateway text".into());
    }
    let text = params["arguments"]["message"]["text"]
        .as_str()
        .ok_or("oversized gateway message has no text")?;
    let descriptor =
        json!({"bytes":text.len(),"sha256":format!("{:x}",Sha256::digest(text.as_bytes()))});
    params["arguments"]["message"]
        .as_object_mut()
        .ok_or("missing message")?
        .remove("text");
    params["arguments"]["text_source"] = descriptor;
    if params.to_string().len() >= yuki_plugin_sdk::MAX_FRAME_BYTES - 2048 {
        return Err("gateway message metadata exceeds frame limit".into());
    }
    Ok(())
}

pub(super) async fn callback(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    params: &Value,
    annotations: bool,
) -> Result<Value> {
    super::domain::active(db, plugin, generation, "forget.owner").await?;
    let handle = params["handle"]
        .as_str()
        .ok_or("missing original text handle")?;
    let row = sqlx::query(
        "SELECT c.execution,c.method,c.args,o.parent,coalesce(x.original_actor,x.origin) AS actor
        FROM host_calls c JOIN host_installations i ON i.plugin=c.plugin AND i.revision=c.revision
        JOIN operations o ON o.namespace=c.execution AND o.operation_id=c.operation
        JOIN executions x ON x.id=c.execution
        WHERE c.handle=? AND c.plugin=? AND c.generation=? AND c.state='dispatched'",
    )
    .bind(handle)
    .bind(plugin)
    .bind(generation)
    .fetch_one(&mut *db)
    .await?;
    let execution: String = row.try_get("execution")?;
    let actor: String = row.try_get("actor")?;
    effects::authorize(db, &execution, &actor, true).await?;
    let args: Value = serde_json::from_str(row.try_get("args")?)?;
    super::send::check_reply(db, plugin, &args).await?;
    super::gateway_mentions::check(db, plugin, &args).await?;
    super::native_route::check(db, plugin, &execution, &args).await?;
    let original = if annotations {
        if !matches!(
            row.try_get::<&str, _>("method")?,
            "gateway.plan_send" | "gateway.dispatch_part"
        ) {
            return Err("annotations belong to another method".into());
        }
        serde_json::to_string(
            args.get("mentions")
                .ok_or("call has no original mentions")?,
        )?
    } else {
        match row.try_get::<&str, _>("method")? {
            "gateway.plan_send" => args["message"]["text"]
                .as_str()
                .ok_or("plan has no text")?
                .to_owned(),
            "gateway.dispatch_part" => {
                let parent: String = row.try_get("parent")?;
                let planned: String = sqlx::query_scalar(
                    "SELECT c.args FROM host_calls c
                JOIN host_installations i ON i.plugin=c.plugin AND i.revision=c.revision
                JOIN operations o ON o.namespace=c.execution AND o.operation_id=c.operation
                WHERE c.execution=? AND c.plugin=? AND o.parent=? AND o.ordinal=0
                AND c.method='gateway.plan_send' AND c.state='confirmed' AND o.state='confirmed'",
                )
                .bind(&execution)
                .bind(plugin)
                .bind(parent)
                .fetch_one(&mut *db)
                .await?;
                let planned: Value = serde_json::from_str(&planned)?;
                if planned["route"] != args["route"] {
                    return Err("text range changed original route".into());
                }
                let range: TextRange = serde_json::from_value(args["part"]["text_range"].clone())?;
                selected(
                    planned["message"]["text"]
                        .as_str()
                        .ok_or("missing original text")?,
                    &range,
                )?
                .to_owned()
            }
            _ => return Err("text handle belongs to another method".into()),
        }
    };
    let offset = usize::try_from(params["offset"].as_u64().ok_or("invalid text offset")?)?;
    let length = usize::try_from(params["length"].as_u64().ok_or("invalid text page")?)?;
    if offset > original.len() || !(1..=12288).contains(&length) {
        return Err("text page outside original selection".into());
    }
    super::gateway_mentions::hold(db, plugin, handle, &execution, &args).await?;
    let dependencies: Vec<i64> = sqlx::query_scalar("SELECT source FROM executions WHERE id=?1 AND source IS NOT NULL UNION SELECT event FROM main_dependencies WHERE execution=?1")
        .bind(&execution).fetch_all(&mut *db).await?;
    for event in dependencies {
        source::acquire(db, plugin, handle, event).await?;
    }
    let end = offset.saturating_add(length).min(original.len());
    Ok(
        json!({"offset":offset,"total":original.len(),"sha256":format!("{:x}",Sha256::digest(original.as_bytes())),
        "bytes":STANDARD.encode(&original.as_bytes()[offset..end]),"done":end==original.len()}),
    )
}
