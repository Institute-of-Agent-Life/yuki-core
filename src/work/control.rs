use super::{
    mailbox,
    tree::{self, Scope},
};
use crate::{Result, runtime::main::Config};
use serde_json::{Value, json};
use sqlx::SqliteConnection;

fn field<'a>(a: &'a Value, key: &str) -> Result<&'a str> {
    a[key]
        .as_str()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| format!("missing {key}").into())
}
async fn list(
    conn: &mut SqliteConnection,
    s: &Scope,
    a: &Value,
    children_only: bool,
) -> Result<Value> {
    let parent = a["parent_work_id"].as_str();
    if let Some(parent) = parent {
        tree::authorize(conn, s, parent).await?;
    }
    let status = a["status"]
        .as_str()
        .unwrap_or(if children_only { "all" } else { "active" });
    let limit = a["limit"].as_i64().unwrap_or(10).clamp(1, 50);
    let rows:Vec<String>=sqlx::query_scalar("WITH RECURSIVE mine(id) AS(SELECT ?1 UNION SELECT w.id FROM work_nodes w JOIN mine m ON w.parent=m.id) SELECT w.id FROM work_nodes w JOIN executions owner ON owner.id=w.owner_execution WHERE w.principal=?2 AND w.entry=?3 AND w.goal IS NOT NULL AND (NOT ?4 OR w.id IN mine) AND (?5 IS NULL OR w.parent=?5) AND (NOT ?6 OR w.parent IS NOT NULL) AND w.id>?7 AND (?8='all' OR (?8='terminal')=(w.status IN('completed','failed','cancelled'))) AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=owner.source OR i.source IN (SELECT event FROM main_dependencies WHERE execution=owner.id)) ORDER BY w.id LIMIT ?9")
        .bind(&s.work).bind(&s.principal).bind(&s.entry).bind(s.child).bind(parent).bind(children_only).bind(a["cursor"].as_str().unwrap_or("")).bind(status).bind(limit+1).fetch_all(&mut *conn).await?;
    let mut items = Vec::new();
    for id in rows.iter().take(limit as usize) {
        items.push(tree::detail(conn, s, id).await?);
    }
    Ok(
        json!({"items":items,"next_cursor":if rows.len()>limit as usize{rows.get(limit as usize-1)}else{None}}),
    )
}

pub(super) async fn stop(
    conn: &mut SqliteConnection,
    s: &Scope,
    id: &str,
    exec: &str,
    op: &str,
    status: &str,
    reason: &str,
) -> Result<Value> {
    tree::authorize(conn, s, id).await?;
    let nodes:Vec<String>=sqlx::query_scalar("WITH RECURSIVE branch(id,depth) AS(SELECT ?,0 UNION ALL SELECT w.id,b.depth+1 FROM work_nodes w JOIN branch b ON w.parent=b.id) SELECT id FROM branch ORDER BY depth DESC,id").bind(id).fetch_all(&mut *conn).await?;
    for node in &nodes {
        sqlx::query("UPDATE executions SET cancelled=1,state='held' WHERE id IN(SELECT execution FROM work_runs WHERE work=?) AND state IN('pending','running')").bind(node).execute(&mut *conn).await?;
        tree::terminal(
            conn,
            node,
            exec,
            &format!("{op}:{node}"),
            if node == id { status } else { "cancelled" },
            reason,
        )
        .await?;
    }
    Ok(json!({"work_id":id,"status":status,"affected":nodes}))
}
async fn propose(
    conn: &mut SqliteConnection,
    s: &Scope,
    exec: &str,
    op: &str,
    kind: &str,
    text: &str,
) -> Result<Value> {
    let work = s.work.as_deref().ok_or("current Work required")?;
    tree::authorize(conn, s, work).await?;
    sqlx::query("INSERT INTO work_proposals(execution,operation,logical,work,summary,kind) SELECT execution,operation_id,logical,?,?,? FROM model_tool_intents WHERE execution=? AND operation_id=? RETURNING operation")
        .bind(work).bind(text).bind(kind).bind(exec).bind(op).fetch_one(&mut *conn).await?;
    Ok(json!({"work_id":work,"proposal":kind,"settlement":"after_original_batch"}))
}
async fn steer(
    conn: &mut SqliteConnection,
    s: &Scope,
    id: &str,
    exec: &str,
    op: &str,
    instruction: &str,
) -> Result<Value> {
    tree::authorize(conn, s, id).await?;
    if instruction.trim().is_empty() {
        return Err("new Work input required".into());
    }
    sqlx::query("DELETE FROM work_suspensions WHERE work=?")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "UPDATE work_waits SET state='cancelled' WHERE work=? AND state IN('proposed','armed')",
    )
    .bind(id)
    .execute(&mut *conn)
    .await?;
    mailbox::put(conn, id, exec, op, "instruction", instruction, None).await?;
    let active:Option<String>=sqlx::query_scalar("SELECT x.id FROM work_runs r JOIN executions x ON x.id=r.execution WHERE r.work=? AND x.state IN('pending','running') AND NOT x.cancelled ORDER BY x.rowid DESC LIMIT 1").bind(id).fetch_optional(&mut *conn).await?;
    let run = if let Some(run) = active {
        run
    } else {
        tree::wake(conn, id, exec, op).await?
    };
    Ok(json!({"work_id":id,"execution":run,"status":"active"}))
}
async fn derive(
    conn: &mut SqliteConnection,
    cfg: &Config,
    s: &Scope,
    exec: &str,
    op: &str,
    a: &Value,
) -> Result<Value> {
    let current = s.work.as_deref().ok_or("derive requires current Work")?;
    let parent = a["parent_work_id"].as_str().unwrap_or(current);
    let id = tree::create(conn, cfg, exec, s, field(a, "goal")?, Some(parent), false).await?;
    if !a["context"].is_null() || !a["files"].is_null() || !a["acceptance"].is_null() {
        mailbox::put(
            conn,
            &id,
            exec,
            op,
            "assignment",
            &json!({"context":a["context"],"files":a["files"],"acceptance":a["acceptance"]})
                .to_string(),
            None,
        )
        .await?;
    }
    let run = tree::wake(conn, &id, exec, op).await?;
    Ok(json!({"child_id":id,"work_id":id,"parent_work_id":parent,"execution":run}))
}
pub async fn run(
    conn: &mut SqliteConnection,
    cfg: &Config,
    s: &Scope,
    exec: &str,
    op: &str,
    name: &str,
    a: &Value,
) -> Result<Value> {
    if name == "subagent_start" {
        return derive(conn, cfg, s, exec, op, a).await;
    }
    if name == "complete" {
        return propose(conn, s, exec, op, "completed", field(a, "summary")?).await;
    }
    let action = field(a, "action")?;
    if name == "subagent_control" {
        if action == "list" {
            return list(conn, s, a, true).await;
        }
        let parent: Option<String> = if a["child_id"].is_null() && action == "message" {
            sqlx::query_scalar("SELECT parent FROM work_nodes WHERE id=?")
                .bind(&s.work)
                .fetch_optional(&mut *conn)
                .await?
                .flatten()
        } else {
            None
        };
        let id = a["child_id"]
            .as_str()
            .or(parent.as_deref())
            .ok_or("child ID required")?;
        if parent.is_some() {
            let body = field(a, "text")?;
            if a["ask"] == true {
                let work = s.work.as_deref().ok_or("current child Work required")?;
                let response = super::questions::parent_question(conn, work, exec, op, body)
                    .await?
                    .ok_or("unknown original parent")?;
                if response["wait"] == true {
                    propose(conn, s, exec, op, "need_input", body).await?;
                }
                return Ok(response);
            }
            let key = mailbox::put(conn, id, exec, op, "message", body, None).await?;
            return Ok(json!({"work_id":id,"message_id":key.to_string()}));
        }
        tree::authorize(conn, s, id).await?;
        let owned: bool =
            sqlx::query_scalar("SELECT parent IS NOT NULL FROM work_nodes WHERE id=?")
                .bind(id)
                .fetch_one(&mut *conn)
                .await?;
        if !owned {
            return Err("original target is not a child".into());
        }
        return match action {
            "status" | "result" => tree::detail(conn, s, id).await,
            "resume" => steer(conn, s, id, exec, op, field(a, "instruction")?).await,
            "cancel" => {
                stop(
                    conn,
                    s,
                    id,
                    exec,
                    op,
                    "cancelled",
                    "cancelled by original manager",
                )
                .await
            }
            "message" => {
                let reply = a["reply_to"].as_str().map(str::parse::<i64>).transpose()?;
                let body = field(a, "text")?;
                let key = mailbox::put(
                    conn,
                    id,
                    exec,
                    op,
                    if reply.is_some() { "reply" } else { "message" },
                    body,
                    reply,
                )
                .await?;
                if reply.is_some() {
                    let status: String =
                        sqlx::query_scalar("SELECT status FROM work_nodes WHERE id=?")
                            .bind(id)
                            .fetch_one(&mut *conn)
                            .await?;
                    if status == "need_input" {
                        tree::wake(conn, id, exec, op).await?;
                    }
                }
                Ok(json!({"work_id":id,"message_id":key.to_string()}))
            }
            _ => Err("unknown child action".into()),
        };
    }
    if name != "task_control" {
        return Err("unknown Work tool".into());
    }
    let id = a["work_id"].as_str().or(s.work.as_deref());
    match action {
        "wait" => super::wait::propose(conn, s, exec, op, a).await,
        "wait_status" => super::wait::status(conn, s, id.ok_or("original Work required")?).await,
        "cancel_wait" => super::wait::cancel(conn, s, id.ok_or("original Work required")?).await,
        "list" => list(conn, s, a, false).await,
        "get" => tree::detail(conn, s, id.ok_or("original Work required")?).await,
        "accept" => {
            if s.child {
                return Err("delegated worker already owns its Work".into());
            }
            let queued = s.work.is_some();
            let id = tree::create(conn, cfg, exec, s, field(a, "goal")?, None, queued).await?;
            let event: Option<i64> = sqlx::query_scalar(
                "SELECT max(event) FROM main_inputs WHERE execution=? AND consumed",
            )
            .bind(exec)
            .fetch_one(&mut *conn)
            .await?;
            if queued && event.is_none() {
                return Err("independent Work requires new original human input".into());
            }
            if let Some(event) = event {
                sqlx::query("INSERT INTO work_accept_sources VALUES(?,?)")
                    .bind(event)
                    .bind(&id)
                    .execute(&mut *conn)
                    .await
                    .map_err(|_| "original input already accepted; update or resume its Work")?;
            }
            if queued {
                tree::wake(conn, &id, exec, op).await?;
            } else {
                tree::attach(conn, &id, exec, op, "accept").await?;
            }
            Ok(json!({"work_id":id,"attached":!queued,"queued":queued}))
        }
        "derive" => derive(conn, cfg, s, exec, op, a).await,
        "resume" => {
            steer(
                conn,
                s,
                id.ok_or("original Work required")?,
                exec,
                op,
                a["goal"]
                    .as_str()
                    .or(a["reason"].as_str())
                    .unwrap_or("Resume the original Work"),
            )
            .await
        }
        "update" => {
            let id = s.work.as_deref().ok_or("update requires current Work")?;
            if a["work_id"].as_str().is_some_and(|target| target != id) {
                return Err("update only changes current Work".into());
            }
            tree::authorize(conn, s, id).await?;
            if a["context_note"].is_null() && a["goal"].as_str().is_none_or(|s| s.trim().is_empty())
            {
                return Err("goal or context_note required".into());
            }
            if !a["context_note"].is_null() {
                super::note::validate(conn, id, exec, &a["context_note"]).await?;
            }
            tree::provenance(conn, id, exec).await?;
            sqlx::query(
                "UPDATE work_nodes SET goal=coalesce(?,goal),note=coalesce(?,note) WHERE id=?",
            )
            .bind(a["goal"].as_str())
            .bind((!a["context_note"].is_null()).then(|| a["context_note"].to_string()))
            .bind(id)
            .execute(&mut *conn)
            .await?;
            tree::detail(conn, s, id).await
        }
        "complete" | "need_input" => {
            let kind = if action == "complete" {
                "completed"
            } else {
                "need_input"
            };
            if id != s.work.as_deref() {
                return Err("proposal requires current Work".into());
            }
            if kind == "need_input"
                && let Some(work) = s.work.as_deref()
                && let Some(parent) =
                    super::questions::parent_question(conn, work, exec, op, field(a, "reason")?)
                        .await?
                && parent["wait"] == false
            {
                return Ok(parent);
            }
            propose(
                conn,
                s,
                exec,
                op,
                kind,
                if kind == "need_input" {
                    field(a, "reason")?
                } else {
                    a["result"].as_str().unwrap_or("")
                },
            )
            .await
        }
        "fail" | "cancel" => {
            stop(
                conn,
                s,
                id.ok_or("original Work required")?,
                exec,
                op,
                if action == "cancel" {
                    "cancelled"
                } else {
                    "failed"
                },
                a["reason"].as_str().unwrap_or(""),
            )
            .await
        }
        _ => Err("unsupported Work action".into()),
    }
}
