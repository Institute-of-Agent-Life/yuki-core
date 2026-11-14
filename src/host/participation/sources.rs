use super::{Change, Policy, mentions, self_source};
use crate::{
    Result,
    ingress::{admit, reply::group_human_reply},
    privacy::source,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};

struct OriginalSource {
    entry_id: String,
    envelope: String,
    raw_actor: String,
    actor_stage: String,
    current_root: String,
    observed_generation: Option<i64>,
    current_generation: i64,
}

pub(super) async fn entry(db: &mut SqliteConnection, policy: &Policy, entry: &str) -> Result<()> {
    if !policy.entries.contains(entry) {
        return Err("entry outside participation scope".into());
    }
    let space: String = sqlx::query_scalar(
        "SELECT space FROM entries WHERE id=? AND scope='group' AND closed_at IS NULL",
    )
    .bind(entry)
    .fetch_one(&mut *db)
    .await?;
    let space: Vec<String> = serde_json::from_str(&space)?;
    if space.len() != 3 {
        return Err("missing original group route".into());
    }
    if space[0] == "qq"
        && super::qq_route(db, &space[1], &space[2])
            .await?
            .is_none_or(|(_, _, enabled)| !enabled)
    {
        return Err("group paused".into());
    }
    Ok(())
}
pub(super) async fn checked(
    db: &mut SqliteConnection,
    plugin: &str,
    policy: &Policy,
    event: i64,
) -> Result<Value> {
    let source = original(db, plugin, policy, event).await?;
    Ok(serde_json::from_str(&source.envelope)?)
}

async fn original(
    db: &mut SqliteConnection,
    plugin: &str,
    policy: &Policy,
    event: i64,
) -> Result<OriginalSource> {
    let row = sqlx::query("SELECT e.entry_id,e.envelope,e.raw_actor,e.actor_stage,
        r.id AS current_root,e.observed_generation,coalesce(w.generation,1) AS current_generation
        FROM events e JOIN persons p ON p.id=e.raw_actor JOIN persons r ON r.id=coalesce(p.current_root,p.id)
        JOIN assignments a ON a.id=e.actor_stage AND a.original_person=e.raw_actor
        JOIN assignments b ON b.id=e.bot_stage LEFT JOIN main_windows w ON w.conversation=e.entry_id
        WHERE e.id=? AND p.enabled AND r.enabled AND a.status='active' AND b.status='active'
        AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)
        AND NOT EXISTS(SELECT 1 FROM participation_erased_mentions WHERE event=e.id)")
        .bind(event).fetch_one(&mut *db).await?;
    let entry_id = row.try_get::<String, _>("entry_id")?;
    entry(db, policy, &entry_id).await?;
    source::acquire(db, plugin, &format!("participation:{event}"), event).await?;
    Ok(OriginalSource {
        entry_id,
        envelope: row.try_get("envelope")?,
        raw_actor: row.try_get("raw_actor")?,
        actor_stage: row.try_get("actor_stage")?,
        current_root: row.try_get("current_root")?,
        observed_generation: row.try_get("observed_generation")?,
        current_generation: row.try_get("current_generation")?,
    })
}

pub(in crate::host) async fn authorize_review(
    db: &mut SqliteConnection,
    plugin: &str,
    event: i64,
) -> Result<()> {
    let (_, policy) = super::policy(db, plugin).await?;
    let original = original(db, plugin, &policy, event).await?;
    let envelope: Value = serde_json::from_str(&original.envelope)?;
    if original.observed_generation != Some(original.current_generation)
        || envelope["kind"] != "message"
    {
        return Err(
            "Jev requires a human observation in the current participation generation".into(),
        );
    }
    Ok(())
}

pub(super) async fn list(db: &mut SqliteConnection, c: &Change, policy: &Policy) -> Result<Value> {
    let target = c.params["entry"].as_str().ok_or("entry required")?;
    entry(db, policy, target).await?;
    let after = c.params["after"].as_i64().unwrap_or(0);
    let ids:Vec<i64>=sqlx::query_scalar("SELECT e.id FROM events e JOIN persons p ON p.id=e.raw_actor JOIN persons r ON r.id=coalesce(p.current_root,p.id)
      JOIN assignments a ON a.id=e.actor_stage AND a.original_person=e.raw_actor JOIN assignments b ON b.id=e.bot_stage
      WHERE e.entry_id=? AND e.id>? AND e.envelope IS NOT NULL AND p.enabled AND r.enabled AND a.status='active' AND b.status='active'
      AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)
      AND NOT EXISTS(SELECT 1 FROM participation_erased_mentions WHERE event=e.id) ORDER BY e.id LIMIT 8")
      .bind(target).bind(after).fetch_all(&mut *db).await?;
    let mut items = vec![];
    for id in &ids {
        let frame = checked(db, &c.plugin, policy, *id).await?;
        let raw = serde_json::to_vec(&frame)?;
        let preview = frame["segments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s["text"].as_str())
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(512)
            .collect::<String>();
        items.push(
            json!({"event":id,"preview":preview,"version":format!("{:x}",Sha256::digest(&raw))}),
        );
    }
    Ok(json!({"entry":target,"items":items,"next":ids.last().copied().unwrap_or(after)}))
}
pub(super) async fn read(db: &mut SqliteConnection, c: &Change, policy: &Policy) -> Result<Value> {
    let id = c.params["event"].as_i64().ok_or("event required")?;
    if c.params["include_self"] == true
        && sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM personal_deliveries WHERE event=?)",
        )
        .bind(id)
        .fetch_one(&mut *db)
        .await?
    {
        return self_source::read(db, c, policy, id).await;
    }
    let original = original(db, &c.plugin, policy, id).await?;
    let frame: Value = serde_json::from_str(&original.envelope)?;
    let raw = serde_json::to_vec(&frame)?;
    let mut page = page(c, id, &raw)?;
    if c.params["include_origin"] == true {
        page["origin"] = json!({
            "entry":original.entry_id,"original_person":original.raw_actor,
            "actor_assignment":original.actor_stage,"current_person":original.current_root,
            "observed_generation":original.observed_generation,
            "current_generation":original.current_generation
        });
    }
    if let Some(cursor) = c.params["mention_cursor"].as_i64() {
        page["mentions"] = mentions::page(db, id, cursor).await?;
    }
    if c.params["include_reply"] == true {
        page["reply_to"] = Value::Null;
        if frame["kind"] == "message"
            && let Some(generation) = original.observed_generation
        {
            let delivery = if c.params["include_self"] == true {
                self_source::anchor(db, &c.plugin, policy, id, generation).await?
            } else {
                None
            };
            if let Some(reference) = delivery {
                page["reply_to"] = reference;
            } else if let Some(reply) = group_human_reply(db, id).await?
                && reply.observed_generation == Some(generation)
                && !sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(SELECT 1 FROM participation_erased_mentions WHERE event=?)",
                )
                .bind(reply.event)
                .fetch_one(&mut *db)
                .await?
            {
                let held =
                    source::acquire(db, &c.plugin, &format!("participation:{id}"), reply.event)
                        .await?;
                page["reply_to"] = reference(reply.event, &serde_json::to_vec(&held["envelope"])?);
            }
        }
    }
    Ok(page)
}

pub(super) fn reference(event: i64, raw: &[u8]) -> Value {
    json!({"event":event,"version":format!("{:x}",Sha256::digest(raw))})
}

pub(super) fn page(c: &Change, event: i64, raw: &[u8]) -> Result<Value> {
    let mut page = reference(event, raw);
    if c.params["version"]
        .as_str()
        .is_some_and(|v| page["version"] != v)
    {
        return Err("original source version changed".into());
    }
    let start = usize::try_from(c.params["cursor"].as_u64().unwrap_or(0))?;
    if start > raw.len() {
        return Err("invalid source cursor".into());
    }
    let end = raw.len().min(start.saturating_add(8192));
    page["data"] = json!(STANDARD.encode(&raw[start..end]));
    page["next"] = json!(end);
    page["size"] = json!(raw.len());
    page["complete"] = json!(end == raw.len());
    Ok(page)
}
pub(super) async fn promote(
    db: &mut SqliteConnection,
    c: &Change,
    policy: &Policy,
) -> Result<Value> {
    let event = c.params["event"].as_i64().ok_or("event required")?;
    let source = original(db, &c.plugin, policy, event).await?;
    if let Some(expected) = c.params["expected_generation"].as_i64()
        && (source.observed_generation != Some(expected) || source.current_generation != expected)
    {
        return Err("participation observation is outside the expected generation".into());
    }
    let result = admit::promote(db, event).await?;
    sqlx::query("INSERT OR IGNORE INTO participation_promotions VALUES(?,?)")
        .bind(&c.plugin)
        .bind(event)
        .execute(db)
        .await?;
    Ok(result)
}
