//! A dispute preserves the source. Only a distinct alternative owns a contradicts edge.
use super::{
    evidence,
    facts::{self, Subject},
    metadata,
    scope::{self, Reader},
    tools,
};
use crate::Result;
use serde_json::{Value, json};
use sqlx::SqliteConnection;

pub(super) async fn apply(
    db: &mut SqliteConnection,
    r: &Reader,
    exec: &str,
    op: &str,
    req: &Value,
    id: &str,
    mut patch: metadata::Patch,
) -> Result<Value> {
    let original = tools::paid_active_person(db, r, exec, id).await?;
    if original["readable_evidence"].as_u64().unwrap_or_default() == 0 {
        return Err("original evidence unavailable".into());
    }
    let event = tools::original_event(db, exec, req).await?;
    let proof = evidence::resolve(db, r, &r.actor, &event).await?;
    let rev = original["revision"]
        .as_i64()
        .ok_or("original revision unavailable")?
        .checked_add(1)
        .ok_or("revision overflow")?;
    let subject = Subject {
        kind: "person".into(),
        person: original["person"].as_str().map(str::to_owned),
        space: None,
        visibility: None,
        entry: None,
    };
    let body = original["body"]
        .as_str()
        .ok_or("original content unavailable")?;
    sqlx::query("UPDATE memory_facts SET revision=? WHERE id=? AND revision=? AND status='active' RETURNING id").bind(rev).bind(id).bind(rev-1).fetch_one(&mut *db).await?;
    sqlx::query(
        "INSERT INTO memory_versions(fact,revision,body,confidence,execution) VALUES(?,?,?,?,?)",
    )
    .bind(id)
    .bind(rev)
    .bind(body)
    .bind(original["confidence"].as_f64())
    .bind(exec)
    .execute(&mut *db)
    .await?;
    sqlx::query("INSERT OR IGNORE INTO memory_conflict_flags VALUES(?,'original')")
        .bind(id)
        .execute(&mut *db)
        .await?;
    let alt = if let Some(text) = req["new_content"].as_str() {
        let text = metadata::normalize(text);
        if text.is_empty() {
            return Err("memory_content_key_and_category_required".into());
        }
        let key = patch
            .memory_key
            .as_deref()
            .or(original["fact_key"].as_str())
            .ok_or("memory_key required")?;
        let m = &original["metadata"];
        patch.category = patch
            .category
            .or_else(|| m["category"].as_str().map(str::to_owned));
        if patch.category.is_none() {
            return Err("memory_content_key_and_category_required".into());
        }
        patch.kind = patch.kind.or_else(|| m["kind"].as_str().map(str::to_owned));
        patch.importance = patch.importance.or(m["importance"].as_i64());
        let alt = facts::insert_fact(db, exec, &subject, "subject", key).await?;
        sqlx::query("UPDATE memory_facts SET status='retracted' WHERE id=?")
            .bind(&alt)
            .execute(&mut *db)
            .await?;
        sqlx::query("INSERT INTO memory_conflict_flags VALUES(?,'alternative')")
            .bind(&alt)
            .execute(&mut *db)
            .await?;
        sqlx::query("INSERT INTO memory_versions(fact,revision,body,confidence,execution) VALUES(?,1,?,?,?)").bind(&alt).bind(text).bind(req["confidence"].as_f64().unwrap_or(0.9)).bind(exec).execute(&mut *db).await?;
        metadata::apply(db, &alt, &patch).await?;
        metadata::temporal(db, &alt, exec, &patch, true).await?;
        evidence::attach(db, &alt, proof).await?;
        tools::bind_sources(db, exec, &alt).await?;
        Some(alt)
    } else {
        evidence::attach(db, id, proof).await?;
        None
    };
    sqlx::query("INSERT INTO memory_conflict_actions(execution,operation,original,alternative,original_revision) VALUES(?,?,?,?,?)").bind(exec).bind(op).bind(id).bind(&alt).bind(rev).execute(&mut *db).await?;
    let recorded = alt.as_deref().unwrap_or(id);
    let number: i64 = sqlx::query_scalar("SELECT number FROM memory_facts WHERE id=?")
        .bind(recorded)
        .fetch_one(&mut *db)
        .await?;
    let recorded_revision = if alt.is_some() { 1 } else { rev };
    sqlx::query("INSERT INTO memory_operations VALUES(?,?,?,?,?,'committed_as_contested')")
        .bind(exec)
        .bind(op)
        .bind(scope::hash(req))
        .bind(recorded)
        .bind(recorded_revision)
        .execute(&mut *db)
        .await?;
    tools::bind_sources(db, exec, id).await?;
    Ok(
        json!({"fact":recorded,"fact_id":number,"revision":recorded_revision,"old_fact_id":original["fact_id"],"new_fact_id":number,"outcome":"committed_as_contested","reason_code":"agent_requested_contest","applied_operation":"contest"}),
    )
}
