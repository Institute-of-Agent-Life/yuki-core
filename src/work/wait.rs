use super::{
    mailbox,
    tree::{self, Scope},
};
use crate::{Result, identity::new_id};
use chrono::DateTime;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub fn clock() -> Result<i64> {
    super::tree::tick()
}
fn instant(v: &Value) -> Result<i64> {
    Ok(
        DateTime::parse_from_rfc3339(v.as_str().ok_or("timezone-qualified instant required")?)?
            .timestamp_millis(),
    )
}

async fn terms(conn: &mut SqliteConnection, id: &str) -> Result<Vec<Value>> {
    let rows=sqlx::query("SELECT ordinal,kind,target,detail,due,hit FROM work_wait_terms WHERE wait=? ORDER BY ordinal").bind(id).fetch_all(conn).await?;
    rows.iter().map(|r|Ok(json!({"ordinal":r.try_get::<i64,_>("ordinal")?,"kind":r.try_get::<String,_>("kind")?,"target":r.try_get::<Option<String>,_>("target")?,"detail":r.try_get::<Option<String>,_>("detail")?,"due_ms":r.try_get::<Option<i64>,_>("due")?,"hit":r.try_get::<Option<String>,_>("hit")?}))).collect()
}
pub async fn status(conn: &mut SqliteConnection, s: &Scope, work: &str) -> Result<Value> {
    tree::authorize(conn, s, work).await?;
    let row=sqlx::query("SELECT id,state,deadline,mode,wake_execution FROM work_waits WHERE work=? ORDER BY rowid DESC LIMIT 1").bind(work).fetch_optional(&mut *conn).await?;
    let Some(row) = row else {
        return Ok(json!({"work_id":work,"state":"none"}));
    };
    let id: String = row.try_get("id")?;
    Ok(
        json!({"work_id":work,"wait_id":id,"state":row.try_get::<String,_>("state")?,"mode":row.try_get::<String,_>("mode")?,"deadline_ms":row.try_get::<Option<i64>,_>("deadline")?,"wake_execution":row.try_get::<Option<String>,_>("wake_execution")?,"conditions":terms(conn,&id).await?}),
    )
}
pub async fn cancel(conn: &mut SqliteConnection, s: &Scope, work: &str) -> Result<Value> {
    tree::authorize(conn, s, work).await?;
    sqlx::query(
        "UPDATE work_waits SET state='cancelled' WHERE work=? AND state IN('proposed','armed')",
    )
    .bind(work)
    .execute(&mut *conn)
    .await?;
    status(conn, s, work).await
}
pub async fn propose(
    conn: &mut SqliteConnection,
    s: &Scope,
    exec: &str,
    op: &str,
    args: &Value,
) -> Result<Value> {
    let work = s.work.as_deref().ok_or("wait requires the current Work")?;
    tree::authorize(conn, s, work).await?;
    let conditions = match (args["conditions"].as_array(), args["run_id"].as_str()) {
        (Some(items), None) => items.clone(),
        (None, Some(run)) => vec![json!({"kind":"owned_run","run_id":run})],
        _ => return Err("provide exactly one original condition group".into()),
    };
    if conditions.is_empty() {
        return Err("wait conditions required".into());
    }
    let mode = args["wait_mode"].as_str().unwrap_or("any");
    if !matches!(mode, "any" | "all") {
        return Err("invalid wait mode".into());
    }
    let now = clock()?;
    let deadline = (!args["deadline_at"].is_null())
        .then(|| instant(&args["deadline_at"]))
        .transpose()?;
    let (logical,since,generation):(String,i64,i64)=sqlx::query_as("SELECT i.logical,m.event,t.window FROM model_tool_intents i JOIN work_request_marks m ON m.logical=i.logical JOIN main_turns t ON t.execution=i.execution WHERE i.execution=? AND i.operation_id=?").bind(exec).bind(op).fetch_one(&mut *conn).await?;
    let id = new_id(conn).await?;
    sqlx::query("INSERT INTO work_waits VALUES(?,?,?,?,?,?,?,?,?,?,'proposed',NULL)")
        .bind(&id)
        .bind(work)
        .bind(exec)
        .bind(&logical)
        .bind(op)
        .bind(mode)
        .bind(generation)
        .bind(since)
        .bind(now)
        .bind(deadline)
        .execute(&mut *conn)
        .await?;
    for (ord, term) in conditions.iter().enumerate() {
        let kind = term["kind"]
            .as_str()
            .ok_or("wait condition kind required")?;
        let (target, detail, due) = match kind {
            "time_due" => {
                let due = match (term.get("after_seconds"), term.get("at")) {
                    (Some(value), None) => {
                        let millis = value.as_f64().ok_or("numeric interval required")? * 1000.0;
                        if !millis.is_finite() || millis.abs() >= i64::MAX as f64 {
                            return Err("time interval overflow".into());
                        }
                        now.checked_add(millis.ceil() as i64)
                            .ok_or("deadline overflow")?
                    }
                    (None, Some(value)) => instant(value)?,
                    _ => return Err("exactly one time_due bound required".into()),
                };
                (None, None, Some(due))
            }
            "conversation" => {
                if term.as_object().is_none_or(|o| o.len() != 1) {
                    return Err(
                        "conversation condition takes no identity or subject filters".into(),
                    );
                }
                (None, None, None)
            }
            "owned_run" => {
                let key = term["run_id"]
                    .as_str()
                    .filter(|v| !v.is_empty())
                    .ok_or("owned run required")?;
                let known:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM work_nodes w WHERE w.parent=?1 AND (w.id=?2 OR EXISTS(SELECT 1 FROM work_runs r WHERE r.work=w.id AND r.execution=?2)))").bind(work).bind(key).fetch_one(&mut *conn).await?;
                if !known {
                    return Err("run is not owned by this Work".into());
                }
                (Some(key), None, None)
            }
            "plugin_event" => {
                let plugin = term["plugin_id"]
                    .as_str()
                    .filter(|v| !v.is_empty())
                    .ok_or("plugin ID required")?;
                let event_type = term["event_type"]
                    .as_str()
                    .filter(|v| !v.is_empty())
                    .ok_or("plugin event type required")?;
                let eligible:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM host_installations WHERE plugin=? AND enabled AND EXISTS(SELECT 1 FROM json_each(grants) WHERE value='gateway.inbound'))").bind(plugin).fetch_one(&mut *conn).await?;
                if !eligible {
                    return Err("event producer not installed with gateway grant".into());
                }
                (Some(plugin), Some(event_type), None)
            }
            _ => return Err("unsupported wait condition kind".into()),
        };
        sqlx::query(
            "INSERT INTO work_wait_terms(wait,ordinal,kind,target,detail,due) VALUES(?,?,?,?,?,?)",
        )
        .bind(&id)
        .bind(i64::try_from(ord)?)
        .bind(kind)
        .bind(target)
        .bind(detail)
        .bind(due)
        .execute(&mut *conn)
        .await?;
    }
    status(conn, s, work).await
}

async fn first_event(
    conn: &mut SqliteConnection,
    wait: &str,
    plugin: Option<(&str, &str)>,
) -> Result<Option<i64>> {
    let (plugin, kind) = plugin.map_or((None, None), |(p, k)| (Some(p), Some(k)));
    Ok(sqlx::query_scalar("SELECT e.id FROM work_waits w JOIN work_nodes n ON n.id=w.work JOIN events e ON e.entry_id=n.entry AND e.id>w.since_event JOIN accepted_sources s ON s.event_id=e.id JOIN persons p ON p.id=e.raw_actor AND p.enabled JOIN persons owner ON owner.id=n.principal AND owner.enabled JOIN assignments a ON a.id=e.actor_stage AND a.status='active' JOIN assignments b ON b.id=e.bot_stage AND b.status='active' JOIN entries d ON d.id=e.entry_id AND d.closed_at IS NULL WHERE w.id=? AND w.generation=coalesce((SELECT generation FROM main_windows WHERE conversation=n.entry),1)
     AND coalesce(p.current_root,p.id)=coalesce(owner.current_root,owner.id) AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)
     AND ((?2 IS NULL AND json_extract(e.envelope,'$.kind')='message') OR (?2 IS NOT NULL AND EXISTS(SELECT 1 FROM host_ingress h JOIN host_installations inst ON inst.plugin=h.plugin AND inst.enabled WHERE h.event=e.id AND h.plugin=?2 AND h.accepted AND json_extract(e.envelope,'$.kind')=?3))) ORDER BY e.id LIMIT 1")
        .bind(wait).bind(plugin).bind(kind).fetch_optional(conn).await?)
}
async fn candidate(
    conn: &mut SqliteConnection,
    id: &str,
    work: &str,
    term: &Value,
    now: i64,
) -> Result<(Option<String>, Option<i64>)> {
    let mut event = None;
    let hit = match term["kind"].as_str() {
        Some("time_due") if term["due_ms"].as_i64().is_some_and(|n| n <= now) => {
            Some(format!("time:{}", term["due_ms"]))
        }
        Some("owned_run") => {
            let key = term["target"].as_str().ok_or("lost child target")?;
            sqlx::query_scalar("SELECT CASE WHEN w.id=?2 THEN 'child:'||w.id||':'||w.status ELSE 'run:'||r.execution||':'||x.state END FROM work_nodes w LEFT JOIN work_runs r ON r.work=w.id LEFT JOIN executions x ON x.id=r.execution WHERE w.parent=?1 AND ((w.id=?2 AND w.status IN('completed','cancelled','failed')) OR (r.execution=?2 AND x.state IN('completed','held'))) LIMIT 1").bind(work).bind(key).fetch_optional(&mut *conn).await?
        }
        Some("conversation") | Some("plugin_event") => {
            let plugin = term["target"].as_str().zip(term["detail"].as_str());
            event = first_event(conn, id, plugin).await?;
            event.map(|e| format!("event:{e}"))
        }
        _ => None,
    };
    Ok((hit, event))
}

pub async fn ready(conn: &mut SqliteConnection, id: &str, now: i64) -> Result<bool> {
    let row=sqlx::query("SELECT w.work,w.execution,w.mode,w.deadline FROM work_waits w JOIN work_nodes n ON n.id=w.work WHERE w.id=? AND w.state='armed' AND n.goal IS NOT NULL AND n.status IN('active','need_input') AND NOT EXISTS(SELECT 1 FROM work_suspensions WHERE work=w.work)").bind(id).fetch_optional(&mut *conn).await?;
    let Some(row) = row else { return Ok(false) };
    let exec: String = row.try_get("execution")?;
    if crate::runtime::execution::eligible(conn, &exec)
        .await
        .is_err()
    {
        return Ok(false);
    }
    if row
        .try_get::<Option<i64>, _>("deadline")?
        .is_some_and(|n| n <= now)
    {
        return Ok(true);
    }
    let work: String = row.try_get("work")?;
    let conditions = terms(conn, id).await?;
    let mut hits = 0;
    for term in &conditions {
        if term["hit"].is_null() {
            if candidate(conn, id, &work, term, now).await?.0.is_some() {
                return Ok(true);
            }
        } else {
            hits += 1
        }
    }
    Ok(if row.try_get::<String, _>("mode")? == "any" {
        hits > 0
    } else {
        hits == conditions.len()
    })
}
async fn record_input(conn: &mut SqliteConnection, id: &str, work: &str, event: i64) -> Result<()> {
    let row=sqlx::query("SELECT e.envelope,w.execution FROM events e JOIN work_waits w ON w.id=? WHERE e.id=? AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)").bind(id).bind(event).fetch_one(&mut *conn).await?;
    let frame: crate::ingress::admit::Frame = serde_json::from_str(row.try_get("envelope")?)?;
    let body = json!({"event":event,"text":frame.text,"segments":frame.segments}).to_string();
    let mail = mailbox::put(
        conn,
        work,
        row.try_get("execution")?,
        &format!("wait-input:{event}"),
        "matched_input",
        &body,
        None,
    )
    .await?;
    sqlx::query("INSERT OR IGNORE INTO work_wait_routed VALUES(?,?)")
        .bind(event)
        .bind(work)
        .execute(&mut *conn)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO work_wait_mail VALUES(?,?)")
        .bind(mail)
        .bind(event)
        .execute(conn)
        .await?;
    Ok(())
}
async fn refresh(conn: &mut SqliteConnection, id: &str, work: &str, now: i64) -> Result<()> {
    for term in terms(conn, id).await? {
        if !term["hit"].is_null() {
            continue;
        }
        let (hit, event) = candidate(conn, id, work, &term, now).await?;
        if let Some(hit) = hit {
            if let Some(event) = event {
                record_input(conn, id, work, event).await?;
            }
            sqlx::query("UPDATE work_wait_terms SET hit=?,event=? WHERE wait=? AND ordinal=? AND hit IS NULL").bind(hit).bind(event).bind(id).bind(term["ordinal"].as_i64()).execute(&mut *conn).await?;
        }
    }
    Ok(())
}
pub async fn reconcile(conn: &mut SqliteConnection, id: &str, now: i64) -> Result<bool> {
    let row=sqlx::query("SELECT w.work,w.execution,w.mode,w.deadline FROM work_waits w JOIN work_nodes n ON n.id=w.work WHERE w.id=? AND w.state='armed' AND n.goal IS NOT NULL AND n.status IN('active','need_input') AND NOT EXISTS(SELECT 1 FROM work_suspensions WHERE work=w.work)").bind(id).fetch_optional(&mut *conn).await?;
    let Some(row) = row else { return Ok(false) };
    let work: String = row.try_get("work")?;
    let exec: String = row.try_get("execution")?;
    crate::runtime::execution::eligible(conn, &exec).await?;
    refresh(conn, id, &work, now).await?;
    let conditions = terms(conn, id).await?;
    let hit = conditions.iter().filter(|c| !c["hit"].is_null()).count();
    let complete = if row.try_get::<String, _>("mode")? == "any" {
        hit > 0
    } else {
        hit == conditions.len()
    };
    let expired = row
        .try_get::<Option<i64>, _>("deadline")?
        .is_some_and(|n| n <= now);
    if !complete && !expired {
        return Ok(false);
    }
    let key = format!("wait:{id}");
    let state = if complete { "woke" } else { "expired" };
    let events:Vec<i64>=sqlx::query_scalar("SELECT DISTINCT e.id FROM work_wait_terms c JOIN events e ON e.id=c.event WHERE c.wait=? AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id) ORDER BY e.id").bind(id).fetch_all(&mut *conn).await?;
    let input = json!({"wait_id":id,"outcome":state,"conditions":conditions,"events":events});
    mailbox::put(
        conn,
        &work,
        &exec,
        &key,
        "wait_signal",
        &input.to_string(),
        None,
    )
    .await?;
    let wake = tree::wake(conn, &work, &exec, &key).await?;
    for event in events {
        sqlx::query("INSERT OR IGNORE INTO main_dependencies VALUES(?,?)")
            .bind(&wake)
            .bind(event)
            .execute(&mut *conn)
            .await?;
    }
    sqlx::query("UPDATE work_waits SET state=?,wake_execution=? WHERE id=?")
        .bind(state)
        .bind(wake)
        .bind(id)
        .execute(conn)
        .await?;
    Ok(true)
}
pub async fn settle(conn: &mut SqliteConnection, exec: &str, logical: &str) -> Result<bool> {
    let id:Option<String>=sqlx::query_scalar("UPDATE work_waits SET state='armed' WHERE execution=? AND logical=? AND state='proposed' RETURNING id").bind(exec).bind(logical).fetch_optional(&mut *conn).await?;
    let Some(id) = id else {
        return Ok(false);
    };
    sqlx::query("UPDATE main_turns SET result=? WHERE execution=?")
        .bind(json!({"wait_id":id}).to_string())
        .bind(exec)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE executions SET state='completed' WHERE id=?")
        .bind(exec)
        .execute(&mut *conn)
        .await?;
    reconcile(conn, &id, clock()?).await?;
    Ok(true)
}
pub async fn route_event(conn: &mut SqliteConnection, event: i64) -> Result<bool> {
    if sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM work_wait_routed WHERE event=?)")
        .bind(event)
        .fetch_one(&mut *conn)
        .await?
    {
        return Ok(true);
    }
    if super::questions::route(conn, event, None).await? {
        return Ok(true);
    }
    let rows=sqlx::query("SELECT w.id,w.work,w.state FROM work_waits w JOIN work_nodes n ON n.id=w.work JOIN events e ON e.entry_id=n.entry WHERE e.id=? AND e.id>w.since_event AND w.state IN('proposed','armed') AND n.goal IS NOT NULL AND NOT EXISTS(SELECT 1 FROM work_suspensions WHERE work=n.id)").bind(event).fetch_all(&mut *conn).await?;
    let mut routed = false;
    for r in rows {
        let id: String = r.try_get("id")?;
        let work: String = r.try_get("work")?;
        refresh(conn, &id, &work, clock()?).await?;
        let matched: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM work_wait_terms WHERE wait=? AND event=?)",
        )
        .bind(&id)
        .bind(event)
        .fetch_one(&mut *conn)
        .await?;
        if matched {
            sqlx::query("INSERT OR IGNORE INTO work_wait_routed VALUES(?,?)")
                .bind(event)
                .bind(&work)
                .execute(&mut *conn)
                .await?;
            routed = true;
            if r.try_get::<String, _>("state")? == "armed" {
                reconcile(conn, &id, clock()?).await?;
            }
        }
    }
    Ok(routed)
}
