use super::Config;
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
    policy: &super::segment::Config,
    audio: Option<&std::collections::BTreeSet<i64>>,
) -> Result<Value> {
    crate::model::profile_store::runtime::pin(conn, exec, cfg).await?;
    let actor = execution::eligible(conn, exec).await?;
    let original_soul = crate::operator::domain::original_soul(conn, exec, soul).await?;
    let soul = original_soul.as_str();
    let turn=sqlx::query("SELECT t.material,t.conversation,t.window,t.rowid,t.origin,x.state FROM main_turns t JOIN executions x ON x.id=t.execution WHERE x.id=?")
        .bind(exec).fetch_one(&mut *conn).await?;
    if !matches!(
        turn.try_get::<String, _>("state")?.as_str(),
        "pending" | "running"
    ) {
        return Ok(Value::Null);
    }
    if let Some(prepared) = audio {
        let missing: Vec<i64> = sqlx::query_scalar(super::audio::MISSING)
            .bind(exec)
            .fetch_all(&mut *conn)
            .await?;
        if missing.iter().any(|id| !prepared.contains(id)) {
            return Ok(json!({"audio_input_changed":true}));
        }
    }
    if crate::social::model_receipt::stale(conn, exec, &actor).await? {
        return hold(conn, exec, crate::social::model_receipt::CHANGED).await;
    }
    super::segment::begin(conn, exec, policy).await?;
    if let Some(next) =
        crate::context::compact_public::job::recover(conn, cfg, exec, &actor).await?
    {
        return Ok(next);
    }
    if let Some(next) = crate::context::compact_exec::recover(conn, cfg, exec, &actor).await? {
        return Ok(next);
    }
    if crate::context::compact_public::history::stale(conn, exec).await? {
        return hold(conn, exec, "public_source_changed").await;
    }
    let activation = format!("main:{exec}");
    execution::activate(conn, exec, &activation, &cfg.contract()).await?;
    let prior=sqlx::query("SELECT r.id,r.ordinal,r.request_json,p.state,p.response,p.diagnostic
      FROM model_requests r LEFT JOIN model_attempts p ON p.logical=r.id WHERE r.activation=? ORDER BY r.ordinal DESC,p.ordinal DESC LIMIT 1")
        .bind(&activation).fetch_optional(&mut *conn).await?;
    let fresh: Vec<i64> = sqlx::query_scalar(
        "SELECT event FROM main_inputs WHERE execution=? AND NOT consumed ORDER BY event",
    )
    .bind(exec)
    .fetch_all(&mut *conn)
    .await?;
    let mut material = match turn.try_get::<Option<String>, _>("material")? {
        Some(raw) => serde_json::from_str::<Input>(&raw)?,
        None => Input::default(),
    };
    let zone = cfg.timezone.as_deref().unwrap_or("Asia/Shanghai");
    if !fresh.is_empty() {
        material.text = crate::context::event::trigger(conn, &fresh, zone, audio).await?;
        sqlx::query("INSERT OR IGNORE INTO exec_public_tails(execution,first_event,last_event,message) VALUES(?,?,?,?)")
            .bind(exec).bind(fresh[0]).bind(fresh[fresh.len()-1]).bind(&material.text).execute(&mut *conn).await?;
    }
    let mut index = 0;
    let mut original_logical = None;
    let mut body;
    if let Some(row) = prior {
        let logical: String = row.try_get("id")?;
        original_logical = Some(logical.clone());
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
        let fresh_mail = crate::work::has_mail(conn, exec).await?;
        let mut continue_turn = !fresh.is_empty() || fresh_mail;
        match result {
            Ok(parsed) => {
                let original: Value = serde_json::from_str(&raw)?;
                cfg.protocol.replay(
                    &mut body,
                    &crate::model::profile_store::wire::replay(cfg, &original),
                )?;
                continue_turn |=
                    cfg.protocol.is_messages() && original["stop_reason"] == "pause_turn";
                if !parsed.intents.as_array().unwrap().is_empty() {
                    let tools=sqlx::query("SELECT call_id,operation_id,paired_receipt FROM model_tool_intents WHERE logical=? ORDER BY ordinal")
                        .bind(&logical).fetch_all(&mut *conn).await?;
                    if tools.iter().any(|t| {
                        t.try_get::<Option<String>, _>("paired_receipt")
                            .ok()
                            .flatten()
                            .is_none()
                    }) {
                        return Ok(json!({"pair":logical,"actor":actor}));
                    }
                    for t in &tools {
                        cfg.protocol.output(
                            &mut body,
                            t.try_get("call_id")?,
                            crate::host::selected_processor::receipt(
                                conn,
                                exec,
                                t.try_get("operation_id")?,
                                t.try_get("paired_receipt")?,
                            )
                            .await?
                            .as_ref(),
                        )?;
                    }
                    for t in tools {
                        crate::media::append_image(
                            conn,
                            cfg,
                            exec,
                            t.try_get("operation_id")?,
                            &mut body,
                        )
                        .await?;
                    }
                    if crate::work::wait::settle(conn, exec, &logical).await? {
                        return Ok(Value::Null);
                    }
                    if !continue_turn && crate::work::settle(conn, exec, &logical, None).await? {
                        return Ok(Value::Null);
                    }
                    continue_turn = true;
                }
                if !continue_turn {
                    if crate::work::settle(conn, exec, &logical, Some(&parsed.result)).await? {
                        return Ok(Value::Null);
                    }
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
        if let Some(yielded) = super::segment::exhausted(conn, exec, policy).await? {
            return Ok(yielded);
        }
        if crate::work::bound(conn, exec).await?.is_some() {
            let mut state = crate::work::context(conn, exec).await?;
            let inbox = crate::work::private_input(conn, exec).await?;
            if !inbox.is_empty() {
                state["private_inbox"] = json!(inbox);
            }
            cfg.protocol.user(
                &mut body,
                &format!("\n\n<runtime_context>\n{state}\n</runtime_context>\n\n"),
            )?;
        }
        if !fresh.is_empty() {
            cfg.protocol.user(&mut body, &material.text)?;
        }
    } else {
        let history = crate::context::compact_public::history::prepare(
            conn,
            exec,
            &actor,
            &assemble::static_program(cfg, soul),
            zone,
        )
        .await?;
        if let Some(origin) = turn.try_get::<Option<String>, _>("origin")? {
            let kind: String = sqlx::query_scalar("SELECT kind FROM main_origins WHERE ref=?")
                .bind(&origin)
                .fetch_one(&mut *conn)
                .await?;
            material
                .context
                .insert(0, json!({"source_kind":kind,"source_ref":origin}));
        }
        let mut work = crate::work::context(conn, exec).await?;
        let inbox = crate::work::private_input(conn, exec).await?;
        if !inbox.is_empty() {
            work["private_inbox"] = json!(inbox);
        }
        material.context.push(work);
        let (manifest, code) = crate::tools::registry::pinned(conn, exec, cfg).await?;
        let slots =
            crate::tools::transient::live(conn, exec, crate::tools::transient::now()?).await?;
        if !slots.is_empty() {
            material.context.push(json!({"short_state":slots}));
        }
        let layout = assemble::body(cfg, soul, &material, history, manifest.declarations(code));
        body = cfg
            .protocol
            .request(layout, cfg, material.structured.as_ref())?;
    }
    if cfg.protocol.is_messages() {
        crate::model::messages::cache_last(&mut body)?;
    }
    if let Some(logical) = original_logical.as_deref() {
        crate::context::compact_exec::adopt(conn, cfg, exec, logical, &mut body).await?;
    }
    loop {
        match crate::context::compact_public::job::consider(
            conn,
            cfg,
            exec,
            &actor,
            &mut body,
            material.structured.is_some(),
        )
        .await
        {
            Ok(Some(next)) => return Ok(next),
            Ok(None) => {}
            Err(e) if e.to_string() == "model_request_capacity" => {}
            Err(e) => return Err(e),
        }
        // A stored E may have been purchased after P changed the initial native
        // prefix. Reapply it to that exact prefix, then check if retiring the
        // private chain made another original public route legal.
        if let Some(logical) = original_logical.as_deref()
            && crate::context::compact_exec::adopt(conn, cfg, exec, logical, &mut body).await?
        {
            continue;
        }
        break;
    }
    match crate::context::compact_exec::consider(
        conn,
        cfg,
        exec,
        &actor,
        &body,
        material.structured.is_some(),
    )
    .await
    {
        Ok(Some(next)) => return Ok(next),
        Ok(None) => {}
        Err(e) if e.to_string() == "model_request_capacity" => {
            return hold(conn, exec, "model_request_capacity").await;
        }
        Err(e) => return Err(e),
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
            native_tools: body["tools"].as_array().is_some_and(|items| {
                items.iter().any(|t| {
                    t["googleSearch"].is_object()
                        || matches!(
                            t["type"].as_str(),
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
