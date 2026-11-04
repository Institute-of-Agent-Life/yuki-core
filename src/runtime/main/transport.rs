use super::{Change, Config, write};
use crate::{
    Result,
    model::{
        attempt::{self, Action},
        chat::Task,
    },
    runtime::effects::{self, Outcome},
    store::Store,
};
use serde_json::{Value, json};
use sqlx::Row;
use std::{path::Path, sync::Arc};
use tokio::sync::watch;

async fn physical(
    client: &reqwest::Client,
    cfg: &Config,
    raw: &str,
) -> Result<(u16, Vec<u8>, bool)> {
    let mut request = client
        .post(&cfg.endpoint)
        .header("Content-Type", "application/json")
        .body(raw.to_owned());
    if let Some(key) = &cfg.key_env {
        request =
            request.bearer_auth(std::env::var(key).map_err(|_| "provider credential unavailable")?);
    }
    let mut response = request.send().await?;
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    let complete = loop {
        match response.chunk().await {
            Ok(Some(part)) if bytes.len() + part.len() <= 8 * 1024 * 1024 => bytes.extend(part),
            Ok(Some(part)) => {
                let remaining = 8 * 1024 * 1024 - bytes.len();
                bytes.extend_from_slice(&part[..remaining]);
                break false;
            }
            Ok(None) => break true,
            _ => break false,
        }
    };
    let utf8 = std::str::from_utf8(&bytes).is_ok();
    Ok((status, bytes, complete && utf8))
}

async fn pair(
    store: &Store,
    cfg: &Config,
    client: &reqwest::Client,
    exec: &str,
    logical: &str,
    actor: &str,
    hub: &crate::gateway::qq::Hub,
) -> Result<()> {
    let tools=sqlx::query("SELECT i.ordinal,i.operation_id,o.action,o.target,o.payload FROM model_tool_intents i JOIN operations o
        ON o.namespace=i.execution AND o.operation_id=i.operation_id WHERE i.logical=? AND i.paired_receipt IS NULL ORDER BY i.ordinal")
        .bind(logical).fetch_all(store.readers()).await?;
    for t in tools {
        if t.try_get::<String, _>("action")? == "external" {
            if t.try_get::<String, _>("target")? == "history_query" {
                crate::history::query::run(
                    store,
                    exec,
                    actor,
                    t.try_get("payload")?,
                    Some(t.try_get("operation_id")?),
                )
                .await?;
                attempt::invoke(
                    store,
                    Action::Pair {
                        logical: logical.into(),
                        ordinal: t.try_get("ordinal")?,
                    },
                )
                .await?;
                continue;
            }
            if cfg.binding_endpoint.is_none() && t.try_get::<String, _>("target")? == "send" {
                let result =
                    crate::social::deliver(store, hub, exec, t.try_get("operation_id")?, actor)
                        .await?;
                if !matches!(
                    result["state"].as_str(),
                    Some("confirmed" | "failed" | "unknown")
                ) {
                    return Err("original social receipt pending".into());
                }
                attempt::invoke(
                    store,
                    Action::Pair {
                        logical: logical.into(),
                        ordinal: t.try_get("ordinal")?,
                    },
                )
                .await?;
                continue;
            }
            let endpoint = cfg
                .binding_endpoint
                .as_ref()
                .ok_or("external binding unavailable")?;
            let id: String = t.try_get("operation_id")?;
            let permit = effects::invoke(
                store,
                effects::Action::Dispatch {
                    execution: exec.into(),
                    id: id.clone(),
                    actor: actor.into(),
                },
            )
            .await?;
            if permit["dispatch_now"] == true {
                let call = json!({"execution":exec,"operation":id,"tool":t.try_get::<String,_>("target")?,"arguments":serde_json::from_str::<Value>(t.try_get("payload")?)?});
                let result = client
                    .post(endpoint)
                    .header("Content-Type", "application/json")
                    .body(call.to_string())
                    .send()
                    .await;
                let data = match result {
                    Ok(r) if r.status().is_success() => r
                        .text()
                        .await
                        .ok()
                        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok()),
                    _ => None,
                };
                effects::invoke(
                    store,
                    effects::Action::Settle {
                        execution: exec.into(),
                        id,
                        outcome: if data.is_some() {
                            Outcome::Confirmed
                        } else {
                            Outcome::Unknown
                        },
                        result: data.unwrap_or(Value::Null).to_string(),
                    },
                )
                .await?;
            }
        }
        attempt::invoke(
            store,
            Action::Pair {
                logical: logical.into(),
                ordinal: t.try_get("ordinal")?,
            },
        )
        .await?;
    }
    Ok(())
}

pub async fn run(
    store: &Store,
    cfg: &Arc<Config>,
    client: &reqwest::Client,
    data: &Path,
    exec: &str,
    mut cancel: watch::Receiver<bool>,
    hub: &crate::gateway::qq::Hub,
) -> Result<()> {
    loop {
        if *cancel.borrow() {
            return Ok(());
        }
        let soul = std::fs::read_to_string(data.join("SOUL.md"))?;
        let step = write(store, cfg, Change::Prepare(exec.to_owned(), soul)).await?;
        if step.is_null() {
            return Ok(());
        }
        if let Some(logical) = step["pair"].as_str() {
            pair(
                store,
                cfg,
                client,
                exec,
                logical,
                step["actor"].as_str().ok_or("missing original actor")?,
                hub,
            )
            .await?;
            continue;
        }
        let logical = step["logical"].as_str().ok_or("missing logical request")?;
        let task: Option<Task> = serde_json::from_value(step["task"].clone())?;
        let attempt_id = format!("{logical}:http:0");
        let permit = attempt::invoke(
            store,
            Action::Dispatch {
                logical: logical.into(),
                attempt: attempt_id.clone(),
            },
        )
        .await?;
        if permit["dispatch_now"] != true {
            return Err("original model request already dispatched".into());
        }
        let raw = permit["request_raw"]
            .as_str()
            .ok_or("missing paid request bytes")?;
        let physical = tokio::select! {
            _=cancel.changed()=>Err("original foreground cancelled".into()),
            result=physical(client,cfg,raw)=>result,
        };
        if let Ok((status, bytes, complete)) = physical {
            let response = String::from_utf8_lossy(&bytes).to_string();
            let usage = cfg.protocol.usage(&response).to_string();
            if !complete {
                attempt::invoke(
                    store,
                    Action::Hold {
                        attempt: attempt_id,
                        body: response,
                        usage,
                        diagnostic: "body_interrupted".into(),
                        partial_bytes: Some(bytes),
                        unknown: true,
                        status: i64::from(status),
                    },
                )
                .await?;
            } else if (400..600).contains(&status) {
                attempt::invoke(
                    store,
                    Action::Rejected {
                        attempt: attempt_id,
                        status: status.into(),
                        unbilled_before_model: false,
                        body: response,
                        usage,
                    },
                )
                .await?;
            } else {
                let parsed = serde_json::from_str::<Value>(&response).unwrap_or(Value::Null);
                let original: Value = serde_json::from_str(raw)?;
                let decoded = cfg
                    .protocol
                    .decode(&response, task.as_ref(), cfg, &original);
                let complete_choice = status == 200 && cfg.protocol.complete(&parsed);
                if !complete_choice || decoded.is_err() && task.is_none() {
                    attempt::invoke(
                        store,
                        Action::Hold {
                            attempt: attempt_id,
                            body: response.clone(),
                            usage,
                            diagnostic:
                                if !cfg.protocol.is_chat() && response.starts_with("event:") {
                                    "incomplete_responses_stream"
                                } else {
                                    "invalid_or_incomplete_choice"
                                }
                                .into(),
                            partial_bytes: (!cfg.protocol.is_chat()
                                && response.starts_with("event:"))
                            .then_some(bytes),
                            unknown: false,
                            status: status.into(),
                        },
                    )
                    .await?;
                } else {
                    let intents = decoded
                        .map(|d| d.intents)
                        .unwrap_or_else(|_| json!([]))
                        .to_string();
                    attempt::invoke(
                        store,
                        Action::Response {
                            attempt: attempt_id,
                            body: response,
                            usage,
                            intents,
                        },
                    )
                    .await?;
                }
            }
        } else {
            attempt::invoke(
                store,
                Action::Unknown {
                    attempt: attempt_id,
                },
            )
            .await?;
        }
    }
}
