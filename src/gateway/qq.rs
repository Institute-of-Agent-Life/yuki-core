mod activity;
mod frame;
mod projection;
mod recovery;

use crate::{Result, store::Store};
use axum::{
    Router,
    extract::{
        Path as RoutePath, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, watch};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub namespace: String,
    pub designated_ingress: String,
    pub bots: Vec<Bot>,
    #[serde(default)]
    pub groups: HashMap<String, GroupPolicy>,
    #[serde(default)]
    pub shared_decision: Option<SharedDecision>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedDecision {
    pub file: String,
    pub sha256: String,
    pub target_image_digest: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bot {
    pub self_id: String,
    pub stage: String,
    pub token_env: String,
    #[serde(default)]
    pub typing_private: bool,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupPolicy {
    Paused,
    Observe,
    Mention,
    Open,
}

struct Endpoint {
    spec: Bot,
    token: [u8; 32],
}
struct Shared {
    image: String,
    space: String,
    topic: Option<String>,
    sequence: String,
    receivers: HashSet<String>,
}

impl Shared {
    fn load(file: &SharedDecision, data: &Path) -> Result<Self> {
        let path = data.join(&file.file).canonicalize()?;
        if !path.starts_with(data.canonicalize()?) {
            return Err("QQ decision must be in data directory".into());
        }
        let raw = std::fs::read(path)?;
        if format!("{:x}", Sha256::digest(&raw)) != file.sha256 {
            return Err("QQ decision digest mismatch".into());
        }
        let doc: Value = serde_json::from_slice(&raw)?;
        let group = &doc["group"];
        if doc["deployment_verified"] != true
            || doc["kind"] != "target_capture"
            || doc["target_image_digest"] != file.target_image_digest
            || doc["capture_image_digest"] != file.target_image_digest
            || group["mode"] != "scoped_shared_key"
        {
            return Err("unwitnessed QQ shared key".into());
        }
        let receivers: HashSet<String> = serde_json::from_value(group["common_receivers"].clone())?;
        if receivers.len() < 2 {
            return Err("shared key requires paired receivers".into());
        }
        Ok(Self {
            image: file.target_image_digest.clone(),
            space: serde_json::from_value(group["applies_to"]["space"].clone())?,
            topic: serde_json::from_value(group["applies_to"]["topic"].clone())?,
            sequence: serde_json::from_value(group["scope"][2].clone())?,
            receivers,
        })
    }
}

#[derive(Subcommand, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Route {
        event: i64,
    },
    Status {
        bot: String,
    },
    Typing {
        event: i64,
        lease: String,
        #[arg(action=clap::ArgAction::Set)]
        begin: bool,
    },
}

#[derive(Clone)]
pub struct Hub(Option<Arc<Inner>>);
struct Inner {
    store: Store,
    namespace: String,
    designated: String,
    groups: HashMap<String, GroupPolicy>,
    bots: HashMap<String, Endpoint>,
    shared: Option<Shared>,
    topology: tokio::sync::RwLock<()>,
    active: Mutex<HashMap<String, Peer>>,
    next: AtomicU64,
}
struct Peer {
    session: u64,
    tx: mpsc::Sender<String>,
    cancel: watch::Sender<bool>,
    echoes: HashSet<String>,
    probes: HashMap<String, tokio::sync::oneshot::Sender<Value>>,
    matched: u64,
    leases: HashMap<String, activity::Leases>,
}

impl Hub {
    pub async fn new(store: Store, config: Option<Config>, data: &Path) -> Result<Self> {
        let Some(config) = config else {
            return Ok(Self(None));
        };
        let mut bots = HashMap::new();
        let mut tokens = HashSet::new();
        for bot in config.bots {
            let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM assignments s JOIN accounts a ON a.id=s.account_id
                WHERE s.id=? AND s.kind='self' AND s.status='active' AND a.platform='qq' AND a.namespace=? AND a.native=?)")
                .bind(&bot.stage).bind(&config.namespace).bind(&bot.self_id).fetch_one(store.readers()).await?;
            if !valid || bots.contains_key(&bot.self_id) {
                return Err("QQ endpoint lacks a unique active matching SELF stage".into());
            }
            let key = std::env::var(&bot.token_env).map_err(|_| "QQ token unavailable")?;
            let token: [u8; 32] = Sha256::digest(key.as_bytes()).into();
            if key.is_empty() || !tokens.insert(token) {
                return Err("QQ endpoints require distinct nonempty credentials".into());
            }
            bots.insert(bot.self_id.clone(), Endpoint { spec: bot, token });
        }
        if !bots.contains_key(&config.designated_ingress) {
            return Err("designated QQ ingress is not configured".into());
        }
        let shared = config
            .shared_decision
            .as_ref()
            .map(|s| Shared::load(s, data))
            .transpose()?;
        for (group, policy) in &config.groups {
            if matches!(policy, GroupPolicy::Paused) {
                store
                    .write(crate::store::Mutation::SeedPausedGroup(
                        config.namespace.clone(),
                        group.clone(),
                        bots[&config.designated_ingress].spec.stage.clone(),
                    ))
                    .await?;
            }
        }
        Ok(Self(Some(Arc::new(Inner {
            store,
            namespace: config.namespace,
            designated: config.designated_ingress,
            groups: config.groups,
            bots,
            shared,
            topology: tokio::sync::RwLock::new(()),
            active: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
        }))))
    }
    pub fn shutdown(&self) {
        if let Some(inner) = &self.0 {
            for peer in inner.active.lock().unwrap().values() {
                let _ = peer.cancel.send(true);
            }
        }
    }

    pub fn router(&self) -> Router {
        if self.0.is_none() {
            return Router::new();
        }
        Router::new()
            .route("/onebot/{bot}", get(upgrade))
            .with_state(self.clone())
    }
    pub async fn operator(&self, action: Action) -> Result<Value> {
        let inner = self.0.as_ref().ok_or("QQ not configured")?;
        match action {
            Action::Status { bot } => {
                let active = inner.active.lock().unwrap();
                let peer = active.get(&bot);
                Ok(
                    json!({"online":peer.is_some(),"matched_echoes":peer.map(|p|p.matched).unwrap_or(0)}),
                )
            }
            Action::Route { event } => {
                let target: Option<String> = sqlx::query_scalar(
                    "SELECT json_extract(envelope,'$.reply_bot') FROM events WHERE id=?",
                )
                .bind(event)
                .fetch_one(inner.store.readers())
                .await?;
                let active = inner.active.lock().unwrap();
                Ok(
                    json!({"bot":target,"online":target.as_ref().is_some_and(|id|active.contains_key(id))}),
                )
            }
            Action::Typing {
                event,
                lease,
                begin,
            } => activity::change(inner, event, lease, begin).await,
        }
    }
}

async fn upgrade(
    State(hub): State<Hub>,
    RoutePath(bot): RoutePath<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(inner) = hub.0 else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(endpoint) = inner.bots.get(&bot) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let token = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    let allowed = token.is_some_and(|s| {
        Sha256::digest(s.as_bytes())
            .iter()
            .zip(endpoint.token)
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
    });
    if !allowed
        || headers.get("x-self-id").and_then(|v| v.to_str().ok()) != Some(&bot)
        || headers.get("x-client-role").and_then(|v| v.to_str().ok()) != Some("Universal")
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.max_message_size(1024 * 1024)
        .on_upgrade(move |socket| connection(inner, bot, socket))
}

async fn connection(inner: Arc<Inner>, bot: String, mut socket: WebSocket) {
    let session = inner.next.fetch_add(1, Ordering::Relaxed);
    let (tx, mut outgoing) = mpsc::channel(32);
    let (cancel, mut cancelled) = watch::channel(false);
    let registering = inner.topology.write().await;
    if let Some(old) = inner.active.lock().unwrap().insert(
        bot.clone(),
        Peer {
            session,
            tx,
            cancel,
            echoes: HashSet::new(),
            probes: HashMap::new(),
            matched: 0,
            leases: HashMap::new(),
        },
    ) {
        let _ = old.cancel.send(true);
    }
    drop(registering);
    let mut controls = tokio::task::JoinSet::new();
    let mut refresh = tokio::time::interval(Duration::from_secs(4));
    refresh.tick().await;
    loop {
        tokio::select! {
            biased;
            _=cancelled.changed()=>break,
            message=socket.recv()=>match message{
                Some(Ok(Message::Text(raw)))=>{
                    let Ok(packet)=serde_json::from_str::<Value>(&raw)else{break};
                    if packet.get("post_type").is_some(){
                        if frame::id(&packet["self_id"]).as_deref()!=Some(&bot){break}
                        if recovery::command(&packet,&bot){
                            if controls.len()<8 {controls.spawn(recovery::handle(inner.clone(),bot.clone(),session,packet));}
                            continue;
                        }
                        if frame::receive(&inner,&bot,&packet).await.is_err(){break}
                    }else if let Some(echo)=packet["echo"].as_str(){
                        let mut active=inner.active.lock().unwrap();
                        if let Some(peer)=active.get_mut(&bot).filter(|p|p.session==session)
                            && packet.get("retcode").is_some(){
                            if let Some(probe)=peer.probes.remove(echo){let _=probe.send(packet);}
                            else if peer.echoes.remove(echo){peer.matched+=1;}
                        }
                    }
                },
                Some(Ok(Message::Ping(bytes)))=>{if socket.send(Message::Pong(bytes)).await.is_err(){break}},
                Some(Ok(Message::Pong(_)))=>{},
                _=>break,
            },
            _=controls.join_next(), if !controls.is_empty()=>{},
            Some(body)=outgoing.recv()=>if socket.send(Message::Text(body.into())).await.is_err(){break},
            _=refresh.tick()=>activity::refresh(&inner,&bot,session).await,
        }
    }
    {
        let _unregister = inner.topology.write().await;
        let mut active = inner.active.lock().unwrap();
        if active.get(&bot).is_some_and(|p| p.session == session) {
            active.remove(&bot);
        }
    }
    let _ = socket.send(Message::Close(None)).await;
}
