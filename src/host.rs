mod aux_model;
pub(crate) mod calls;
mod commands;
pub(crate) mod composition;
pub(crate) mod domain;
mod grants;
pub(crate) mod maintenance;
mod media;
pub(crate) mod participation;
pub(crate) mod personal;
pub(crate) mod plugin_sessions;
mod process;
mod send;
use crate::{
    Result,
    store::{Mutation, Store},
};
pub use grants::Install;
use serde_json::{Value, json};
use sqlx::Row;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

pub enum Change {
    AuxModel(Box<aux_model::Change>),
    Participation(Box<participation::Change>),
    Maintenance(Box<maintenance::Change>),
    PluginSession(Box<plugin_sessions::Change>),
    Configure(Vec<Install>),
    Start(String, bool),
    Online(String, String),
    Close(String, String),
    Enable(String, bool),
    Callback(String, String, String, Value),
    Call(calls::Change),
    Composition(Box<composition::Change>),
    CommandDone(i64, String),
    Reported(i64, String),
}
pub(crate) async fn write(store: &Store, change: Change) -> Result<Value> {
    store.write(Mutation::Host(Box::new(change))).await
}
struct Slot {
    spec: Install,
    session: Mutex<Option<process::Session>>,
}
pub struct Hub {
    pub(crate) qq: crate::gateway::qq::Hub,
    slots: BTreeMap<String, Slot>,
    data: PathBuf,
    shutdown: tokio::sync::watch::Sender<bool>,
}
#[derive(clap::Subcommand, serde::Deserialize, serde::Serialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Status,
    Process {
        plugin: String,
        execution: String,
        actor: String,
        operation: String,
        /// Exact event/slot or immutable artifact ID and digest.
        source: String,
        purpose: String,
    },
    Restart {
        id: String,
    },
    Enable {
        id: String,
        #[arg(action=clap::ArgAction::Set)]
        enabled: bool,
    },
}

impl Hub {
    pub async fn start(
        store: &mut Store,
        installs: Vec<Install>,
        data: &Path,
        qq: crate::gateway::qq::Hub,
    ) -> Result<()> {
        grants::validate(&installs)?;
        if installs.is_empty()
            && !sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM host_installations)")
                .fetch_one(store.readers())
                .await?
        {
            return Ok(());
        }
        write(store, Change::Configure(installs.clone())).await?;
        if installs.is_empty() {
            return Ok(());
        }
        let slots = installs
            .into_iter()
            .map(|spec| {
                (
                    spec.id.clone(),
                    Slot {
                        spec,
                        session: Mutex::new(None),
                    },
                )
            })
            .collect();
        let (shutdown, end) = tokio::sync::watch::channel(false);
        let hub = Arc::new(Self {
            qq: qq.clone(),
            shutdown,
            slots,
            data: data.into(),
        });
        store.host = Some(hub.clone());
        tokio::spawn(commands::serve(hub.clone(), store.clone(), qq, end));
        for (id, slot) in &hub.slots {
            if slot.spec.composition.is_some() {
                let started = hub.restart(store, id).await;
                if store.main.as_ref().is_some_and(|cfg| {
                    cfg.code_tool.as_deref()
                        == slot.spec.composition.as_ref().map(|c| c.tool.as_str())
                }) {
                    started?;
                }
                continue;
            }
            let (h, s, id) = (hub.clone(), store.clone(), id.clone());
            tokio::spawn(async move {
                let _ = h.restart(&s, &id).await;
            });
        }
        Ok(())
    }
    async fn restart(&self, store: &Store, id: &str) -> Result<Value> {
        let slot = self.slots.get(id).ok_or("plugin not installed")?;
        let mut guard = slot.session.lock().await;
        if let Some(old) = guard.take() {
            old.stop();
            old.wait_closed().await;
            write(store, Change::Close(id.into(), old.generation)).await?;
        }
        let pending = pending_erasure(store, id).await?;
        if !pending.is_empty() {
            if slot.spec.erasure_args.is_none() {
                return Err("erasure pending; no cleanup-only entry installed".into());
            }
            let cleanup = start_session(store, &slot.spec, &self.data, true).await?;
            let res = async {
                for ob in pending {
                    acknowledge(store, id, &cleanup, &ob).await?;
                }
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            }
            .await;
            cleanup.stop();
            cleanup.wait_closed().await;
            write(store, Change::Close(id.into(), cleanup.generation.clone())).await?;
            res?;
        }
        let session = start_session(store, &slot.spec, &self.data, false).await?;
        *guard = Some(session.clone());
        let result = json!({"plugin":id,"generation":session.generation,"online":true});
        let (s, i) = (store.clone(), slot.spec.clone());
        tokio::spawn(maintain(s, i, session));
        Ok(result)
    }
    pub async fn stop(&self) {
        self.shutdown.send_replace(true);
        for slot in self.slots.values() {
            if let Some(s) = slot.session.lock().await.take() {
                s.stop();
                s.wait_closed().await;
            }
        }
    }
    pub async fn operator(&self, store: &Store, action: Action) -> Result<Value> {
        match action {
            Action::Process {
                plugin,
                execution,
                actor,
                operation,
                source,
                purpose,
            } => {
                let mut tx = store.readers().begin().await?;
                crate::runtime::effects::authorize(&mut tx, &execution, &actor, true).await?;
                tx.commit().await?;
                let spec = &self
                    .slots
                    .get(&plugin)
                    .ok_or("selected processor unavailable")?
                    .spec;
                self.execute(store,calls::Request{
 vision:false,plugin:plugin.clone(),revision:spec.revision()?,execution,operation,method:"media.process".into(),name:"media.process".into(),args:json!({"media":serde_json::from_str::<Value>(&source)?,"purpose":purpose}),parent:None,ordinal:None}).await
            }
            Action::Status => {
                let rows=sqlx::query("SELECT plugin,namespace,revision,enabled,generation,online FROM host_installations ORDER BY plugin").fetch_all(store.readers()).await?;
                let items=rows.iter().map(|r|Ok(json!({"plugin":r.try_get::<String,_>("plugin")?,"namespace":r.try_get::<String,_>("namespace")?,"enabled":r.try_get::<bool,_>("enabled")?,"online":r.try_get::<bool,_>("online")?,"generation":r.try_get::<Option<String>,_>("generation")?,"revision":r.try_get::<String,_>("revision")?}))).collect::<Result<Vec<_>>>()?;
                Ok(json!({"plugins":items,"host_schema_revision":2}))
            }
            Action::Restart { id } => self.restart(store, &id).await,
            Action::Enable { id, enabled } => {
                let result = write(store, Change::Enable(id.clone(), enabled)).await?;
                if enabled {
                    self.restart(store, &id).await?;
                } else if let Some(slot) = self.slots.get(&id)
                    && let Some(session) = slot.session.lock().await.take()
                {
                    session.stop();
                }
                Ok(result)
            }
        }
    }
    async fn execute(&self, store: &Store, r: calls::Request) -> Result<Value> {
        let opened = write(store, Change::Call(calls::Change::Open(r.clone()))).await?;
        let handle = opened["handle"]
            .as_str()
            .ok_or("missing bound handle")?
            .to_owned();
        let slot = self.slots.get(&r.plugin).ok_or("plugin not installed")?;
        let session = slot.session.lock().await.clone();
        let (method,params,generation)=if opened["send"] == true {
            (r.method.clone(),opened["params"].clone(),opened["generation"].as_str().ok_or("missing original generation")?.to_owned())
        } else if opened["state"]=="unknown" && sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM code_parents WHERE handle=? AND NOT closed AND snapshot IS NOT NULL)").bind(&handle).fetch_one(store.readers()).await? {
            let generation=session.as_ref().ok_or("original Code runtime offline")?.generation.clone();
            let params=write(store,Change::Composition(Box::new(composition::Change::Rebind(handle.clone(),generation.clone())))).await?;
            ("code.resume".into(),params,generation)
        } else { return Ok(opened); };
        let Some(session) = session.filter(|s| s.generation == generation) else {
            write(
                store,
                Change::Call(calls::Change::Unsent { handle, generation }),
            )
            .await?;
            return Err("original plugin generation unavailable".into());
        };
        let result = session
            .call(store, &method, params, Some(handle.clone()))
            .await;
        if result.is_err() && session.stopped() {
            write(
                store,
                Change::Close(r.plugin.clone(), generation.to_owned()),
            )
            .await?;
            let state: String = sqlx::query_scalar("SELECT state FROM host_calls WHERE handle=?")
                .bind(&handle)
                .fetch_one(store.readers())
                .await?;
            if state == "unknown" {
                return Ok(json!({"handle":handle,"state":state}));
            }
        }
        result
    }
    pub async fn tool(
        &self,
        store: &Store,
        exec: &str,
        operation: &str,
        name: &str,
        args: Value,
        vision: bool,
    ) -> Result<Option<Value>> {
        let frozen:Option<String>=sqlx::query_scalar("SELECT m.body FROM tool_manifests m JOIN execution_tool_views v USING(revision) WHERE v.execution=?").bind(exec).fetch_optional(store.readers()).await?;
        let Some(provider) = frozen
            .as_deref()
            .map(serde_json::from_str::<crate::tools::registry::Manifest>)
            .transpose()?
            .and_then(|m| m.0.get(name).and_then(|t| t.provider.clone()))
        else {
            return Ok(None);
        };
        let (parent, ordinal): (Option<String>, Option<i64>) = sqlx::query_as(
            "SELECT parent,ordinal FROM operations WHERE namespace=? AND operation_id=?",
        )
        .bind(exec)
        .bind(operation)
        .fetch_one(store.readers())
        .await?;
        Ok(Some(
            self.execute(
                store,
                calls::Request {
                    vision,
                    plugin: provider.id,
                    revision: provider.revision,
                    execution: exec.into(),
                    operation: operation.into(),
                    method: "tool.call".into(),
                    name: name.into(),
                    args,
                    parent,
                    ordinal,
                },
            )
            .await?,
        ))
    }
}
async fn start_session(
    store: &Store,
    install: &Install,
    data: &Path,
    cleanup: bool,
) -> Result<process::Session> {
    let (session, ready) = process::launch(store, install, data, cleanup).await?;
    if !matches!(
        tokio::time::timeout(Duration::from_secs(5), ready).await,
        Ok(Ok(true))
    ) {
        session.stop();
        return Err("plugin initialization unavailable".into());
    }
    Ok(session)
}
async fn pending_erasure(store: &Store, id: &str) -> Result<Vec<Value>> {
    let value =
        crate::privacy::invoke(store, crate::privacy::Action::Pending { owner: id.into() }).await?;
    Ok(serde_json::from_value(value["obligations"].clone())?)
}
async fn acknowledge(
    store: &Store,
    id: &str,
    session: &process::Session,
    ob: &Value,
) -> Result<()> {
    let params = json!({"key":ob["key"],"watermark":ob["watermark"],"sources":ob["sources"]});
    let reply = session
        .call(store, "forget.request", params.clone(), None)
        .await?;
    if reply["key"] != params["key"] || reply["watermark"] != params["watermark"] {
        return Err("cleanup acknowledgment differs from assigned watermark".into());
    }
    write(
        store,
        Change::Callback(
            id.into(),
            session.generation.clone(),
            "forget.ack".into(),
            reply,
        ),
    )
    .await?;
    Ok(())
}
async fn maintain(store: Store, install: Install, session: process::Session) {
    let mut watermark = 0i64;
    let mut hints = store.subscribe_commits();
    let mut last_forget = std::time::Instant::now() - Duration::from_secs(5);
    loop {
        if session.stopped() {
            break;
        }
        let _ = tokio::time::timeout(Duration::from_millis(500), hints.recv()).await;
        if session.stopped() {
            break;
        }
        let pending = match pending_erasure(&store, &install.id).await {
            Ok(items) => items,
            Err(_) => continue,
        };
        if let Some(ob) = pending.first() {
            if install.grants.contains("forget.owner")
                && last_forget.elapsed() >= Duration::from_secs(2)
            {
                last_forget = std::time::Instant::now();
                let _ = acknowledge(&store, &install.id, &session, ob).await;
            }
            continue;
        }
        if install.grants.contains("observe.metadata") && let Ok(rows)=sqlx::query("SELECT o.event,e.entry_id,json_extract(e.envelope,'$.kind') AS kind FROM host_observations o JOIN events e ON e.id=o.event JOIN host_installations i ON i.plugin=o.plugin AND i.online AND i.enabled AND i.generation=o.generation
            WHERE o.plugin=? AND o.generation=? AND o.event>? AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=o.event) ORDER BY o.event LIMIT 64").bind(&install.id).bind(&session.generation).bind(watermark).fetch_all(store.readers()).await{
            for r in rows{
                let Ok(event)=r.try_get::<i64,_>("event")else{continue};
                watermark=event;
                session.notify(json!({"jsonrpc":"2.0","method":"observer.event","params":{"event":event,"entry":r.try_get::<Option<String>,_>("entry_id").ok().flatten(),"kind":r.try_get::<String,_>("kind").unwrap_or_default()}}));
            }
        }
        if install.grants.contains("command.invoke") {
            let cmd=sqlx::query("SELECT c.event,c.path,c.execution,e.envelope FROM host_command_events c JOIN events e ON e.id=c.event WHERE c.plugin=? AND c.generation=? AND c.state='queued' ORDER BY event LIMIT 1").bind(&install.id).bind(&session.generation).fetch_optional(store.readers()).await;
            if let Ok(Some(r)) = cmd
                && let (Ok(event), Ok(path), Ok(exec), Ok(raw)) = (
                    r.try_get::<i64, _>("event"),
                    r.try_get::<String, _>("path"),
                    r.try_get::<String, _>("execution"),
                    r.try_get::<String, _>("envelope"),
                )
            {
                let f = serde_json::from_str::<admit_frame::Frame>(&raw);
                if let (Ok(frame), Some(hub)) = (f, &store.host) {
                    let args = json!({"text":frame.text.unwrap_or_else(||frame.segments.into_iter().filter_map(|s|if let admit_frame::Segment::Text{text}=s{Some(text)}else{None}).collect::<String>())});
                    let result = hub
                        .execute(
                            &store,
                            calls::Request {
                                vision: false,
                                plugin: install.id.clone(),
                                revision: install.revision().unwrap_or_default(),
                                execution: exec,
                                operation: format!("command:{event}"),
                                method: "command.invoke".into(),
                                name: path,
                                args,
                                parent: None,
                                ordinal: None,
                            },
                        )
                        .await;
                    let state = match result.as_ref().ok().and_then(|x| x["state"].as_str()) {
                        Some("confirmed" | "failed") => "complete",
                        Some("unknown" | "dispatched") => "unknown",
                        _ => "unavailable",
                    };
                    let _ = write(&store, Change::CommandDone(event, state.into())).await;
                }
            }
        }
    }
}
use crate::ingress::admit as admit_frame;
