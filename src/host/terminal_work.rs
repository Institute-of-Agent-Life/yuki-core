//! Bind a Manager request and its observed run to the original Work, not model arguments.
use super::{calls::Request, domain};
use crate::{Result, privacy::source, runtime::effects};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

fn terminal(tool: &str) -> bool {
    matches!(tool, "terminal_exec" | "terminal_read" | "terminal_control")
}

async fn work(db: &mut SqliteConnection, exec: &str) -> Result<String> {
    let (work,actor):(String,String)=sqlx::query_as("SELECT w.id,coalesce(x.original_actor,x.origin) FROM executions x JOIN work_runs r ON r.execution=x.id AND r.work=x.work_id JOIN work_nodes w ON w.id=r.work JOIN executions owner ON owner.id=w.owner_execution WHERE x.id=? AND w.status IN('active','need_input') AND w.goal IS NOT NULL AND NOT EXISTS(SELECT 1 FROM work_suspensions WHERE work=w.id) AND NOT EXISTS(SELECT 1 FROM work_sources s JOIN invalidated_sources i ON i.source=s.event WHERE s.work=w.id) AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=owner.source OR i.source IN(SELECT event FROM main_dependencies WHERE execution=owner.id))").bind(exec).fetch_optional(&mut *db).await?.ok_or("active original Work required")?;
    effects::authorize(db, exec, &actor, true).await?;
    Ok(work)
}
pub(super) async fn open(
    db: &mut SqliteConnection,
    r: &Request,
    h: &str,
    generation: &str,
) -> Result<()> {
    if !terminal(&r.name) {
        return Ok(());
    }
    domain::active(db, &r.plugin, generation, "terminal.work").await?;
    if ["work_id", "execution", "device_id", "env", "request_id"]
        .iter()
        .any(|k| r.args.get(k).is_some())
    {
        return Err("server terminal scope is supplied by the Host".into());
    }
    let w = work(db, &r.execution).await?;
    let deployment: String = sqlx::query_scalar(
        "SELECT json_extract(value,'$.deployment') FROM core_meta WHERE key='plugin-approval:'||?",
    )
    .bind(&r.plugin)
    .fetch_one(&mut *db)
    .await?;
    let launch: Option<String> = if r.name == "terminal_exec" {
        None
    } else {
        let run = r.args["run_id"]
            .as_str()
            .ok_or("an observed Manager run is required")?;
        let (handle,e):(String,String)=sqlx::query_as("SELECT t.handle,c.execution FROM terminal_work_calls t JOIN host_calls c USING(handle) WHERE t.plugin=? AND t.deployment=? AND t.work=? AND t.run=?").bind(&r.plugin).bind(&deployment).bind(&w).bind(run).fetch_optional(&mut *db).await?.ok_or("run not bound to this Work")?;
        if work(db, &e).await? != w {
            return Err("launch Work changed".into());
        }
        Some(handle)
    };
    // A proven unsent call can return only on the exact frozen scope.
    sqlx::query_scalar::<_,String>("INSERT INTO terminal_work_calls(handle,work,plugin,deployment,tool,launch) VALUES(?,?,?,?,?,?) ON CONFLICT(handle) DO UPDATE SET handle=excluded.handle WHERE work=excluded.work AND plugin=excluded.plugin AND deployment=excluded.deployment AND tool=excluded.tool AND launch IS excluded.launch RETURNING handle").bind(h).bind(w).bind(&r.plugin).bind(deployment).bind(&r.name).bind(launch).fetch_one(&mut *db).await?;
    let events:Vec<i64>=sqlx::query_scalar("SELECT e.id FROM events e WHERE e.raw_actor IS NOT NULL AND e.id IN(SELECT source FROM executions WHERE id=?1 UNION SELECT event FROM main_dependencies WHERE execution=?1 UNION SELECT event FROM work_sources WHERE work=(SELECT work FROM terminal_work_calls WHERE handle=?2))").bind(&r.execution).bind(h).fetch_all(&mut *db).await?;
    for event in events {
        source::acquire(db, &r.plugin, h, event).await?;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum Access {
    Dispatch,
    Callback,
    Receipt,
}
async fn scope(
    db: &mut SqliteConnection,
    h: &str,
    plugin: &str,
    generation: &str,
    access: Access,
) -> Result<Value> {
    domain::active(db, plugin, generation, "terminal.work").await?;
    domain::active(db, plugin, generation, "tool.call").await?;
    domain::active(db, plugin, generation, "forget.owner").await?;
    let r=sqlx::query("SELECT t.work,t.tool,t.launch,t.run,c.operation,c.execution,c.state,c.generation,c.args,l.run AS target,lc.execution AS origin FROM terminal_work_calls t JOIN host_calls c USING(handle) JOIN host_installations i ON i.plugin=t.plugin AND i.revision=c.revision JOIN core_meta a ON a.key='plugin-approval:'||t.plugin AND json_extract(a.value,'$.deployment')=t.deployment LEFT JOIN terminal_work_calls l ON l.handle=t.launch AND l.work=t.work AND l.deployment=t.deployment AND l.plugin=t.plugin LEFT JOIN host_calls lc ON lc.handle=l.handle WHERE t.handle=? AND t.plugin=?").bind(h).bind(plugin).fetch_one(&mut *db).await?;
    let tool: &str = r.get("tool");
    let exec: &str = r.get("execution");
    let w: &str = r.get("work");
    let state: &str = r.get("state");
    if work(db, exec).await? != w {
        return Err("terminal Work changed".into());
    }
    let recovery = tool == "terminal_exec" && matches!(state, "unknown" | "confirmed");
    if access != Access::Receipt
        && !recovery
        && (generation != r.get::<&str, _>("generation")
            || !(state == "dispatched" || access == Access::Dispatch && state == "queued"))
    {
        return Err("terminal call is no longer dispatched".into());
    }
    let args: Value = serde_json::from_str(r.get("args"))?;
    let (mode, run) = if tool == "terminal_exec" {
        (
            if recovery || r.get::<Option<&str>, _>("run").is_some() {
                "reconcile"
            } else {
                "launch"
            },
            r.get::<Option<&str>, _>("run"),
        )
    } else {
        let origin: &str = r
            .try_get::<Option<&str>, _>("origin")?
            .ok_or("missing launch")?;
        if work(db, origin).await? != w
            || r.get::<Option<&str>, _>("target") != args["run_id"].as_str()
        {
            return Err("run outside original launch".into());
        }
        let mode = if tool == "terminal_read" {
            "read"
        } else {
            args["action"]
                .as_str()
                .filter(|a| matches!(*a, "input" | "interrupt" | "cancel" | "close"))
                .ok_or("invalid terminal action")?
        };
        (mode, r.get::<Option<&str>, _>("target"))
    };
    Ok(
        json!({"handle":h,"execution":exec,"work_id":w,"installation":plugin,"operation":r.get::<&str,_>("operation"),"request_id":h,"launch_request":r.get::<Option<&str>,_>("launch").unwrap_or(h),"run_id":run,"mode":mode,"tool":tool}),
    )
}
pub(super) async fn check(
    db: &mut SqliteConnection,
    h: &str,
    plugin: &str,
    generation: &str,
) -> Result<()> {
    let known: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM terminal_work_calls WHERE handle=?)")
            .bind(h)
            .fetch_one(&mut *db)
            .await?;
    if known {
        scope(db, h, plugin, generation, Access::Dispatch).await?;
    }
    Ok(())
}
pub(super) async fn receipt(
    db: &mut SqliteConnection,
    h: &str,
    plugin: &str,
    generation: &str,
) -> Result<bool> {
    let known: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM terminal_work_calls WHERE handle=?)")
            .bind(h)
            .fetch_one(&mut *db)
            .await?;
    if known {
        scope(db, h, plugin, generation, Access::Receipt).await?;
    }
    Ok(known)
}
pub(super) async fn callback(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    method: &str,
    p: &Value,
) -> Result<Value> {
    let h = p["handle"].as_str().ok_or("missing original handle")?;
    let s = scope(db, h, plugin, generation, Access::Callback).await?;
    if method == "terminal.scope" {
        return Ok(s);
    }
    if s["tool"] != "terminal_exec" || p["request_id"] != h {
        return Err("receipt is not for the original launch".into());
    }
    let run = p["run_id"].as_str().ok_or("missing Manager run")?;
    sqlx::query_scalar::<_,String>("UPDATE terminal_work_calls SET run=?2 WHERE handle=?1 AND (run IS NULL OR run=?2) RETURNING run").bind(h).bind(run).fetch_one(db).await?;
    Ok(json!({"request_id":h,"run_id":run}))
}
