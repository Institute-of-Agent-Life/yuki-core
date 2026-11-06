use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    pub model_requests: i64,
    pub resume_delay_ms: i64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            model_requests: 24,
            resume_delay_ms: 2000,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.model_requests <= 0 || !(1..=60000).contains(&self.resume_delay_ms) {
            return Err("invalid Work segment scheduling limits".into());
        }
        Ok(())
    }
}

async fn request_count(db: &mut SqliteConnection, exec: &str) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "WITH RECURSIVE owned(execution) AS (
            SELECT ?1 UNION SELECT x.id FROM owned parent
            JOIN main_origins o ON o.parent=parent.execution JOIN executions x ON x.origin=o.ref
            WHERE o.kind='embedding' OR o.ref GLOB 'native-search:*'
                OR x.id IN(SELECT execution FROM host_aux_origins)
         ) SELECT count(*) FROM owned JOIN activations a ON a.execution=owned.execution
           JOIN model_requests r ON r.activation=a.id",
    )
    .bind(exec)
    .fetch_one(db)
    .await?)
}

pub(super) async fn begin(db: &mut SqliteConnection, exec: &str, policy: &Config) -> Result<()> {
    let last = sqlx::query(
        "SELECT ordinal,state FROM main_segments WHERE execution=? ORDER BY ordinal DESC LIMIT 1",
    )
    .bind(exec)
    .fetch_optional(&mut *db)
    .await?;
    if last
        .as_ref()
        .is_some_and(|r| r.get::<&str, _>("state") == "active")
    {
        return Ok(());
    }
    let ordinal = last.map_or(0, |r| r.get::<i64, _>("ordinal") + 1);
    let initial_requests = request_count(db, exec).await?;
    sqlx::query(
        "INSERT INTO main_segments(execution,ordinal,first_request_count,request_limit,state,started_at)
         VALUES(?1,?2,?5,?3,'active',?4)",
    )
    .bind(exec)
    .bind(ordinal)
    .bind(policy.model_requests)
    .bind(super::now()?)
    .bind(initial_requests)
    .execute(&mut *db)
    .await?;
    sqlx::query(
        "UPDATE work_nodes SET status='active' WHERE status='queued'
         AND id IN(SELECT work FROM work_runs WHERE execution=?)",
    )
    .bind(exec)
    .execute(&mut *db)
    .await?;
    sqlx::query(
        "UPDATE main_turns SET diagnostic=NULL WHERE execution=? AND diagnostic='segment_budget'",
    )
    .bind(exec)
    .execute(db)
    .await?;
    Ok(())
}

/// Called only after the original response, ordered effects, and terminal proposals settle.
pub(super) async fn exhausted(
    db: &mut SqliteConnection,
    exec: &str,
    policy: &Config,
) -> Result<Option<Value>> {
    let segment = sqlx::query(
        "SELECT ordinal,request_limit,first_request_count FROM main_segments
         WHERE execution=? AND state='active'",
    )
    .bind(exec)
    .fetch_one(&mut *db)
    .await?;
    let count = request_count(db, exec).await? - segment.get::<i64, _>("first_request_count");
    if count < segment.get::<i64, _>("request_limit") {
        return Ok(None);
    }
    let Some(work) = crate::work::bound(db, exec).await? else {
        return super::step::hold(db, exec, "agent_segment_budget_exhausted")
            .await
            .map(Some);
    };
    let now = super::now()?;
    sqlx::query("UPDATE main_segments SET state='yielded',request_count=?,yielded_at=? WHERE execution=? AND ordinal=?")
        .bind(count).bind(now).bind(exec).bind(segment.get::<i64,_>("ordinal")).execute(&mut *db).await?;
    sqlx::query("UPDATE executions SET state='pending' WHERE id=? AND NOT cancelled")
        .bind(exec)
        .execute(&mut *db)
        .await?;
    sqlx::query("UPDATE work_nodes SET status='queued',updated=? WHERE id=? AND status='active'")
        .bind(now)
        .bind(&work)
        .execute(&mut *db)
        .await?;
    sqlx::query(
        "UPDATE main_turns SET ready_after=?,diagnostic='segment_budget' WHERE execution=?",
    )
    .bind(now + policy.resume_delay_ms)
    .bind(exec)
    .execute(db)
    .await?;
    Ok(Some(json!({"segment_yielded":true,"work_id":work})))
}
