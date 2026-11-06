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

pub(crate) async fn physical(
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
    let secret = if let Some(binding) = &cfg.profile_binding {
        let credentials = cfg
            .credentials
            .as_ref()
            .ok_or("original_provider_credential_unavailable")?;
        for (name, value) in &credentials.headers {
            request = request.header(name, value);
        }
        request = request.timeout(std::time::Duration::from_secs_f64(
            binding.profile["timeout_seconds"]
                .as_f64()
                .ok_or("validation_error")?,
        ));
        credentials.key.clone()
    } else {
        cfg.key_env
            .as_ref()
            .map(|key| std::env::var(key).map_err(|_| "provider credential unavailable"))
            .transpose()?
    };
    if let Some(secret) = secret {
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
        if store
            .write(crate::store::Mutation::ToolBudget(
                exec.clone(),
                actor.clone(),
                id.clone(),
            ))
            .await?
            != true
        {
            return Ok(());
        }
        let state: String =
            sqlx::query_scalar("SELECT state FROM operations WHERE namespace=? AND operation_id=?")
                .bind(&exec)
                .bind(&id)
                .fetch_one(store.readers())
                .await?;
        if matches!(state.as_str(), "confirmed" | "failed") {
            return Ok(());
        }
        crate::host::composition::check_runtime(&store, &exec, &id).await?;
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
            "inspect_conversation_attachment" | "workspace_inspect" => {
                crate::media::perform(&store, &cfg, &hub, &exec, &actor, &id).await?;
            }
            "search_memory" => {
                crate::memory::search_tool::perform(&store, &exec, &actor, &id, &t.payload).await?;
            }
            "read_memory" | "memory_change" => {
                crate::memory::tools::perform(&store, &exec, &actor, &id).await?
            }
            "automation_query" | "automation_manage" | "task_control" | "subagent_start"
            | "subagent_control" => {
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
            "web_search" if cfg.bridge_search => {
                crate::model::search::tool(&store, &exec, &actor, &id, &t.payload).await?
            }
            "query_chat_history" => {
                if let Err(error) = crate::history::query::original::run(
                    &store,
                    &exec,
                    &actor,
                    &t.payload,
                    Some(&id),
                )
                .await
                {
                    crate::tools::reject(&store, &exec, &actor, &id, error.to_string()).await?;
                }
            }
            "history_query" => {
                if let Err(error) =
                    crate::history::query::run(&store, &exec, &actor, &t.payload, Some(&id)).await
                {
                    crate::tools::reject(&store, &exec, &actor, &id, error.to_string()).await?;
                }
            }
            "lookup_tools"
            | "time_get_current"
            | "history_directory"
            | "get_group_members"
            | "find_contacts"
            | "find_conversations"
            | "update_short_state"
            | "get_my_capabilities"
            | "read_tool_artifact" => {
                crate::tools::perform(&store, &exec, &actor, &id, &t.target, &t.payload).await?;
            }
            "recall_own_message" => {
                crate::social::recall::perform(&store, &hub, &exec, &actor, &id).await?;
            }
            "send" if cfg.binding_endpoint.is_none() => {
                let delivery: Result<()> = async {
                    if let Some(host) = &store.host
                        && host.deliver(&store, &exec, &actor, &id).await?.is_some()
                    {
                        return Ok(());
                    }

                    crate::media::prepare_host_delivery(&store, &exec, &actor, &t.payload).await?;
                    let result = crate::social::deliver(&store, &hub, &exec, &id, &actor).await?;
                    if !matches!(
                        result["state"].as_str(),
                        Some("confirmed" | "failed" | "unknown")
                    ) {
                        return Err("original social receipt pending".into());
                    }
                    Ok(())
                }
                .await;
                if let Err(err) = delivery {
                    if !err.is::<crate::social::destination::Refused>() {
                        return Err(err);
                    }
                    let receipt =
                        crate::tools::reject_receipt(&store, &exec, &actor, &id, err.to_string())
                            .await?;
                    if receipt["state"] != "failed" {
                        return Err(err);
                    }
                }
            }
            _ => {
                if let Some(host) = &store.host {
                    let delegated = host
                        .tool(
                            &store,
                            &exec,
                            &id,
                            &t.target,
                            serde_json::from_str(&t.payload)?,
                            cfg.vision_input,
                        )
                        .await;
                    match delegated {
                        Ok(Some(receipt)) => {
                            if !matches!(
                                receipt["state"].as_str(),
                                Some("confirmed" | "failed" | "unknown")
                            ) {
                                return Err("original plugin effect pending".into());
                            }
                            return Ok(());
                        }
                        Err(error) => {
                            let state: String = sqlx::query_scalar(
                                "SELECT state FROM operations WHERE namespace=? AND operation_id=?",
                            )
                            .bind(&exec)
                            .bind(&id)
                            .fetch_one(store.readers())
                            .await?;
                            if state == "intent" {
                                return crate::tools::reject(
                                    &store,
                                    &exec,
                                    &actor,
                                    &id,
                                    error.to_string(),
                                )
                                .await;
                            }
                            return Err(error);
                        }
                        Ok(None) => {}
                    }
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
pub(crate) async fn dispatch_child(
    store: &Store,
    exec: &str,
    op: &str,
    hub: &crate::gateway::qq::Hub,
) -> Result<()> {
    let profile: String = sqlx::query_scalar(
        "SELECT w.profile FROM work_runs r JOIN work_nodes w ON w.id=r.work WHERE r.execution=?",
    )
    .bind(exec)
    .fetch_one(store.readers())
    .await?;
    let cfg: Config = serde_json::from_str(&profile)?;
    let actor: String =
        sqlx::query_scalar("SELECT coalesce(original_actor,origin) FROM executions WHERE id=?")
            .bind(exec)
            .fetch_one(store.readers())
            .await?;
    let row = sqlx::query(
        "SELECT action,target,payload FROM operations WHERE namespace=? AND operation_id=?",
    )
    .bind(exec)
    .bind(op)
    .fetch_one(store.readers())
    .await?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    Dispatch {
        store: store.clone(),
        cfg,
        client,
        exec: exec.into(),
        actor,
        hub: hub.clone(),
    }
    .one(Intent {
        ordinal: 0,
        operation_id: op.into(),
        action: row.get("action"),
        target: row.get("target"),
        payload: row.get("payload"),
    })
    .await
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
    #[cfg(test)]
    crate::store::tests::commit_barrier("paired-before-normal-observation").await;
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
        let step = write(
            store,
            cfg,
            Change::Prepare(exec.to_owned(), soul, store.work_segments),
        )
        .await?;
        if step["segment_yielded"] == true {
            #[cfg(test)]
            crate::store::tests::commit_barrier("work-segment-yielded").await;
            return Ok(());
        }
        if step.is_null() {
            return Ok(());
        }
        if step["compact_committed"] == true {
            continue;
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
        if cfg.profile_binding.is_some() && cfg.credentials.is_none() {
            return Err("original_provider_credential_unavailable".into());
        }
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
                                } else if step["compact"] == true
                                    && complete_choice
                                    && !cfg.native_web_search
                                {
                                    "compact_local_response_invalid"
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
                    if step["compact"] == true
                        && (decoded.as_ref().is_ok_and(|turn| {
                            turn.intents.as_array().is_none_or(|v| !v.is_empty())
                                || ["native_events", "citations", "media"].iter().any(|k| {
                                    turn.result[*k].as_array().is_some_and(|v| !v.is_empty())
                                })
                                || !turn.result["grounding"].is_null()
                        }) || parsed["stop_reason"] == "pause_turn")
                    {
                        attempt::invoke(
                            store,
                            Action::Hold {
                                attempt: attempt_id,
                                body: response,
                                usage,
                                diagnostic: if step["private_compact"] == true {
                                    "private_compact_called_tool_or_paused"
                                } else {
                                    "public_compact_called_tool_or_paused"
                                }
                                .into(),
                                partial_bytes: None,
                                unknown: false,
                                status: status.into(),
                            },
                        )
                        .await?;
                        continue;
                    }
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
                    #[cfg(test)]
                    crate::store::tests::commit_barrier("profile-paid-response").await;
                    #[cfg(test)]
                    if step["compact"] == true {
                        crate::store::tests::commit_barrier(if step["private_compact"] == true {
                            "private-response-before-publication"
                        } else {
                            "compact-response-before-publication"
                        })
                        .await;
                    }
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
