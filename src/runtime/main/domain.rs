use super::{Action, Config};
use crate::{Result, identity::new_id, ingress::admit, model::chat::Input, runtime::execution};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub async fn window(conn: &mut SqliteConnection, entry: &str) -> Result<i64> {
    Ok(sqlx::query_scalar("INSERT INTO main_windows(conversation,generation) VALUES(?,1) ON CONFLICT(conversation) DO UPDATE SET conversation=excluded.conversation RETURNING generation")
        .bind(entry).fetch_one(conn).await?)
}
pub async fn operator(conn: &mut SqliteConnection, config: &Config, cmd: &Action) -> Result<Value> {
    if matches!(cmd,Action::Register{kind,..} if kind=="memory_extract")
        || matches!(cmd,Action::Register{id,..}|Action::Trigger{id,..}|Action::Revoke{id} if id.starts_with("memory-extract:"))
    {
        return Err("extraction origin belongs to its original batch".into());
    }
    match cmd {
        Action::Register {
            kind,
            id,
            conversation,
            parent,
        } => {
            if matches!(kind.as_str(), "work" | "child") != parent.is_some() {
                return Err("Work and child bindings require an original parent".into());
            }
            if let Some(parent) = parent {
                execution::eligible(conn, parent).await?;
                let original: String =
                    sqlx::query_scalar("SELECT conversation FROM main_turns WHERE execution=?")
                        .bind(parent)
                        .fetch_one(&mut *conn)
                        .await?;
                if original != *conversation {
                    return Err("parent belongs to another conversation".into());
                }
            }
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM entries WHERE id=? AND closed_at IS NULL",
            )
            .bind(conversation)
            .fetch_one(&mut *conn)
            .await?;
            sqlx::query("INSERT INTO main_origins(ref,kind,conversation,parent) VALUES(?,?,?,?) ON CONFLICT(ref) DO NOTHING")
              .bind(id).bind(kind).bind(conversation).bind(parent).execute(&mut *conn).await?;
            let old: (String, String, Option<String>) =
                sqlx::query_as("SELECT kind,conversation,parent FROM main_origins WHERE ref=?")
                    .bind(id)
                    .fetch_one(&mut *conn)
                    .await?;
            if old != (kind.clone(), conversation.clone(), parent.clone()) {
                return Err("registered origin conflicts".into());
            }
            Ok(json!({"origin":id,"kind":kind,"conversation":conversation}))
        }
        Action::Trigger { id, key, body } => {
            let input: Input = serde_json::from_str(body)?;
            if key.is_empty() || input.text.trim().is_empty() {
                return Err("registered trigger and text required".into());
            }
            if let Some(task) = &input.structured {
                jsonschema::validator_for(&task.schema).map_err(|e| e.to_string())?;
                if task.validation_retries > 2 {
                    return Err("validation retries exceed policy".into());
                }
            }
            let canonical = serde_json::to_string(&input)?;
            let row=sqlx::query("SELECT o.conversation,o.parent FROM main_origins o JOIN entries e ON e.id=o.conversation WHERE ref=? AND o.enabled AND e.closed_at IS NULL")
                .bind(id).fetch_one(&mut *conn).await?;
            if let Some(existing) = sqlx::query(
                "SELECT execution,material FROM main_turns WHERE origin=? AND occurrence=?",
            )
            .bind(id)
            .bind(key)
            .fetch_optional(&mut *conn)
            .await?
            {
                if existing
                    .try_get::<Option<String>, _>("material")?
                    .as_deref()
                    != Some(&canonical)
                {
                    return Err("internal occurrence conflicts or is erased".into());
                }
                return Ok(json!({"execution":existing.try_get::<String,_>("execution")?}));
            }
            let entry: String = row.try_get("conversation")?;
            let generation = window(conn, &entry).await?;
            let exec = new_id(conn).await?;
            sqlx::query("INSERT INTO executions(id,state,origin) VALUES(?,'pending',?)")
                .bind(&exec)
                .bind(id)
                .execute(&mut *conn)
                .await?;
            sqlx::query("INSERT INTO main_turns(execution,conversation,window,origin,occurrence,material,ready_after) VALUES(?,?,?,?,?,?,0)")
                .bind(&exec).bind(entry).bind(generation).bind(id).bind(key).bind(canonical).execute(&mut *conn).await?;
            if let Some(parent) = row.try_get::<Option<String>, _>("parent")? {
                execution::eligible(conn, &parent).await?;
                sqlx::query("INSERT INTO budget_members(execution,root) SELECT ?,root FROM budget_members WHERE execution=?")
                    .bind(&exec).bind(&parent).execute(&mut *conn).await?;
                sqlx::query("INSERT INTO main_dependencies(execution,event) SELECT ?,event FROM main_dependencies WHERE execution=?")
                    .bind(&exec).bind(&parent).execute(&mut *conn).await?;
            } else {
                execution::root(conn, &exec, config.root_slots, config.max_input_units).await?;
            }
            Ok(json!({"execution":exec}))
        }
        Action::Revoke { id } => {
            sqlx::query_scalar::<_, String>(
                "UPDATE main_origins SET enabled=0 WHERE ref=? RETURNING ref",
            )
            .bind(id)
            .fetch_one(conn)
            .await?;
            Ok(json!({"origin":id,"enabled":false}))
        }
        Action::Inspect { execution } => inspect(conn, execution).await,
    }
}

pub async fn inspect(conn: &mut SqliteConnection, id: &str) -> Result<Value> {
    let r=sqlx::query("SELECT x.source,x.origin,x.original_actor,x.state,x.cancelled,t.window,t.conversation,t.result,t.diagnostic,
      b.root,coalesce(o.kind,'human') AS kind FROM executions x JOIN main_turns t ON t.execution=x.id
      JOIN budget_members b ON b.execution=x.id LEFT JOIN main_origins o ON o.ref=x.origin WHERE x.id=?")
        .bind(id).fetch_one(&mut *conn).await?;
    let actor = r
        .try_get::<Option<String>, _>("original_actor")?
        .or(r.try_get("origin")?)
        .ok_or("missing origin")?;
    crate::runtime::effects::authorize(conn, id, &actor, false).await?;
    let activations: Vec<(String, String)> = sqlx::query_as(
        "SELECT id,profile_contract FROM activations WHERE execution=? ORDER BY rowid",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let inputs: Vec<i64> =
        sqlx::query_scalar("SELECT event FROM main_inputs WHERE execution=? ORDER BY event")
            .bind(id)
            .fetch_all(&mut *conn)
            .await?;
    let receipts:Vec<(String,String,Option<String>)>=sqlx::query_as("SELECT a.id,a.state,a.diagnostic FROM model_attempts a JOIN model_requests r ON r.id=a.logical
      JOIN activations v ON v.id=r.activation WHERE v.execution=? ORDER BY r.ordinal,a.ordinal").bind(id).fetch_all(&mut *conn).await?;
    let revision: Option<String> =
        sqlx::query_scalar("SELECT revision FROM execution_tool_views WHERE execution=?")
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?;
    Ok(
        json!({"execution":id,"source":r.try_get::<Option<i64>,_>("source")?,"origin":actor,"kind":r.try_get::<String,_>("kind")?,
      "conversation":r.try_get::<String,_>("conversation")?,"window":r.try_get::<i64,_>("window")?,"state":r.try_get::<String,_>("state")?,
      "cancelled":r.try_get::<bool,_>("cancelled")?,"root":r.try_get::<String,_>("root")?,"attempts":receipts,"activations":activations,"inputs":inputs,
      "result":r.try_get::<Option<String>,_>("result")?.as_deref().map(serde_json::from_str::<Value>).transpose()?,
      "diagnostic":r.try_get::<Option<String>,_>("diagnostic")?,"tool_contract":"main-chat-v1","tool_revision":revision}),
    )
}

pub fn human_text(envelope: &str) -> Result<String> {
    let v: Value = serde_json::from_str(envelope)?;
    Ok(v["text"].as_str().map(str::to_owned).unwrap_or_else(|| {
        v["segments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s["text"].as_str())
            .collect()
    }))
}

pub async fn admit_human(
    conn: &mut SqliteConnection,
    cfg: &Config,
    event: i64,
    now: i64,
) -> Result<()> {
    let row=sqlx::query("SELECT e.entry_id,e.raw_actor,e.envelope,c.scope FROM events e JOIN entries c ON c.id=e.entry_id WHERE e.id=? AND c.closed_at IS NULL")
        .bind(event).fetch_one(&mut *conn).await?;
    let (entry, actor): (String, String) = (row.try_get("entry_id")?, row.try_get("raw_actor")?);
    let text = human_text(row.try_get("envelope")?)?;
    let generation = window(conn, &entry).await?;
    // The ingress claim revalidates original actor and route after any concurrent assignment change.
    let cmd = text.trim().to_ascii_lowercase();
    if matches!(cmd.as_str(), "/ai new" | "/ai help") {
        let proof = admit::claim(conn, event).await?;
        let generation = if cmd == "/ai new" {
            sqlx::query_scalar("UPDATE main_windows SET generation=generation+1 WHERE conversation=? RETURNING generation").bind(&entry).fetch_one(&mut *conn).await?
        } else {
            generation
        };
        sqlx::query("INSERT INTO main_controls(event,command,generation) VALUES(?,?,?)")
            .bind(event)
            .bind(cmd)
            .bind(generation)
            .execute(&mut *conn)
            .await?;
        sqlx::query("UPDATE executions SET state='completed' WHERE id=?")
            .bind(proof["execution"].as_str())
            .execute(conn)
            .await?;
        return Ok(());
    }
    let previous=sqlx::query("SELECT x.id,x.state,x.work_id,t.ready_after,
      EXISTS(SELECT 1 FROM operations WHERE namespace=x.id AND state!='intent') OR EXISTS(SELECT 1 FROM model_requests r JOIN activations a ON a.id=r.activation JOIN model_attempts p ON p.logical=r.id WHERE a.execution=x.id AND r.native_tools) AS crossed
      FROM main_turns t JOIN executions x ON x.id=t.execution WHERE x.original_actor=? AND t.conversation=? AND t.window=?
        AND x.state IN ('pending','running') AND NOT x.cancelled ORDER BY t.rowid DESC LIMIT 1")
        .bind(&actor).bind(&entry).bind(generation).fetch_optional(&mut *conn).await?;
    let mut target = None;
    if let Some(p) = previous {
        let id: String = p.try_get("id")?;
        let state: String = p.try_get("state")?;
        if state == "pending" && now <= p.try_get::<i64, _>("ready_after")? {
            target = Some(id);
        } else if state == "running" && row.try_get::<String, _>("scope")? == "private" {
            if p.try_get::<Option<String>, _>("work_id")?.is_some()
                || p.try_get::<bool, _>("crossed")?
            {
                target = Some(id);
            } else {
                sqlx::query("UPDATE executions SET cancelled=1,state='held' WHERE id=?")
                    .bind(id)
                    .execute(&mut *conn)
                    .await?;
            }
        }
    }
    let target = if let Some(target) = target {
        // A batched event is still authorized independently. No separate execution is minted.
        admit::validate_claim(conn, event).await?;
        target
    } else {
        let claim = admit::claim(conn, event).await?;
        let exec = claim["execution"].as_str().ok_or("missing execution")?;
        sqlx::query(
            "INSERT INTO main_turns(execution,conversation,window,ready_after) VALUES(?,?,?,?)",
        )
        .bind(exec)
        .bind(entry)
        .bind(generation)
        .bind(now + cfg.merge_ms)
        .execute(&mut *conn)
        .await?;
        execution::root(conn, exec, cfg.root_slots, cfg.max_input_units).await?;
        exec.to_string()
    };
    sqlx::query("INSERT INTO main_inputs(event,execution) VALUES(?,?)")
        .bind(event)
        .bind(&target)
        .execute(&mut *conn)
        .await?;
    sqlx::query("INSERT INTO main_dependencies(execution,event) VALUES(?,?)")
        .bind(&target)
        .bind(event)
        .execute(conn)
        .await?;
    Ok(())
}
