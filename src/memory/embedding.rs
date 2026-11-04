pub(super) mod profile;
mod writer;
use crate::{
    Result,
    store::{Mutation, Store},
};
use profile::Profile;
pub use profile::{Config, Mode};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::{sync::Arc, time::Duration};

pub struct Engine {
    pub(super) profile: Arc<Profile>,
    client: reqwest::Client,
    query_locks: [tokio::sync::Mutex<()>; 32],
}
pub enum Outcome {
    Ok(String),
    Rejected(i64),
    Unknown,
}
pub enum Change {
    Init,
    OpenReference(Arc<Profile>),
    Documents(Arc<Profile>),
    Query(Arc<Profile>, String, String, String, String),
    Dispatch(Arc<Profile>, String),
    Settle(Arc<Profile>, String, Outcome),
    Retry(String),
}
pub async fn apply(conn: &mut SqliteConnection, change: &Change) -> Result<Value> {
    match change {
        Change::Init => writer::init(conn).await,
        Change::OpenReference(p) => Ok(json!(super::index::open(conn, p).await?)),
        Change::Documents(p) => writer::documents(conn, p).await,
        Change::Query(p, exec, actor, op, text) => {
            writer::query_job(conn, p, exec, actor, op, text).await
        }
        Change::Dispatch(p, id) => writer::dispatch(conn, p, id).await,
        Change::Settle(p, id, out) => writer::settle(conn, p, id, out).await,
        Change::Retry(profile) => {
            let n=sqlx::query("UPDATE embedding_items SET state='released' WHERE state='missing' AND logical IN(SELECT logical FROM embedding_requests WHERE profile=? AND state IN('partial','failed'))").bind(profile).execute(conn).await?.rows_affected();
            Ok(json!({"released_missing":n}))
        }
    }
}
async fn write(store: &Store, change: Change) -> Result<Value> {
    store.write(Mutation::Vector(Box::new(change))).await
}
fn engine(store: &Store) -> Result<&Engine> {
    store
        .vectors
        .as_deref()
        .ok_or_else(|| "semantic embeddings disabled in lexical-only mode".into())
}
pub async fn status(store: &Store) -> Result<Value> {
    let Some(engine) = &store.vectors else {
        return Ok(json!({"mode":"lexical-only","embedding_enabled":false}));
    };
    let profile = engine.profile.identity();
    let (total,covered):(i64,i64)=sqlx::query_as("SELECT count(*),count(v.fact) FROM memory_facts f LEFT JOIN fact_vectors v ON v.fact=f.id AND v.revision=f.revision AND v.profile=? WHERE f.status='active'").bind(&profile).fetch_one(store.readers()).await?;
    let rows=sqlx::query("SELECT j.purpose,j.state,count(*) AS logical,sum((SELECT count(*) FROM model_attempts a WHERE a.logical=j.logical)) AS attempts,sum((SELECT count(*) FROM model_attempts a WHERE a.logical=j.logical AND a.ever_unknown)) AS unknowns,sum((SELECT json_extract(a.usage_json,'$.input_tokens') FROM model_attempts a WHERE a.logical=j.logical)) AS tokens FROM embedding_requests j WHERE j.profile=? GROUP BY j.purpose,j.state ORDER BY j.purpose,j.state").bind(&profile).fetch_all(store.readers()).await?;
    let usage:Vec<_>=rows.iter().map(|r|json!({"purpose":r.get::<&str,_>("purpose"),"state":r.get::<&str,_>("state"),"logical":r.get::<i64,_>("logical"),"attempts":r.get::<i64,_>("attempts"),"unknown":r.get::<i64,_>("unknowns"),"known_input_tokens":r.get::<Option<i64>,_>("tokens")})).collect();
    let degraded:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM embedding_items i JOIN embedding_requests j ON j.logical=i.logical WHERE j.profile=?1 AND i.state IN('missing','unknown')) OR coalesce((SELECT state IN('failed','partial','unknown') FROM embedding_requests WHERE profile=?1 AND purpose='query' ORDER BY rowid DESC LIMIT 1),0)").bind(&profile).fetch_one(store.readers()).await?;
    Ok(
        json!({"mode":"hybrid","profile":profile,"dimension":engine.profile.dimensions,"total":total,"covered":covered,"missing":total-covered,"degraded":degraded,"usage":usage}),
    )
}
pub async fn retry_missing(store: &Store) -> Result<Value> {
    write(store, Change::Retry(engine(store)?.profile.identity())).await
}
pub async fn query(store: &Store, exec: &str, actor: &str, op: &str, text: &str) -> Result<Value> {
    let engine = engine(store)?;
    let p = &engine.profile;
    // Bound synchronization memory. Recheck current authority after joining an identical query.
    use sha2::{Digest, Sha256};
    let normalized = profile::bounded(text, p.max_characters);
    let stripe = usize::from(Sha256::digest(normalized.as_bytes())[0]) % engine.query_locks.len();
    let _flight = engine.query_locks[stripe].lock().await;
    let prepared = write(
        store,
        Change::Query(p.clone(), exec.into(), actor.into(), op.into(), text.into()),
    )
    .await?;
    if let Some(id) = prepared["logical"].as_str() {
        run(store, id).await
    } else {
        Ok(prepared)
    }
}
async fn run(store: &Store, id: &str) -> Result<Value> {
    let engine = engine(store)?;
    let p = &engine.profile;
    let next = write(store, Change::Dispatch(p.clone(), id.into())).await?;
    if next["dispatch_now"] != true {
        return Err("embedding attempt already dispatched; no duplicate call".into());
    }
    let mut req = engine
        .client
        .post(p.endpoint())
        .header("content-type", "application/json")
        .body(
            next["request_raw"]
                .as_str()
                .ok_or("missing original embedding request")?
                .to_owned(),
        );
    if let Some(key) = &p.key_env {
        req = req.bearer_auth(std::env::var(key)?);
    }
    let outcome = match req.send().await {
        Ok(mut r) if r.status().is_success() => {
            let mut bytes = Vec::new();
            let mut valid = true;
            loop {
                match r.chunk().await {
                    Ok(Some(chunk)) if chunk.len() <= 32 * 1024 * 1024 - bytes.len() => {
                        bytes.extend_from_slice(&chunk)
                    }
                    Ok(None) => break,
                    _ => {
                        valid = false;
                        break;
                    }
                }
            }
            if valid {
                Outcome::Ok(String::from_utf8(bytes).unwrap_or_default())
            } else {
                Outcome::Unknown
            }
        }
        Ok(r) => Outcome::Rejected(i64::from(r.status().as_u16())),
        Err(err) if err.is_connect() => Outcome::Rejected(503),
        Err(_) => Outcome::Unknown,
    };
    write(store, Change::Settle(p.clone(), id.into(), outcome)).await
}
pub async fn start(store: &mut Store, cfg: Config) -> Result<Option<tokio::task::JoinHandle<()>>> {
    let Some(profile) = cfg.embedding else {
        return Ok(None);
    };
    let profile = Arc::new(profile);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    write(store, Change::Init).await?;
    store.vectors = Some(Arc::new(Engine {
        profile: profile.clone(),
        client,
        query_locks: std::array::from_fn(|_| tokio::sync::Mutex::new(())),
    }));
    let store = store.clone();
    Ok(Some(tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(300));
        loop {
            tick.tick().await;
            let step = async {
                if writer::needs(&store, &profile).await? {
                    let job = write(&store, Change::Documents(profile.clone())).await?;
                    if let Some(id) = job["logical"].as_str() {
                        run(&store, id).await?;
                    }
                }
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            }
            .await;
            if let Err(e) = step {
                eprintln!("Embedding worker: {e}");
            }
        }
    })))
}

/// Advance the optional comparison index by one committed page. A caller can
/// resume after a cold owner or short IPC deadline without claiming paid work.
pub(super) async fn open_reference(store: &Store) -> Result<bool> {
    let p = engine(store)?.profile.clone();
    let ready: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM retrieval_profiles WHERE profile=? AND cursor>=upper_bound)",
    )
    .bind(p.identity())
    .fetch_one(store.readers())
    .await?;
    Ok(ready || write(store, Change::OpenReference(p)).await? == true)
}
