use super::{Change, Policy};
use crate::{Result, ingress::admit, privacy::source};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};

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
    let row = sqlx::query("SELECT e.entry_id,e.envelope FROM events e JOIN persons p ON p.id=e.raw_actor JOIN persons r ON r.id=coalesce(p.current_root,p.id)
        JOIN assignments a ON a.id=e.actor_stage JOIN assignments b ON b.id=e.bot_stage
        WHERE e.id=? AND p.enabled AND r.enabled AND a.status='active' AND b.status='active'
        AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)")
        .bind(event).fetch_one(&mut *db).await?;
    entry(db, policy, row.get("entry_id")).await?;
    source::acquire(db, plugin, &format!("participation:{event}"), event).await?;
    Ok(serde_json::from_str(row.get("envelope"))?)
}
pub(super) async fn list(db: &mut SqliteConnection, c: &Change, policy: &Policy) -> Result<Value> {
    let target = c.params["entry"].as_str().ok_or("entry required")?;
    entry(db, policy, target).await?;
    let after = c.params["after"].as_i64().unwrap_or(0);
    let ids:Vec<i64>=sqlx::query_scalar("SELECT e.id FROM events e JOIN persons p ON p.id=e.raw_actor JOIN persons r ON r.id=coalesce(p.current_root,p.id)
      JOIN assignments a ON a.id=e.actor_stage JOIN assignments b ON b.id=e.bot_stage
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
    let frame = checked(db, &c.plugin, policy, id).await?;
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
    Ok(
        json!({"event":id,"version":digest,"data":STANDARD.encode(&raw[start..end]),"next":end,"size":raw.len(),"complete":end==raw.len()}),
    )
}
pub(super) async fn promote(
    db: &mut SqliteConnection,
    c: &Change,
    policy: &Policy,
) -> Result<Value> {
    let event = c.params["event"].as_i64().ok_or("event required")?;
    checked(db, &c.plugin, policy, event).await?;
    let result = admit::promote(db, event).await?;
    sqlx::query("INSERT OR IGNORE INTO participation_promotions VALUES(?,?)")
        .bind(&c.plugin)
        .bind(event)
        .execute(db)
        .await?;
    Ok(result)
}
