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
    if cfg.protocol.is_messages() {
        request = request.header("anthropic-version", "2023-06-01");
    }
    if let Some(key) = &cfg.key_env {
        let secret = std::env::var(key).map_err(|_| "provider credential unavailable")?;
        request = if cfg.protocol.is_messages() {
            request.header("x-api-key", secret)
        } else if cfg.protocol.is_gemini() {
            request.header("x-goog-api-key", secret)
        } else {
            request.bearer_auth(secret)
        };
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

#[derive(Clone)]
struct Intent {
    ordinal: i64,
    operation_id: String,
    action: String,
    target: String,
    payload: String,
}
impl Intent {
    fn name(&self) -> &str {
        if self.action == "external" {
            &self.target
        } else {
            &self.action
        }
    }
}
#[derive(Clone)]
struct Dispatch {
    store: Store,
    cfg: Config,
    client: reqwest::Client,
    exec: String,
    actor: String,
    hub: crate::gateway::qq::Hub,
}
impl Dispatch {
    async fn one(self, t: Intent) -> Result<()> {
        let Self {
            store,
            cfg,
            client,
            exec,
            actor,
            hub,
        } = self;
        let id = t.operation_id;
        if t.action != "external" {
            effects::invoke(
                &store,
                effects::Action::Local {
                    execution: exec,
                    actor,
                    id,
                },
            )
            .await?;
            return Ok(());
        }
        match t.target.as_str() {
            "read_memory" | "memory_change" => {
                crate::memory::tools::perform(&store, &exec, &actor, &id).await?
            }
            "task_control" | "subagent_start" | "subagent_control" => {
                crate::work::perform(&store, &cfg, &exec, &actor, &id).await?;
            }
            "complete"
                if sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(SELECT 1 FROM work_runs WHERE execution=?)",
                )
                .bind(&exec)
                .fetch_one(store.readers())
                .await? =>
            {
                crate::work::perform(&store, &cfg, &exec, &actor, &id).await?;
            }
            "history_query" => {
                if let Err(error) =
                    crate::history::query::run(&store, &exec, &actor, &t.payload, Some(&id)).await
                {
                    crate::tools::reject(&store, &exec, &actor, &id, error.to_string()).await?;
                }
            }
            "lookup_tools" | "time_get_current" | "history_directory" | "update_short_state"
            | "read_tool_artifact" => {
                crate::tools::perform(&store, &exec, &actor, &id, &t.target, &t.payload).await?;
            }
            "send" if cfg.binding_endpoint.is_none() => {
                if let Some(host) = &store.host
                    && host.deliver(&store, &exec, &actor, &id).await?.is_some()
                {
                    return Ok(());
                }

                let result = crate::social::deliver(&store, &hub, &exec, &id, &actor).await?;
                if !matches!(
                    result["state"].as_str(),
                    Some("confirmed" | "failed" | "unknown")
                ) {
                    return Err("original social receipt pending".into());
                }
            }
            _ => {
                if let Some(host) = &store.host
                    && let Some(receipt) = host
                        .tool(
                            &store,
                            &exec,
                            &id,
                            &t.target,
                            serde_json::from_str(&t.payload)?,
                        )
                        .await?
                {
                    if !matches!(
                        receipt["state"].as_str(),
                        Some("confirmed" | "failed" | "unknown")
                    ) {
                        return Err("original plugin effect pending".into());
                    }
                    return Ok(());
                }
                let endpoint = cfg
                    .binding_endpoint
                    .as_ref()
                    .ok_or("external binding unavailable")?;
                let grant = effects::invoke(
                    &store,
                    effects::Action::Dispatch {
                        execution: exec.clone(),
                        id: id.clone(),
                        actor,
                    },
                )
                .await?;
                if grant["dispatch_now"] == true {
                    let call = json!({"execution":exec,"operation":id,"tool":t.target,"arguments":serde_json::from_str::<Value>(&t.payload)?});
                    let response = client
                        .post(endpoint)
                        .header("Content-Type", "application/json")
                        .body(call.to_string())
                        .send()
                        .await;
                    let data = match response {
                        Ok(r) if r.status().is_success() => r
                            .text()
                            .await
                            .ok()
                            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok()),
                        _ => None,
                    };
                    effects::invoke(
                        &store,
                        effects::Action::Settle {
                            execution: exec,
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
        }
        Ok(())
    }
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
    let intents=sqlx::query("SELECT i.ordinal,i.operation_id,o.action,o.target,o.payload FROM model_tool_intents i JOIN operations o ON o.namespace=i.execution AND o.operation_id=i.operation_id WHERE i.logical=? AND i.paired_receipt IS NULL ORDER BY i.ordinal")
        .bind(logical).try_map(|r:sqlx::sqlite::SqliteRow|Ok(Intent{ordinal:r.try_get("ordinal")?,operation_id:r.try_get("operation_id")?,action:r.try_get("action")?,target:r.try_get("target")?,payload:r.try_get("payload")?})).fetch_all(store.readers()).await?;
    let frozen:Option<String>=sqlx::query_scalar("SELECT m.body FROM tool_manifests m JOIN execution_tool_views v USING(revision) WHERE v.execution=?").bind(exec).fetch_optional(store.readers()).await?;
    // Prior activations without a frozen execution revision retain their serial order.
    let frozen = frozen
        .as_deref()
        .map(serde_json::from_str::<crate::tools::registry::Manifest>)
        .transpose()?;
    let safe = |t: &Intent| {
        frozen
            .as_ref()
            .and_then(|m| m.0.get(t.name()))
            .is_some_and(|entry| entry.parallel_read)
    };
    let dispatch = Dispatch {
        store: store.clone(),
        cfg: cfg.clone(),
        client: client.clone(),
        exec: exec.into(),
        actor: actor.into(),
        hub: hub.clone(),
    };
    let mut start = 0;
    while start < intents.len() {
        let end = if safe(&intents[start]) {
            start + intents[start..].iter().take_while(|t| safe(t)).count()
        } else {
            start + 1
        };
        crate::tools::batch::execute(&intents[start..end], |intent| dispatch.clone().one(intent))
            .await?;
        #[cfg(test)]
        if intents[start..end].iter().any(|t| t.name() == "send") {
            crate::store::tests::commit_barrier("delivered-before-pair").await;
        }
        for t in &intents[start..end] {
            attempt::invoke(
                store,
                Action::Pair {
                    logical: logical.into(),
                    ordinal: t.ordinal,
                },
            )
            .await?;
        }
        start = end;
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
                                    if cfg.protocol.is_messages() {
                                        "incomplete_messages_stream"
                                    } else if cfg.protocol.is_gemini() {
                                        "incomplete_generate_content_stream"
                                    } else {
                                        "incomplete_responses_stream"
                                    }
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
