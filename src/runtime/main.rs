mod domain;
mod step;
pub(crate) mod transport;
use crate::{
    Result,
    store::{Mutation, Store},
};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default, skip_serializing_if = "crate::model::wire::Protocol::is_chat")]
    pub protocol: crate::model::wire::Protocol,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub messages_thinking: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<i64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gemini_mixed_tools: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub native_web_search: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bridge_search: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub code_projection: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_page_budget: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_storage_bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    pub endpoint: String,
    pub model: String,
    pub name: String,
    pub merge_ms: i64,
    pub root_slots: i64,
    pub max_input_units: i64,
    pub max_output: i64,
    #[serde(default)]
    pub allow_remote: bool,
    pub key_env: Option<String>,
    pub binding_endpoint: Option<String>,
}
impl Config {
    pub fn contract(&self) -> String {
        format!(
            "chat-v1:{:x}",
            Sha256::digest(serde_json::to_vec(self).expect("profile serializes"))
        )
    }
    pub fn validate(&self) -> Result<()> {
        if (self.messages_thinking || self.thinking_budget.is_some())
            && !self.protocol.is_messages()
        {
            return Err("Messages thinking profile requires Messages protocol".into());
        }
        if self
            .thinking_budget
            .is_some_and(|n| n < 1024 || n >= self.max_output || self.reasoning_effort.is_some())
        {
            return Err("Messages manual budget must fit below max tokens and cannot specify adaptive effort".into());
        }
        if self
            .result_page_budget
            .is_some_and(|b| !(512..=32000).contains(&b))
            || self.result_storage_bytes.is_some_and(|n| n < 0)
        {
            return Err("invalid tool result storage or page budget".into());
        }
        if let Some(zone) = &self.timezone {
            zone.parse::<chrono_tz::Tz>()?;
        }
        if self.protocol.is_chat()
            && (self.native_web_search
                || self.tool_choice.is_some()
                || self.reasoning_effort.is_some())
        {
            return Err("Responses preferences require a Responses profile".into());
        }
        if self.tool_choice.as_deref().is_some_and(|v| {
            !matches!(v, "auto" | "none" | "required")
                && crate::tools::registry::builtin(v).is_none()
        }) {
            return Err("unknown provider tool selection".into());
        }
        if self
            .reasoning_effort
            .as_ref()
            .is_some_and(|v| v.trim().is_empty())
        {
            return Err("empty reasoning effort".into());
        }
        let url = reqwest::Url::parse(&self.endpoint)?;
        if self.gemini_mixed_tools && !self.protocol.is_gemini() {
            return Err("mixed Google Search profile requires GenerateContent".into());
        }
        if self.protocol.is_gemini() {
            let model = self.model.strip_prefix("models/").unwrap_or(&self.model);
            if model.is_empty()
                || !url
                    .path()
                    .ends_with(&format!("/models/{model}:generateContent"))
            {
                return Err(
                    "GenerateContent endpoint must select the original configured model".into(),
                );
            }
        }
        let local = url
            .host_str()
            .and_then(|h| h.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query().is_some()
            || !(local && url.scheme() == "http" || self.allow_remote && url.scheme() == "https")
        {
            return Err(
                "Main requires a loopback recorder or an explicitly enabled HTTPS endpoint".into(),
            );
        }
        if self.model.trim().is_empty()
            || !(0..=60000).contains(&self.merge_ms)
            || self.root_slots <= 0
            || self.max_output <= 0
            || self.max_input_units < self.max_output
        {
            return Err("invalid Main profile or budget".into());
        }
        if let Some(binding) = &self.binding_endpoint {
            let u = reqwest::Url::parse(binding)?;
            if u.scheme() != "http"
                || u.host_str()
                    .and_then(|h| h.parse::<std::net::IpAddr>().ok())
                    .is_none_or(|ip| !ip.is_loopback())
                || !u.username().is_empty()
                || u.password().is_some()
            {
                return Err("synthetic effect peer must be on loopback".into());
            }
        }
        Ok(())
    }
}
#[derive(Subcommand, Deserialize, Serialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Register {
        kind: String,
        id: String,
        conversation: String,
        #[arg(long)]
        parent: Option<String>,
    },
    Trigger {
        id: String,
        key: String,
        body: String,
    },
    Revoke {
        id: String,
    },
    Inspect {
        execution: String,
    },
}

pub enum Change {
    Operator(Action),
    Human(i64, i64),
    Defer(i64, i64),
    Prepare(String, String),
    Hold(String, String),
}
pub async fn apply(conn: &mut SqliteConnection, cfg: &Config, action: &Change) -> Result<Value> {
    match action {
        Change::Operator(cmd) => domain::operator(conn, cfg, cmd).await,
        Change::Human(event, now) => {
            domain::admit_human(conn, cfg, *event, *now).await?;
            sqlx::query("DELETE FROM main_admission_delays WHERE event=?")
                .bind(event)
                .execute(conn)
                .await?;
            Ok(Value::Null)
        }
        Change::Defer(event, until) => {
            sqlx::query("INSERT INTO main_admission_delays(event,retry_after,failures) VALUES(?,?,1)
                ON CONFLICT(event) DO UPDATE SET retry_after=excluded.retry_after,failures=main_admission_delays.failures+1")
                .bind(event).bind(until).execute(conn).await?;
            Ok(Value::Null)
        }
        Change::Prepare(exec, soul) => step::prepare(conn, cfg, exec, soul).await,
        Change::Hold(exec, reason) => step::hold(conn, exec, reason).await,
    }
}
async fn write(store: &Store, cfg: &Arc<Config>, change: Change) -> Result<Value> {
    store
        .write(Mutation::Main(cfg.clone(), Box::new(change)))
        .await
}
pub async fn invoke(store: &Store, action: Action) -> Result<Value> {
    let cfg = store.main.as_ref().ok_or("Main not configured")?;
    if let Action::Inspect { execution } = action {
        let mut tx = store.readers().begin().await?;
        return domain::inspect(&mut tx, &execution).await;
    }
    write(store, cfg, Change::Operator(action)).await
}
fn now() -> Result<i64> {
    Ok(i64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}
pub fn start(
    store: Store,
    data: PathBuf,
    hub: crate::gateway::qq::Hub,
) -> Result<Option<tokio::task::JoinHandle<()>>> {
    let Some(cfg) = store.main.clone() else {
        return Ok(None);
    };
    cfg.validate()?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(30))
        .build()?;
    let runner = Runner {
        hub,
        store,
        cfg,
        client,
        data,
        active: HashMap::new(),
        tasks: tokio::task::JoinSet::new(),
    };
    Ok(Some(tokio::spawn(runner.serve())))
}

struct Job {
    entry: String,
    cancel: watch::Sender<bool>,
    task: tokio::task::Id,
}
struct Runner {
    hub: crate::gateway::qq::Hub,
    store: Store,
    cfg: Arc<Config>,
    client: reqwest::Client,
    data: PathBuf,
    active: HashMap<String, Job>,
    tasks: tokio::task::JoinSet<String>,
}
impl Runner {
    async fn serve(mut self) {
        let mut timer = tokio::time::interval(Duration::from_millis(50));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_error = None;
        loop {
            tokio::select! {
                Some(result)=self.tasks.join_next(),if !self.tasks.is_empty()=>match result{
                    Ok(exec)=>{self.active.remove(&exec);}
                    Err(error)=>{
                        let original=self.active.iter().find(|(_,job)|job.task==error.id()).map(|(exec,_)|exec.clone());
                        if let Some(exec)=original{
                            self.active.remove(&exec);
                            let _=write(&self.store,&self.cfg,Change::Hold(exec,"runner_interrupted".into())).await;
                        }
                    }
                },
                _=timer.tick()=>{
                    let failure=self.tick().await.err().map(|err|err.to_string());
                    if failure!=last_error && failure.is_some(){eprintln!("Main admission paused: {}",failure.as_deref().unwrap_or_default());}
                    last_error=failure;
                }
            }
        }
    }

    async fn tick(&mut self) -> Result<()> {
        let timestamp = now()?;
        let events:Vec<i64>=sqlx::query_scalar("SELECT s.event_id FROM accepted_sources s JOIN events e ON e.id=s.event_id JOIN entries d ON d.id=e.entry_id
            JOIN assignments a ON a.id=e.actor_stage AND a.status='active' JOIN assignments b ON b.id=e.bot_stage AND b.status='active'
            JOIN persons p ON p.id=e.raw_actor JOIN persons r ON r.id=coalesce(p.current_root,p.id)
            WHERE p.enabled AND r.enabled AND d.closed_at IS NULL AND NOT EXISTS(SELECT 1 FROM main_inputs WHERE event=s.event_id)
            AND NOT EXISTS(SELECT 1 FROM host_command_events WHERE event=s.event_id) AND NOT EXISTS(SELECT 1 FROM main_controls WHERE event=s.event_id) AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=s.event_id)
            AND NOT EXISTS(SELECT 1 FROM executions x WHERE x.source=s.event_id AND (x.state!='pending' OR x.cancelled))
            AND NOT EXISTS(SELECT 1 FROM main_admission_delays WHERE event=s.event_id AND retry_after>?) ORDER BY s.event_id LIMIT 64").bind(timestamp).fetch_all(self.store.readers()).await?;
        for event in events {
            if write(&self.store, &self.cfg, Change::Human(event, timestamp))
                .await
                .is_err()
            {
                write(
                    &self.store,
                    &self.cfg,
                    Change::Defer(event, timestamp + 5000),
                )
                .await?;
            }
        }
        let mut busy: HashSet<String> = self.active.values().map(|job| job.entry.clone()).collect();
        for (exec, job) in &self.active {
            let cancelled: bool = sqlx::query_scalar("SELECT cancelled FROM executions WHERE id=?")
                .bind(exec)
                .fetch_one(self.store.readers())
                .await?;
            if cancelled && !*job.cancel.borrow() {
                let _ = job.cancel.send(true);
            }
        }
        let rows=sqlx::query("SELECT x.id,t.conversation FROM executions x JOIN main_turns t ON t.execution=x.id WHERE x.state IN ('pending','running')
          AND NOT x.cancelled AND t.ready_after<=? AND NOT EXISTS(SELECT 1 FROM main_turns prior JOIN executions p ON p.id=prior.execution
          WHERE prior.conversation=t.conversation AND prior.rowid<t.rowid AND p.state IN ('pending','running') AND NOT p.cancelled) ORDER BY t.rowid LIMIT 32")
          .bind(timestamp).fetch_all(self.store.readers()).await?;
        for row in rows {
            let exec: String = row.try_get("id")?;
            let entry: String = row.try_get("conversation")?;
            if self.active.len() >= 32 {
                break;
            }
            if !busy.insert(entry.clone()) {
                continue;
            }
            let (tx, rx) = watch::channel(false);
            let (store, cfg, client, data) = (
                self.store.clone(),
                self.cfg.clone(),
                self.client.clone(),
                self.data.clone(),
            );
            let id = exec.clone();
            let hub = self.hub.clone();
            let task = self.tasks.spawn(async move {
                if transport::run(&store, &cfg, &client, &data, &id, rx, &hub)
                    .await
                    .is_err()
                {
                    let _ = write(
                        &store,
                        &cfg,
                        Change::Hold(id.clone(), "runner_failed".into()),
                    )
                    .await;
                }
                id
            });
            self.active.insert(
                exec,
                Job {
                    entry,
                    cancel: tx,
                    task: task.id(),
                },
            );
        }
        Ok(())
    }
}
