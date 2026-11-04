use crate::Result;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

/// Bound by the authenticated gateway. Caller never supplies an actor or target in the command body.
pub struct Source {
    pub namespace: String,
    pub group: String,
    pub bot: String,
    pub stage: String,
    pub sender: String,
    pub native: String,
    pub occurred: i64,
    pub session: i64,
}
pub struct Recovery {
    pub source: Source,
    pub actor_stage: String,
    pub person: String,
    pub online_stages: Vec<String>,
    pub eligible_stages: Vec<String>,
    pub before: Option<(String, i64, bool)>,
}

pub async fn route(
    conn: &mut SqliteConnection,
    namespace: &str,
    group: &str,
) -> Result<Option<(String, i64, bool)>> {
    Ok(sqlx::query_as(
        "SELECT stage,revision,enabled FROM recovered_groups WHERE namespace=? AND native_group=?",
    )
    .bind(namespace)
    .bind(group)
    .fetch_optional(conn)
    .await?)
}

pub async fn principal(
    conn: &mut SqliteConnection,
    s: &Source,
) -> Result<Option<(String, String)>> {
    Ok(sqlx::query_as("SELECT actor.id,p.id FROM assignments actor JOIN accounts h ON actor.account_id=h.id
      JOIN persons p ON p.id=actor.original_person JOIN persons root ON root.id=coalesce(p.current_root,p.id)
      JOIN assignments bot ON bot.id=? JOIN accounts self ON self.id=bot.account_id WHERE h.native=? AND h.namespace=? AND h.platform='qq'
      AND self.native=? AND self.platform=h.platform AND self.namespace=h.namespace AND bot.kind='self' AND bot.status='active'
      AND actor.kind='person' AND actor.status='active' AND p.enabled AND root.enabled AND root.admin
      AND coalesce(actor.started_at,actor.first_observed)<=? AND coalesce(bot.started_at,bot.first_observed)<=?
      AND (SELECT status FROM assignment_states WHERE stage=actor.id AND at<=? ORDER BY at DESC LIMIT 1)='active'
      AND (SELECT status FROM assignment_states WHERE stage=bot.id AND at<=? ORDER BY at DESC LIMIT 1)='active'")
        .bind(&s.stage).bind(&s.sender).bind(&s.namespace).bind(&s.bot).bind(s.occurred).bind(s.occurred).bind(s.occurred).bind(s.occurred).fetch_optional(conn).await?)
}

pub async fn replay(
    conn: &mut SqliteConnection,
    s: &Source,
    actor: &str,
    person: &str,
) -> Result<bool> {
    let prior=sqlx::query("SELECT actor_stage,person FROM group_recovery_receipts WHERE namespace=? AND native_group=? AND stage=? AND native_message=?")
        .bind(&s.namespace).bind(&s.group).bind(&s.stage).bind(&s.native).fetch_optional(conn).await?;
    if let Some(row) = prior {
        if row.try_get::<String, _>("actor_stage")? != actor
            || row.try_get::<String, _>("person")? != person
        {
            return Err("source control identity conflict".into());
        }
        return Ok(true);
    }
    Ok(false)
}

pub async fn apply(conn: &mut SqliteConnection, r: &Recovery) -> Result<Value> {
    let s = &r.source;
    if principal(conn, s).await?.as_ref() != Some(&(r.actor_stage.clone(), r.person.clone())) {
        return Ok(json!("denied"));
    }
    if replay(conn, s, &r.actor_stage, &r.person).await? {
        return Ok(json!("enabled"));
    }
    for stage in &r.online_stages {
        let enabled:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM assignments WHERE id=? AND kind='self' AND status='active')")
            .bind(stage).fetch_one(&mut *conn).await?;
        if enabled != r.eligible_stages.contains(stage) {
            return Ok(json!("conflict"));
        }
    }
    if route(conn, &s.namespace, &s.group).await? != r.before {
        return Ok(json!("conflict"));
    }
    sqlx::query("INSERT INTO recovered_groups(namespace,native_group,stage,enabled,revision) VALUES(?,?,?,1,1)
      ON CONFLICT(namespace,native_group) DO UPDATE SET stage=excluded.stage,enabled=1,revision=recovered_groups.revision+1
      WHERE recovered_groups.stage!=excluded.stage OR NOT recovered_groups.enabled")
      .bind(&s.namespace).bind(&s.group).bind(&s.stage).execute(&mut *conn).await?;
    let after = route(conn, &s.namespace, &s.group).await?;
    sqlx::query("INSERT INTO group_recovery_receipts(namespace,native_group,native_message,stage,actor_stage,person,connection,before_state,after_state)
      VALUES(?,?,?,?,?,?,?,?,?)").bind(&s.namespace).bind(&s.group).bind(&s.native).bind(&s.stage).bind(&r.actor_stage)
      .bind(&r.person).bind(s.session).bind(serde_json::to_string(&r.before)?).bind(serde_json::to_string(&after)?).execute(conn).await?;
    Ok(json!("enabled"))
}
