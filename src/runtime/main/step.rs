use super::{Config, domain::human_text};
use crate::{
    Result,
    context::assemble,
    model::{attempt, chat::Input},
    runtime::execution,
};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub async fn prepare(
    conn: &mut SqliteConnection,
    cfg: &Config,
    exec: &str,
    soul: &str,
) -> Result<Value> {
    let actor = execution::eligible(conn, exec).await?;
    let turn=sqlx::query("SELECT t.material,t.conversation,t.window,t.rowid,t.origin,x.state FROM main_turns t JOIN executions x ON x.id=t.execution WHERE x.id=?")
        .bind(exec).fetch_one(&mut *conn).await?;
    if !matches!(
        turn.try_get::<String, _>("state")?.as_str(),
        "pending" | "running"
    ) {
        return Ok(Value::Null);
    }
    let activation = format!("main:{exec}");
    execution::activate(conn, exec, &activation, &cfg.contract()).await?;
    let prior=sqlx::query("SELECT r.id,r.ordinal,r.request_json,p.state,p.response,p.diagnostic
      FROM model_requests r LEFT JOIN model_attempts p ON p.logical=r.id WHERE r.activation=? ORDER BY r.ordinal DESC,p.ordinal DESC LIMIT 1")
        .bind(&activation).fetch_optional(&mut *conn).await?;
    let event_rows = sqlx::query(
        "SELECT e.id,e.envelope,i.consumed FROM main_inputs i JOIN events e ON e.id=i.event
      WHERE i.execution=? ORDER BY e.id",
    )
    .bind(exec)
    .fetch_all(&mut *conn)
    .await?;
    let mut material = match turn.try_get::<Option<String>, _>("material")? {
        Some(raw) => serde_json::from_str::<Input>(&raw)?,
        None => Input::default(),
    };
    let fresh: Vec<String> = event_rows
        .iter()
        .filter(|r| r.try_get::<bool, _>("consumed").ok() == Some(false))
        .map(|r| human_text(r.try_get("envelope")?))
        .collect::<Result<_>>()?;
    if !fresh.is_empty() {
        material.text = fresh.join("\n");
    }
    let mut index = 0;
    let mut body;
    if let Some(row) = prior {
        let logical: String = row.try_get("id")?;
        let state: Option<String> = row.try_get("state")?;
        if state.is_none() {
            return Ok(json!({"logical":logical,"actor":actor,"task":material.structured}));
        }
        if matches!(
            state.as_deref(),
            Some("unknown" | "response_held" | "rejected" | "dispatched")
        ) {
            hold(conn, exec, state.as_deref().unwrap()).await?;
            return Ok(Value::Null);
        }
        let raw: String = row.try_get("response")?;
        let task = material.structured.as_ref();
        index = row.try_get::<i64, _>("ordinal")? + 1;
        body = serde_json::from_str::<Value>(row.try_get("request_json")?)?;
        let result = cfg.protocol.decode(&raw, task, cfg, &body);
        let mut continue_turn = !fresh.is_empty();
        match result {
            Ok(parsed) => {
                let original: Value = serde_json::from_str(&raw)?;
                cfg.protocol.replay(&mut body, &original)?;
                continue_turn |=
                    cfg.protocol.is_messages() && original["stop_reason"] == "pause_turn";
                if !parsed.intents.as_array().unwrap().is_empty() {
                    let tools=sqlx::query("SELECT call_id,paired_receipt FROM model_tool_intents WHERE logical=? ORDER BY ordinal")
                        .bind(&logical).fetch_all(&mut *conn).await?;
                    if tools.iter().any(|t| {
                        t.try_get::<Option<String>, _>("paired_receipt")
                            .ok()
                            .flatten()
                            .is_none()
                    }) {
                        return Ok(json!({"pair":logical,"actor":actor}));
                    }
                    for t in tools {
                        cfg.protocol.output(
                            &mut body,
                            t.try_get("call_id")?,
                            t.try_get("paired_receipt")?,
                        )?;
                    }
                    continue_turn = true;
                }
                if !continue_turn {
                    sqlx::query("UPDATE main_turns SET result=? WHERE execution=?")
                        .bind(parsed.result.to_string())
                        .bind(exec)
                        .execute(&mut *conn)
                        .await?;
                    sqlx::query("UPDATE executions SET state='completed' WHERE id=?")
                        .bind(exec)
                        .execute(conn)
                        .await?;
                    return Ok(Value::Null);
                }
            }
            Err(err) => {
                if task.is_none_or(|task| index > i64::from(task.validation_retries)) {
                    hold(conn, exec, "validation_failed").await?;
                    return Ok(Value::Null);
                }
                cfg.protocol
                    .user(&mut body, &format!("Correct the structured result: {err}"))?;
            }
        }
        if !fresh.is_empty() {
            cfg.protocol.user(&mut body, &material.text)?;
        }
    } else {
        let mut history = Vec::new();
        if turn.try_get::<Option<String>, _>("material")?.is_none() {
            let rows=sqlx::query("SELECT e.id,e.envelope FROM main_inputs i JOIN events e ON e.id=i.event JOIN main_turns t ON t.execution=i.execution JOIN executions x ON x.id=t.execution
              WHERE t.conversation=? AND t.window=? AND t.rowid<? AND e.raw_actor=? AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id) ORDER BY e.id")
                .bind(turn.try_get::<String,_>("conversation")?).bind(turn.try_get::<i64,_>("window")?).bind(turn.try_get::<i64,_>("rowid")?).bind(&actor).fetch_all(&mut *conn).await?;
            for r in rows {
                sqlx::query("INSERT OR IGNORE INTO main_dependencies(execution,event) VALUES(?,?)")
                    .bind(exec)
                    .bind(r.try_get::<i64, _>("id")?)
                    .execute(&mut *conn)
                    .await?;
                history.push(json!({"role":"user","content":human_text(r.try_get("envelope")?)?}));
            }
        }
        if let Some(origin) = turn.try_get::<Option<String>, _>("origin")? {
            let kind: String = sqlx::query_scalar("SELECT kind FROM main_origins WHERE ref=?")
                .bind(&origin)
                .fetch_one(&mut *conn)
                .await?;
            material
                .context
                .insert(0, json!({"source_kind":kind,"source_ref":origin}));
        }
        body = cfg.protocol.request(
            assemble::body(cfg, soul, &material, history),
            cfg,
            material.structured.as_ref(),
        )?;
    }
    if cfg.protocol.is_messages() {
        crate::model::messages::cache_last(&mut body)?;
    }
    let raw = body.to_string();
    let logical = format!("{exec}:logical:{index}");
    let estimated = i64::try_from(raw.len())?;
    attempt::apply(
        conn,
        &attempt::Action::Reserve(attempt::Reservation {
            activation,
            logical: logical.clone(),
            input: estimated,
            output: cfg.max_output,
            safe_rate_limit_once: false,
            native_tools: body["tools"].as_array().is_some_and(|tools| {
                tools.iter().any(|tool| {
                    matches!(
                        tool["type"].as_str(),
                        Some("web_search" | "web_search_20250305")
                    )
                })
            }),
            request: raw,
        }),
    )
    .await?;
    sqlx::query("UPDATE main_inputs SET consumed=1 WHERE execution=?")
        .bind(exec)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE executions SET state='running' WHERE id=?")
        .bind(exec)
        .execute(conn)
        .await?;
    Ok(json!({"logical":logical,"actor":actor,"task":material.structured}))
}

pub async fn hold(conn: &mut SqliteConnection, exec: &str, diagnostic: &str) -> Result<Value> {
    sqlx::query("UPDATE executions SET state='held' WHERE id=? AND state IN ('pending','running')")
        .bind(exec)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE main_turns SET diagnostic=? WHERE execution=? AND EXISTS(SELECT 1 FROM executions x WHERE x.id=? AND x.state='held')")
        .bind(diagnostic).bind(exec).bind(exec).execute(conn).await?;
    Ok(Value::Null)
}
