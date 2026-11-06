mod aux_model;
pub(crate) mod calls;
mod commands;
pub(crate) mod composition;
pub(crate) mod configuration;
mod control;
pub(crate) mod domain;
pub(crate) mod gateway_media;
pub(crate) mod grants;
pub(crate) mod lifecycle;
pub(crate) mod maintenance;
mod media;
pub(crate) mod notification;
pub(crate) mod participation;
pub(crate) mod personal;
pub(crate) mod plugin_sessions;
mod process;
pub(crate) mod selected_processor;
mod send;
mod terminal_work;
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
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::sync::Mutex;

pub enum Change {
    AuxModel(Box<aux_model::Change>),
    Participation(Box<participation::Change>),
    Maintenance(Box<maintenance::Change>),
    PluginSession(Box<plugin_sessions::Change>),
    Notification(notification::Change),
    Configure(Vec<Install>),
    Start(String, bool, String),
    Online(String, String),
    Close(String, String),
    Enable(String, bool),
    Callback(String, String, String, Value),
    Call(calls::Change),
    FetchDispatch(Box<gateway_media::Gate>, String, String),
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
    slots: RwLock<BTreeMap<String, Arc<Slot>>>,
    inventory: Mutex<()>,
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
    fn installed(&self) -> Vec<(String, Arc<Slot>)> {
        self.slots
            .read()
            .expect("host inventory lock")
            .iter()
            .map(|(id, slot)| (id.clone(), slot.clone()))
            .collect()
    }
    fn slot(&self, id: &str) -> Result<Arc<Slot>> {
        self.slots
            .read()
            .expect("host inventory lock")
            .get(id)
            .cloned()
            .ok_or_else(|| "plugin not installed".into())
    }
    pub(crate) async fn discover(
        &self,
        store: &Store,
        authority: crate::operator::config::Authority,
        body: Value,
    ) -> Result<Value> {
        use crate::operator::domain::{Request, invoke};
        let _inventory = self.inventory.lock().await;
        let probe = invoke(
            store,
            Request::DiscoverAuth(authority.clone(), body.clone()),
        )
        .await?;
        let prepared = if probe["new"] == true {
            let declarations = (|| -> Result<Vec<Install>> {
                let installs = crate::config::Config::read(&self.data)?.plugins;
                grants::validate(&installs)?;
                Ok(installs)
            })();
            let installs = match declarations {
                Ok(installs) => installs,
                Err(_) => {
                    return invoke(
                        store,
                        Request::Failed(
                            authority,
                            "plugin.discover".into(),
                            body,
                            "validation_error".into(),
                        ),
                    )
                    .await;
                }
            };
            invoke(
                store,
                Request::Discover(authority.clone(), body.clone(), installs),
            )
            .await?
        } else {
            probe
        };
        if prepared["prepared"] != true {
            return Ok(prepared);
        }
        #[cfg(test)]
        crate::store::tests::commit_barrier("plugin-discovery-committed").await;
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT value FROM core_meta WHERE key LIKE 'plugin-install:%' ORDER BY key",
        )
        .fetch_all(store.readers())
        .await?;
        let installs = rows
            .into_iter()
            .map(|s| serde_json::from_str::<Install>(&s).map_err(Into::into))
            .collect::<Result<Vec<_>>>()?;
        let mut slots = BTreeMap::new();
        for (id, slot) in self.installed() {
            if installs.iter().any(|i| {
                i.id == id && serde_json::to_value(i).ok() == serde_json::to_value(&slot.spec).ok()
            }) {
                slots.insert(id, slot);
            } else if let Some(session) = slot.session.lock().await.take() {
                session.stop();
                session.wait_closed().await;
                write(store, Change::Close(id, session.generation)).await?;
            }
        }
        for spec in installs {
            slots.entry(spec.id.clone()).or_insert_with(|| {
                Arc::new(Slot {
                    spec,
                    session: Mutex::new(None),
                })
            });
        }
        *self.slots.write().expect("host inventory lock") = slots;
        invoke(store, Request::Discovered(authority, body)).await
    }
    pub(crate) async fn runtime_health(&self, store: &Store) -> Result<Vec<Value>> {
        let enabled: std::collections::BTreeSet<String> =
            sqlx::query_scalar("SELECT plugin FROM host_installations WHERE enabled")
                .fetch_all(store.readers())
                .await?
                .into_iter()
                .collect();
        let mut running = 0;
        let mut gateway = self.qq.connected();
        let (
            mut maintenance,
            mut maintenance_running,
            mut participation,
            mut participation_running,
        ) = (false, false, false, false);
        for (id, slot) in self.installed() {
            let active = enabled.contains(&id)
                && slot
                    .session
                    .lock()
                    .await
                    .as_ref()
                    .is_some_and(|s| !s.stopped());
            running += usize::from(active);
            gateway |= active && slot.spec.platform.is_some();
            maintenance |= enabled.contains(&id) && slot.spec.maintenance.is_some();
            maintenance_running |= active && slot.spec.maintenance.is_some();
            participation |= enabled.contains(&id) && slot.spec.participation.is_some();
            participation_running |= active && slot.spec.participation.is_some();
        }
        Ok(vec![
            json!({"name":"plugins","enabled":true,"running":running>0,"running_count":running,"healthy":null}),
            json!({"name":"gateway","enabled":null,"running":gateway,"healthy":null}),
            json!({"name":"memory_maintenance","enabled":maintenance,"running":maintenance_running,"healthy":null}),
            json!({"name":"semantic_participation","enabled":participation,"running":participation_running,"healthy":null}),
        ])
    }
    pub(crate) async fn observe(
        &self,
        store: &Store,
        args: &Value,
        generation: &str,
    ) -> Result<Value> {
        let id = args["plugin_id"].as_str().ok_or("validation_error")?;
        let slot = self.slot(id)?;
        let session = slot
            .session
            .lock()
            .await
            .clone()
            .filter(|s| s.generation == generation && !s.stopped())
            .ok_or("observation_unavailable")?;
        let params = json!({"cursor":args["cursor"],"limit":args["limit"].as_u64().unwrap_or(10)});
        let definition = grants::schema("plugin.observe", "host_to_plugin", &params)?;
        let output = session.call(store, "plugin.observe", params, None).await?;
        if serde_json::to_vec(&output)?.len() > 65536
            || !jsonschema::validator_for(&definition["result"])?.is_valid(&output)
        {
            return Err("observation_invalid".into());
        }
        Ok(output)
    }
    pub async fn start(
        store: &mut Store,
        installs: Vec<Install>,
        data: &Path,
        qq: crate::gateway::qq::Hub,
    ) -> Result<()> {
        grants::validate(&installs)?;
        if !installs.is_empty()
            || sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM host_installations)")
                .fetch_one(store.readers())
                .await?
        {
            write(store, Change::Configure(installs.clone())).await?;
        }
        let slots = installs
            .into_iter()
            .map(|spec| {
                (
                    spec.id.clone(),
                    Arc::new(Slot {
                        spec,
                        session: Mutex::new(None),
                    }),
                )
            })
            .collect();
        let (shutdown, end) = tokio::sync::watch::channel(false);
        let hub = Arc::new(Self {
            qq: qq.clone(),
            shutdown,
            slots: RwLock::new(slots),
            inventory: Mutex::new(()),
            data: data.into(),
        });
        store.host = Some(hub.clone());
        tokio::spawn(notification::serve(
            hub.clone(),
            store.clone(),
            qq.clone(),
            end.clone(),
        ));
        tokio::spawn(commands::serve(hub.clone(), store.clone(), qq, end));
        let mut pending = hub.installed();
        if grants::stable_base() {
            // A gateway can produce its first inbound request immediately after initialize.
            // Let independent tools finish their bounded handshakes before starting ingress.
            let (tools, others) = pending.into_iter().partition::<Vec<_>, _>(|(_, slot)| {
                slot.spec.grants.contains("tool.call")
                    && !slot.spec.grants.contains("gateway.inbound")
            });
            let mut starting = tokio::task::JoinSet::new();
            for (id, _) in tools {
                let (hub, store) = (hub.clone(), store.clone());
                starting.spawn(async move {
                    let _ = hub.restart(&store, &id).await;
                });
            }
            while let Some(joined) = starting.join_next().await {
                joined?;
            }
            pending = others;
        }
        for (id, slot) in pending {
            if slot.spec.composition.is_some() {
                // A discovered or disabled Code plugin must not block the owner from
                // starting its control plane and holding the original paid execution.
                let started = hub.reconcile(store, &id, false).await;
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
        self.reconcile(store, id, true).await
    }
    pub(crate) async fn reconcile(&self, store: &Store, id: &str, force: bool) -> Result<Value> {
        let slot = self.slot(id)?;
        let mut guard = slot.session.lock().await;
        let (enabled, online, generation): (bool, bool, Option<String>) = sqlx::query_as(
            "SELECT enabled,online,generation FROM host_installations WHERE plugin=?",
        )
        .bind(id)
        .fetch_one(store.readers())
        .await?;
        if !force
            && enabled
            && online
            && guard
                .as_ref()
                .is_some_and(|s| !s.stopped() && Some(&s.generation) == generation.as_ref())
        {
            return Ok(json!({"plugin":id,"generation":generation,"online":true}));
        }
        if let Some(old) = guard.take() {
            old.stop();
            old.wait_closed().await;
            write(store, Change::Close(id.into(), old.generation)).await?;
        }
        if !enabled {
            if force {
                return Err("plugin disabled".into());
            }
            return Ok(json!({"plugin":id,"online":false}));
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
        for (_, slot) in self.installed() {
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
                let slot = self.slot(&plugin)?;
                let spec = &slot.spec;
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
                } else if let Ok(slot) = self.slot(&id)
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
        let slot = self.slot(&r.plugin)?;
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
        let effective:Option<String>=sqlx::query_scalar("SELECT grants FROM host_installations WHERE plugin=? AND generation=? AND enabled AND online").bind(&install.id).bind(&session.generation).fetch_optional(store.readers()).await.ok().flatten();
        let Some(effective) = effective
            .and_then(|s| serde_json::from_str::<std::collections::BTreeSet<String>>(&s).ok())
        else {
            break;
        };
        let pending = match pending_erasure(&store, &install.id).await {
            Ok(items) => items,
            Err(_) => continue,
        };
        if let Some(ob) = pending.first() {
            if effective.contains("forget.owner") && last_forget.elapsed() >= Duration::from_secs(2)
            {
                last_forget = std::time::Instant::now();
                let _ = acknowledge(&store, &install.id, &session, ob).await;
            }
            continue;
        }
        if effective.contains("observe.metadata") && let Ok(rows)=sqlx::query("SELECT o.event,e.entry_id,json_extract(e.envelope,'$.kind') AS kind FROM host_observations o JOIN events e ON e.id=o.event JOIN host_installations i ON i.plugin=o.plugin AND i.online AND i.enabled AND i.generation=o.generation
            WHERE o.plugin=? AND o.generation=? AND o.event>? AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=o.event) ORDER BY o.event LIMIT 64").bind(&install.id).bind(&session.generation).bind(watermark).fetch_all(store.readers()).await{
            for r in rows{
                let Ok(event)=r.try_get::<i64,_>("event")else{continue};
                watermark=event;
                session.notify(json!({"jsonrpc":"2.0","method":"observer.event","params":{"event":event,"entry":r.try_get::<Option<String>,_>("entry_id").ok().flatten(),"kind":r.try_get::<String,_>("kind").unwrap_or_default()}}));
            }
        }
        if effective.contains("command.invoke") {
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
