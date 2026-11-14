use super::{Change, Policy};
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
        AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)")
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
pub(super) async fn list(db: &mut SqliteConnection, c: &Change, policy: &Policy) -> Result<Value> {
    let target = c.params["entry"].as_str().ok_or("entry required")?;
    entry(db, policy, target).await?;
    let after = c.params["after"].as_i64().unwrap_or(0);
    let ids:Vec<i64>=sqlx::query_scalar("SELECT e.id FROM events e JOIN persons p ON p.id=e.raw_actor JOIN persons r ON r.id=coalesce(p.current_root,p.id)
      JOIN assignments a ON a.id=e.actor_stage AND a.original_person=e.raw_actor JOIN assignments b ON b.id=e.bot_stage
      WHERE e.entry_id=? AND e.id>? AND e.envelope IS NOT NULL AND p.enabled AND r.enabled AND a.status='active' AND b.status='active'
      AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id) ORDER BY e.id LIMIT 8")
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
    let original = original(db, &c.plugin, policy, id).await?;
    let frame: Value = serde_json::from_str(&original.envelope)?;
    let raw = serde_json::to_vec(&frame)?;
    let digest = format!("{:x}", Sha256::digest(&raw));
    if c.params["version"].as_str().is_some_and(|v| v != digest) {
        return Err("original source version changed".into());
    }
    let start = c.params["cursor"].as_u64().unwrap_or(0) as usize;
    if start > raw.len() {
        return Err("invalid source cursor".into());
    }
    let end = raw.len().min(start + 8192);
    let mut page = json!({"event":id,"version":digest,"data":STANDARD.encode(&raw[start..end]),"next":end,"size":raw.len(),"complete":end==raw.len()});
    if c.params["include_origin"] == true {
        page["origin"] = json!({
            "entry":original.entry_id,"original_person":original.raw_actor,
            "actor_assignment":original.actor_stage,"current_person":original.current_root,
            "observed_generation":original.observed_generation,
            "current_generation":original.current_generation
        });
    }
    if c.params["include_reply"] == true {
        page["reply_to"] = Value::Null;
        if frame["kind"] == "message"
            && let Some(generation) = original.observed_generation
            && let Some(reply) = group_human_reply(db, id).await?
            && reply.observed_generation == Some(generation)
        {
            let held =
                source::acquire(db, &c.plugin, &format!("participation:{id}"), reply.event).await?;
            let raw = serde_json::to_vec(&held["envelope"])?;
            page["reply_to"] = json!({
                "event": reply.event,
                "version": format!("{:x}", Sha256::digest(&raw))
            });
        }
    }
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
