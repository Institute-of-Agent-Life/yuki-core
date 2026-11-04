use super::clock::Trigger;
use crate::{
    Result,
    identity::new_id,
    runtime::{execution, main::Config},
};
use serde_json::json;
use sqlx::{Row, SqliteConnection, SqlitePool};

pub async fn ready(pool: &SqlitePool, now: i64) -> Result<Vec<i64>> {
    Ok(sqlx::query_scalar("SELECT id FROM (
      SELECT s.id, s.next_due AS due FROM schedules s WHERE s.status='active' AND s.next_due<=? AND NOT EXISTS(SELECT 1 FROM schedule_occurrences o WHERE o.schedule=s.id AND o.status='running')
      UNION ALL SELECT o.schedule,o.planned_utc FROM schedule_occurrences o JOIN executions x ON x.id=o.execution LEFT JOIN work_nodes w ON w.id=x.work_id
      WHERE o.status='running' AND ((x.work_id IS NOT NULL AND o.work IS NULL) OR
        (x.work_id IS NULL AND (x.state IN('completed','held') OR x.cancelled)) OR w.status IN('completed','failed','cancelled')))
      ORDER BY due,id LIMIT 32").bind(now).fetch_all(pool).await?)
}

async fn settle(conn: &mut SqliteConnection, id: i64, now: i64) -> Result<bool> {
    let Some(o)=sqlx::query("SELECT o.*,x.state,x.cancelled,x.work_id,w.status AS work_state,t.diagnostic AS turn_diagnostic
     FROM schedule_occurrences o JOIN executions x ON x.id=o.execution JOIN main_turns t ON t.execution=x.id LEFT JOIN work_nodes w ON w.id=x.work_id
     WHERE o.schedule=? AND o.status='running'").bind(id).fetch_optional(&mut *conn).await? else{return Ok(false)};
    let work: Option<String> = o.try_get("work_id")?;
    if work.is_some() && o.try_get::<Option<String>, _>("work")?.is_none() {
        sqlx::query("UPDATE schedule_occurrences SET work=? WHERE id=?")
            .bind(&work)
            .bind(o.get::<i64, _>("id"))
            .execute(&mut *conn)
            .await?;
    }
    let state = if let Some(w) = o.try_get::<Option<String>, _>("work_state")? {
        match w.as_str() {
            "completed" => "succeeded",
            "failed" => "failed",
            "cancelled" => "cancelled",
            _ => return Ok(true),
        }
    } else if o.try_get::<bool, _>("cancelled")? {
        "cancelled"
    } else {
        match o.get::<String, _>("state").as_str() {
            "completed" => "succeeded",
            "held" => "blocked",
            _ => return Ok(true),
        }
    };
    let planned: i64 = o.try_get("planned_utc")?;
    sqlx::query("UPDATE schedule_occurrences SET status=?,diagnostic=? WHERE id=?")
        .bind(state)
        .bind(o.try_get::<Option<String>, _>("turn_diagnostic")?)
        .bind(o.get::<i64, _>("id"))
        .execute(&mut *conn)
        .await?;
    let s=sqlx::query("SELECT s.*,v.trigger_json,v.zone FROM schedules s JOIN schedule_versions v ON v.schedule=s.id AND v.version=s.version WHERE s.id=?")
        .bind(id).fetch_one(&mut *conn).await?;
    let status: String = s.try_get("status")?;
    if matches!(status.as_str(), "cancelled" | "completed") {
        return Ok(true);
    }
    let capped = s
        .try_get::<Option<i64>, _>("max_runs")?
        .is_some_and(|cap| s.get::<i64, _>("run_count") >= cap);
    if capped && state != "blocked" {
        sqlx::query("UPDATE schedules SET status='completed',next_due=NULL WHERE id=?")
            .bind(id)
            .execute(conn)
            .await?;
    } else if s.try_get::<Option<i64>, _>("next_due")? == Some(planned) {
        let rule: Trigger = serde_json::from_str(s.get("trigger_json"))?;
        let due = rule.after_completion(planned, now, s.get("zone"))?;
        let next_status = if status == "paused" {
            "paused"
        } else if state == "blocked" {
            "blocked"
        } else if due.is_none() {
            if state == "succeeded" {
                "completed"
            } else {
                "failed"
            }
        } else {
            "active"
        };
        sqlx::query("UPDATE schedules SET next_due=?,status=? WHERE id=?")
            .bind(due)
            .bind(next_status)
            .bind(id)
            .execute(conn)
            .await?;
    }
    Ok(true)
}

/// All writes below belong to the same owner transaction. The original schedule owns
/// a fresh root budget. Only an explicit later Work acceptance binds this occurrence.
pub async fn reconcile(conn: &mut SqliteConnection, cfg: &Config, id: i64, now: i64) -> Result<()> {
    if settle(conn, id, now).await? {
        return Ok(());
    }
    let Some(s)=sqlx::query("SELECT s.*,v.goal FROM schedules s JOIN schedule_versions v ON v.schedule=s.id AND v.version=s.version WHERE s.id=? AND s.status='active' AND s.next_due<=? AND v.goal IS NOT NULL AND (s.max_runs IS NULL OR s.run_count<s.max_runs)")
        .bind(id).bind(now).fetch_optional(&mut *conn).await? else{return Ok(())};
    let planned: i64 = s.try_get("next_due")?;
    let version: i64 = s.try_get("version")?;
    let entry: String = s.try_get("entry")?;
    let owner: String = s.try_get("owner")?;
    let creator: String = s.try_get("creator_execution")?;
    let origin = format!("schedule-occurrence:{id}:{planned}");
    let exec = new_id(conn).await?;
    sqlx::query("INSERT INTO main_origins(ref,kind,conversation,parent) VALUES(?,'schedule',?,?)")
        .bind(&origin)
        .bind(&entry)
        .bind(&creator)
        .execute(&mut *conn)
        .await?;
    let occurrence:i64=sqlx::query_scalar("INSERT INTO schedule_occurrences(schedule,version,planned_utc,execution,owner,entry,status,profile) VALUES(?,?,?,?,?,?,'running',?) RETURNING id")
        .bind(id).bind(version).bind(planned).bind(&exec).bind(&owner).bind(&entry).bind(serde_json::to_string(cfg)?).fetch_one(&mut *conn).await?;
    sqlx::query(
        "INSERT INTO executions(id,state,origin,source_occurrence) VALUES(?,'pending',?,?)",
    )
    .bind(&exec)
    .bind(&origin)
    .bind(occurrence)
    .execute(&mut *conn)
    .await?;
    let window = crate::runtime::main::window(conn, &entry).await?;
    let input = json!({"text":s.get::<String,_>("goal"),"context":[{"schedule_id":id,"occurrence":occurrence,"planned_utc_ms":planned,"definition_version":version,"owner":owner}]});
    sqlx::query("INSERT INTO main_turns(execution,conversation,window,origin,occurrence,material,ready_after) VALUES(?,?,?,?,?,?,0)")
        .bind(&exec).bind(&entry).bind(window).bind(&origin).bind(planned.to_string()).bind(input.to_string()).execute(&mut *conn).await?;
    sqlx::query("INSERT INTO main_dependencies SELECT ?,event FROM schedule_sources WHERE schedule=? AND version=?").bind(&exec).bind(id).bind(version).execute(&mut *conn).await?;
    if execution::eligible(conn, &exec).await.is_err() {
        sqlx::query("UPDATE executions SET state='held' WHERE id=?")
            .bind(&exec)
            .execute(&mut *conn)
            .await?;
        sqlx::query("UPDATE main_turns SET diagnostic='original_schedule_authority_denied' WHERE execution=?").bind(&exec).execute(&mut *conn).await?;
    } else {
        execution::root(conn, &exec, cfg.root_slots, cfg.max_input_units).await?;
    }
    Ok(())
}

/// A scheduled SELF can use its own registered group receiver even without a human
/// event in the execution ancestry. Its original entry, receiver and window must agree.
pub async fn self_group_route(
    conn: &mut SqliteConnection,
    exec: &str,
) -> Result<Option<crate::social::send::Route>> {
    let row=sqlx::query("WITH RECURSIVE family(id) AS(SELECT ?1 UNION SELECT o.parent FROM family f JOIN executions x ON x.id=f.id JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL)
        SELECT s.owner,s.entry,s.self_receiver,e.space,g.stage,g.revision,t.window,m.generation,
          (g.enabled AND stage.status='active' AND stage.kind='self') AS live
        FROM family f JOIN executions x ON x.id=f.id JOIN schedule_occurrences so ON so.id=x.source_occurrence
        JOIN schedules s ON s.id=so.schedule JOIN main_origins own ON own.ref=s.owner AND own.kind='self'
        JOIN entries e ON e.id=s.entry AND e.scope='group' JOIN main_turns t ON t.execution=?1 JOIN main_windows m ON m.conversation=e.id
        LEFT JOIN recovered_groups g ON g.namespace=json_extract(e.space,'$[1]') AND g.native_group=json_extract(e.space,'$[2]')
        LEFT JOIN assignments stage ON stage.id=g.stage
        WHERE json_extract(e.space,'$[0]')='qq' LIMIT 1").bind(exec).fetch_optional(conn).await?;
    let Some(r) = row else { return Ok(None) };
    let pinned: Option<String> = r.try_get("self_receiver")?;
    if pinned.is_none()
        || pinned != r.try_get::<Option<String>, _>("stage")?
        || r.try_get::<Option<bool>, _>("live")? != Some(true)
        || r.get::<i64, _>("window") != r.get::<i64, _>("generation")
    {
        return Err("registered SELF group receiver unavailable".into());
    }
    let space: Vec<String> = serde_json::from_str(r.get("space"))?;
    Ok(Some(crate::social::send::Route {
        entry: r.get("entry"),
        person: r.get("owner"),
        stage: pinned.ok_or("receiver unavailable")?,
        target: space.get(2).ok_or("missing group")?.parse()?,
        private_stage: None,
        group: true,
        window: r.get("window"),
        revision: r.try_get("revision")?,
    }))
}
