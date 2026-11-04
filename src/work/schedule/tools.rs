use super::clock::Trigger;
use crate::{
    Result,
    runtime::main::Config,
    tools::registry::Tool,
    work::tree::{Scope, tick},
};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::sync::LazyLock;

static GOLD: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../../fixtures/schedule/v4-clock-tools.json"
    ))
    .expect("original automation definitions")
});

pub fn declaration(name: &str) -> Option<Tool> {
    let def = GOLD["tools"].get(name)?;
    let mut schema = def["parameters"].clone();
    if name == "automation_manage" {
        schema["properties"]["task"]["properties"]["trigger"]["oneOf"].as_array_mut()?.push(json!({
            "type":"object","additionalProperties":false,"required":["type","expression"],"properties":{
                "type":{"const":"cron"},"expression":{"type":"string","minLength":1},"timezone":{"type":["string","null"]}}
        }));
    }
    Some(Tool {
        name: name.into(),
        description: def["description"].as_str()?.into(),
        schema,
        parallel_read: false,
        cacheable: false,
        code_direct: false,
        provider: None,
    })
}

async fn authorize(conn: &mut SqliteConnection, caller: &Scope, id: i64) -> Result<()> {
    let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM schedules s LEFT JOIN persons owner ON owner.id=s.owner
        LEFT JOIN persons p ON p.id=?1 LEFT JOIN persons root ON root.id=coalesce(p.current_root,p.id)
        WHERE s.id=?2 AND (s.owner=?1 OR (p.enabled AND root.enabled AND (root.admin OR root.id=coalesce(owner.current_root,owner.id)))))")
        .bind(&caller.principal).bind(id).fetch_one(conn).await?;
    if !allowed {
        return Err("schedule owner or current administrator required".into());
    }
    Ok(())
}
async fn definition(conn: &mut SqliteConnection, id: i64) -> Result<Value> {
    let r=sqlx::query("SELECT s.*,v.goal,v.name,v.trigger_json,v.zone FROM schedules s JOIN schedule_versions v ON v.schedule=s.id AND v.version=s.version WHERE s.id=? AND v.goal IS NOT NULL")
        .bind(id).fetch_one(conn).await?;
    Ok(
        json!({"automation_id":id,"owner":r.get::<String,_>("owner"),"name":r.get::<Option<String>,_>("name"),"goal":r.get::<String,_>("goal"),
      "trigger":serde_json::from_str::<Value>(r.get("trigger_json"))?,"timezone":r.get::<String,_>("zone"),"version":r.get::<i64,_>("version"),"status":r.get::<String,_>("status"),
      "run_count":r.get::<i64,_>("run_count"),"max_runs":r.get::<Option<i64>,_>("max_runs"),"next_due_utc_ms":r.get::<Option<i64>,_>("next_due")}),
    )
}
async fn evidence(conn: &mut SqliteConnection, id: i64, version: i64, exec: &str) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO schedule_sources SELECT ?1,?2,e.id FROM events e WHERE e.id IN (SELECT source FROM executions WHERE id=?3 UNION SELECT event FROM main_dependencies WHERE execution=?3)")
        .bind(id).bind(version).bind(exec).execute(conn).await?;
    Ok(())
}
async fn write_version(
    conn: &mut SqliteConnection,
    id: i64,
    version: i64,
    task: &Value,
    zone: &str,
    exec: &str,
) -> Result<()> {
    let goal = task["goal"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("nonblank goal required")?;
    let name = task["name"].as_str();
    if name.is_some_and(|s| s.trim().is_empty()) {
        return Err("public name must be nonblank".into());
    }
    sqlx::query("INSERT INTO schedule_versions VALUES(?,?,?,?,?,?)")
        .bind(id)
        .bind(version)
        .bind(goal)
        .bind(name)
        .bind(task["trigger"].to_string())
        .bind(zone)
        .execute(&mut *conn)
        .await?;
    evidence(conn, id, version, exec).await
}
fn max_runs(args: &Value, prior: Option<i64>) -> Result<Option<i64>> {
    match args.get("max_runs") {
        None => Ok(prior),
        Some(Value::Null) => Ok(None),
        Some(n) => Ok(Some(
            n.as_i64()
                .filter(|n| *n > 0)
                .ok_or("positive max_runs required")?,
        )),
    }
}
async fn create(
    conn: &mut SqliteConnection,
    cfg: &Config,
    scope: &Scope,
    exec: &str,
    args: &Value,
    now: i64,
) -> Result<Value> {
    let source:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM persons WHERE id=?1 AND enabled) OR EXISTS(SELECT 1 FROM main_origins WHERE ref=?1 AND kind='self' AND enabled)")
        .bind(&scope.principal).fetch_one(&mut *conn).await?;
    if !source {
        return Err("an original person or registered SELF is required".into());
    }
    let task = &args["task"];
    let trigger: Trigger = serde_json::from_value(task["trigger"].clone())?;
    let zone = cfg.timezone.as_deref().unwrap_or("Asia/Shanghai");
    let due = trigger.initial(now, zone)?;
    // A SELF group has only the explicitly selected receiver. Never infer it from the latest human.
    let stage:Option<String>=sqlx::query_scalar("SELECT g.stage FROM recovered_groups g JOIN entries e ON e.id=?1 AND e.scope='group'
        WHERE json_extract(e.space,'$[0]')='qq' AND json_extract(e.space,'$[1]')=g.namespace AND json_extract(e.space,'$[2]')=g.native_group AND g.enabled
        AND EXISTS(SELECT 1 FROM main_origins WHERE ref=?2 AND kind='self')").bind(&scope.entry).bind(&scope.principal).fetch_optional(&mut *conn).await?;
    let id:i64=sqlx::query_scalar("INSERT INTO schedules(owner,creator_execution,entry,status,next_due,max_runs,self_receiver) VALUES(?,?,?,'active',?,?,?) RETURNING id")
        .bind(&scope.principal).bind(exec).bind(&scope.entry).bind(due).bind(max_runs(args,None)?).bind(stage).fetch_one(&mut *conn).await?;
    write_version(conn, id, 1, task, zone, exec).await?;
    let mut result = definition(conn, id).await?;
    result["confirmation"] = json!("persisted");
    Ok(result)
}
async fn update(
    conn: &mut SqliteConnection,
    id: i64,
    exec: &str,
    args: &Value,
    now: i64,
) -> Result<Value> {
    let current = definition(conn, id).await?;
    if matches!(current["status"].as_str(), Some("cancelled" | "completed")) {
        return Err("terminal schedule".into());
    }
    let zone = current["timezone"]
        .as_str()
        .ok_or("missing schedule timezone")?;
    let mut task = args["task"].clone();
    if task.get("name").is_none() {
        task["name"] = current["name"].clone();
    }
    let trigger: Trigger = serde_json::from_value(task["trigger"].clone())?;
    let changed = trigger != serde_json::from_value(current["trigger"].clone())?;
    let due = if changed {
        Some(trigger.initial(now, zone)?)
    } else {
        current["next_due_utc_ms"].as_i64()
    };
    let version = current["version"].as_i64().ok_or("missing version")? + 1;
    write_version(conn, id, version, &task, zone, exec).await?;
    // The creating event remains the basis for every version, even across an admin edit.
    let creator: String = sqlx::query_scalar("SELECT creator_execution FROM schedules WHERE id=?")
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    evidence(conn, id, version, &creator).await?;
    let cap = max_runs(args, current["max_runs"].as_i64())?;
    sqlx::query("UPDATE schedules SET version=?,max_runs=?,next_due=? WHERE id=?")
        .bind(version)
        .bind(cap)
        .bind(due)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE schedules SET status='completed',next_due=NULL WHERE id=? AND max_runs<=run_count AND NOT EXISTS(SELECT 1 FROM schedule_occurrences WHERE schedule=? AND status='running')")
        .bind(id).bind(id).execute(&mut *conn).await?;
    definition(conn, id).await
}
async fn query(conn: &mut SqliteConnection, scope: &Scope, args: &Value) -> Result<Value> {
    let limit = args["limit"].as_u64().unwrap_or(20).clamp(1, 100) as i64;
    let offset = args
        .get("cursor")
        .map(|s| s.as_str().ok_or("invalid cursor"))
        .transpose()?
        .unwrap_or("0")
        .parse::<u32>()?;
    let view = args["view"].as_str().ok_or("missing view")?;
    if view == "list" {
        let status = args["status"].as_str().unwrap_or("active");
        let rows=sqlx::query("SELECT s.id,v.name,s.status,s.next_due FROM schedules s JOIN schedule_versions v ON v.schedule=s.id AND v.version=s.version WHERE (?1='all' OR status=?1) ORDER BY s.id LIMIT ?2 OFFSET ?3")
            .bind(status).bind(limit+1).bind(offset).fetch_all(conn).await?;
        let items:Vec<_>=rows.iter().take(limit as usize).map(|r|json!({"automation_id":r.get::<i64,_>("id"),"name":r.get::<Option<String>,_>("name"),"status":r.get::<String,_>("status"),"next_due_utc_ms":r.get::<Option<i64>,_>("next_due")})).collect();
        return Ok(
            json!({"tasks":items,"next_cursor":(rows.len()>limit as usize).then(||(i64::from(offset)+limit).to_string())}),
        );
    }
    let id = args["automation_id"]
        .as_i64()
        .ok_or("missing schedule ID")?;
    authorize(conn, scope, id).await?;
    if view == "get" {
        return definition(conn, id).await;
    }
    if view != "history" {
        return Err("unknown schedule view".into());
    }
    let rows=sqlx::query("SELECT o.*,v.goal FROM schedule_occurrences o JOIN schedule_versions v ON v.schedule=o.schedule AND v.version=o.version WHERE o.schedule=? ORDER BY o.id DESC LIMIT ? OFFSET ?")
        .bind(id).bind(limit+1).bind(offset).fetch_all(conn).await?;
    let runs:Vec<_>=rows.iter().take(limit as usize).map(|r|json!({"execution":r.get::<String,_>("execution"),"version":r.get::<i64,_>("version"),"planned_utc_ms":r.get::<i64,_>("planned_utc"),"definition":r.get::<Option<String>,_>("goal"),"owner":r.get::<String,_>("owner"),"status":r.get::<String,_>("status"),"work":r.get::<Option<String>,_>("work"),"diagnostic":r.get::<Option<String>,_>("diagnostic")})).collect();
    Ok(
        json!({"runs":runs,"next_cursor":(rows.len()>limit as usize).then(||(i64::from(offset)+limit).to_string())}),
    )
}
pub(crate) async fn action(
    conn: &mut SqliteConnection,
    cfg: &Config,
    scope: &Scope,
    exec: &str,
    name: &str,
    args: &Value,
) -> Result<Value> {
    if name == "automation_query" {
        return query(conn, scope, args).await;
    }
    let now = tick()?;
    let action = args["action"].as_str().ok_or("missing schedule action")?;
    if action == "create" {
        return create(conn, cfg, scope, exec, args, now).await;
    }
    let id = args["automation_id"]
        .as_i64()
        .ok_or("missing schedule id")?;
    authorize(conn, scope, id).await?;
    if action == "update" {
        return update(conn, id, exec, args, now).await;
    }
    state_change(conn, id, action, now).await
}
pub(crate) async fn state_change(
    conn: &mut SqliteConnection,
    id: i64,
    action: &str,
    now: i64,
) -> Result<Value> {
    let sql = match action {
        "pause" => {
            "UPDATE schedules SET status='paused' WHERE id=?1 AND status NOT IN('cancelled','completed')"
        }
        "resume" => {
            "UPDATE schedules SET status='active',next_due=coalesce(next_due,?2) WHERE id=?1 AND status IN('paused','failed','blocked') AND EXISTS(SELECT 1 FROM schedule_versions v WHERE v.schedule=?1 AND v.version=schedules.version AND v.goal IS NOT NULL)"
        }
        "cancel" => {
            "UPDATE schedules SET status='cancelled',next_due=NULL WHERE id=?1 AND status IN('active','paused')"
        }
        "run_now" => {
            "UPDATE schedules SET next_due=?2,status='active' WHERE id=?1 AND status NOT IN('cancelled','completed') AND EXISTS(SELECT 1 FROM schedule_versions v WHERE v.schedule=?1 AND v.version=schedules.version AND v.goal IS NOT NULL)"
        }
        _ => return Err("unknown schedule action".into()),
    };
    let mut q = sqlx::query(sql).bind(id);
    if sql.contains("?2") {
        q = q.bind(now);
    }
    let changed = q.execute(&mut *conn).await?.rows_affected() > 0;
    Ok(json!({"automation_id":id,"changed":changed}))
}
