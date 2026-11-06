pub mod blob;
pub(crate) mod handles;
mod tools;
use crate::{
    Result,
    gateway::qq::Hub,
    runtime::effects,
    store::{Mutation, Store},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use blob::{MAX_BYTES, Root, Snapshot};
pub(crate) use handles::display;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
pub(crate) use tools::{append_image, declaration, perform};

#[derive(Default, Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub input_root: Option<PathBuf>,
    pub gateway_transfer_directory: Option<String>,
    #[serde(default)]
    pub download_origins: Vec<String>,
}
pub struct Runtime {
    inputs: Arc<Root>,
    transfer: Arc<Root>,
    gateway: Option<String>,
    origins: Vec<reqwest::Url>,
    client: reqwest::Client,
}
impl Runtime {
    pub fn open(data: &Path, cfg: Config) -> Result<Arc<Self>> {
        let local = data.join("media-inputs");
        let transfer = data.join("media-transfer");
        for dir in [&local, &transfer] {
            match std::fs::create_dir(dir) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
        }
        let inputs = cfg.input_root.as_deref().unwrap_or(&local);
        if !inputs.is_absolute() {
            return Err("configured input root must be absolute".into());
        }
        let mut origins = Vec::new();
        for raw in cfg.download_origins {
            let url = reqwest::Url::parse(&raw)?;
            if !(url.scheme() == "https"
                || url.scheme() == "http"
                    && url
                        .host_str()
                        .and_then(|s| s.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok())
                        .is_some_and(|ip| ip.is_loopback()))
                || !url.username().is_empty()
                || url.password().is_some()
                || url.path() != "/"
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err("invalid configured media origin".into());
            }
            origins.push(url);
        }
        if cfg
            .gateway_transfer_directory
            .as_deref()
            .is_some_and(|v| v.trim().is_empty() || v.contains('\0'))
        {
            return Err("invalid receiver transfer directory".into());
        }
        Ok(Arc::new(Self {
            inputs: Arc::new(Root::open(inputs)?),
            transfer: Arc::new(Root::open(&transfer)?),
            gateway: cfg.gateway_transfer_directory,
            origins,
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(20))
                .build()?,
        }))
    }
    async fn receive(&self, hub: &Hub, proof: &Value) -> Result<Snapshot> {
        let reference = proof["origin"]
            .as_str()
            .ok_or("original media reference unavailable")?;
        let kind = proof["kind"].as_str().ok_or("missing media kind")?;
        if let Some(s) = self.direct(reference).await? {
            return Ok(s);
        }
        let (action, args) = match kind {
            "image" => ("get_image", json!({"file":reference})),
            "audio" | "record" => ("get_record", json!({"file":reference,"out_format":"mp3"})),
            _ => return Err("original media needs an authorized byte source".into()),
        };
        let response = hub
            .request(
                proof["stage"].as_str().ok_or("missing receiver stage")?,
                action,
                args,
            )
            .await?;
        if response["status"] != "ok" || response["retcode"] != 0 {
            return Err("original receiver could not resolve media".into());
        }
        let info = &response["data"];
        let info = if info["data"].is_object() {
            &info["data"]
        } else {
            info
        };
        let keys = if action == "get_record" {
            ["base64", "file", "url"]
        } else {
            ["url", "file", "base64"]
        };
        for key in keys {
            if let Some(value) = info[key].as_str() {
                if key == "base64" && !value.starts_with("data:") && !value.starts_with("base64://")
                {
                    return decode(value);
                }
                if let Some(s) = self.direct(value).await? {
                    return Ok(s);
                }
            }
        }
        Err("receiver returned no authorized bytes".into())
    }
    async fn direct(&self, value: &str) -> Result<Option<Snapshot>> {
        if let Some(encoded) = value.strip_prefix("base64://") {
            return decode(encoded).map(Some);
        }
        if let Some(data) = value.strip_prefix("data:") {
            let (header, encoded) = data.split_once(',').ok_or("invalid media data URL")?;
            if !header.ends_with(";base64") {
                return Err("base64 media required".into());
            }
            return decode(encoded).map(Some);
        }
        if !value.starts_with("http://") && !value.starts_with("https://") {
            return Ok(None);
        }
        let url = reqwest::Url::parse(value)?;
        if !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || !self
                .origins
                .iter()
                .any(|good| good.origin() == url.origin())
        {
            return Ok(None);
        }
        let mut response = self.client.get(url).send().await?.error_for_status()?;
        if response
            .content_length()
            .is_some_and(|n| n > MAX_BYTES as u64)
        {
            return Err("media too large".into());
        }
        let mut bytes = Vec::new();
        while let Some(part) = response.chunk().await? {
            if part.len() > MAX_BYTES - bytes.len() {
                return Err("media too large".into());
            }
            bytes.extend_from_slice(&part);
        }
        Snapshot::new(bytes).map(Some)
    }
}
fn decode(encoded: &str) -> Result<Snapshot> {
    if encoded.len() > MAX_BYTES.div_ceil(3) * 4 {
        return Err("encoded media too large".into());
    }
    Snapshot::new(STANDARD.decode(encoded)?)
}
pub(crate) fn mime(snapshot: &Snapshot) -> &'static str {
    let b = &snapshot.data;
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        "image/gif"
    } else if b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else if std::str::from_utf8(b).is_ok() && !b.contains(&0) {
        "text/plain"
    } else {
        "application/octet-stream"
    }
}
#[derive(clap::Subcommand, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Metadata {
        exec: String,
        actor: String,
        event: i64,
    },
    CacheFile {
        exec: String,
        actor: String,
        event: i64,
        slot: i64,
        stage: String,
        reference: String,
        relative: String,
    },
    Snapshot {
        exec: String,
        actor: String,
        operation: String,
        relative: String,
        filename: String,
        kind: String,
    },
    Pin {
        exec: String,
        actor: String,
        event: i64,
        slot: i64,
        operation: String,
        filename: String,
        kind: String,
    },
    Gc,
}
pub enum Change {
    Cache {
        exec: String,
        actor: String,
        event: i64,
        slot: i64,
        proof: Value,
        data: Snapshot,
    },
    Version {
        exec: String,
        actor: String,
        op: String,
        requested: Option<String>,
        basis: String,
        filename: String,
        kind: String,
        data: Snapshot,
        source: Option<(i64, i64)>,
    },
    Stage {
        exec: String,
        actor: String,
        version: String,
    },
    Bind {
        exec: String,
        actor: String,
        version: String,
        relative: Option<String>,
        reference: String,
    },
    Result {
        exec: String,
        actor: String,
        op: String,
        value: Value,
        proof: Option<Value>,
        success: bool,
    },
    Gc,
    Abandon(String),
    Reaped(String),
    Startup,
}
fn runtime(store: &Store) -> Result<&Arc<Runtime>> {
    store
        .media
        .as_ref()
        .ok_or_else(|| "media runtime unavailable".into())
}
pub async fn apply(conn: &mut SqliteConnection, change: &Change) -> Result<Value> {
    match change {
        Change::Cache {
            exec,
            actor,
            event,
            slot,
            proof,
            data,
        } => handles::cache(conn, exec, actor, *event, *slot, proof, data).await,
        Change::Version {
            exec,
            actor,
            op,
            requested,
            basis,
            filename,
            kind,
            data,
            source,
        } => {
            let content_type = mime(data);
            if kind == "image" && !content_type.starts_with("image/") {
                return Err("source is not a supported image".into());
            }
            handles::version(
                conn,
                &handles::Version {
                    exec,
                    actor,
                    op,
                    requested: requested.as_deref(),
                    basis,
                    filename,
                    kind,
                    mime: content_type,
                    source: *source,
                },
                data,
            )
            .await
        }
        Change::Stage {
            exec,
            actor,
            version,
        } => {
            effects::authorize(conn, exec, actor, true).await?;
            bind_version_sources(conn, exec, version).await?;
            let r=sqlx::query("SELECT v.digest FROM media_versions v JOIN media_objects b ON b.digest=v.digest WHERE v.id=? AND EXISTS(SELECT 1 FROM media_version_readers r WHERE r.version=v.id AND r.reader=?)").bind(version).bind(exec).fetch_one(&mut *conn).await?;
            let id = crate::identity::new_id(conn).await?;
            let rel = format!("yuki-{id}.bin");
            sqlx::query("INSERT INTO media_transfer_files VALUES(?,?,?,'pending')")
                .bind(&rel)
                .bind(r.get::<String, _>("digest"))
                .bind(version)
                .execute(conn)
                .await?;
            Ok(json!({"relative":rel,"digest":r.get::<String,_>("digest")}))
        }
        Change::Bind {
            exec,
            actor,
            version,
            relative,
            reference,
        } => {
            effects::authorize(conn, exec, actor, true).await?;
            bind_version_sources(conn, exec, version).await?;
            if let Some(rel) = relative {
                let existing: Option<String> = sqlx::query_scalar(
                    "SELECT relative FROM media_transfer_files WHERE version=? AND state='ready'",
                )
                .bind(version)
                .fetch_optional(&mut *conn)
                .await?;
                if existing.is_some_and(|old| old != *rel) {
                    sqlx::query("UPDATE media_transfer_files SET state='stale' WHERE relative=? AND version=?").bind(rel).bind(version).execute(conn).await?;
                    return Ok(json!({"existing":true}));
                }
                sqlx::query_scalar::<_,String>("UPDATE media_transfer_files SET state='ready' WHERE version=? AND relative=? AND digest=(SELECT digest FROM media_versions v WHERE id=? AND EXISTS(SELECT 1 FROM media_version_readers r WHERE r.version=v.id AND r.reader=?)) RETURNING relative")
                    .bind(version).bind(rel).bind(version).bind(exec).fetch_one(&mut *conn).await?;
            }
            sqlx::query_scalar::<_,String>("INSERT INTO social_media_bindings SELECT id,execution,kind,filename,?1 FROM media_versions v WHERE id=?2 AND EXISTS(SELECT 1 FROM media_version_readers r WHERE r.version=v.id AND r.reader=?3) AND digest IS NOT NULL ON CONFLICT(id) DO UPDATE SET id=excluded.id WHERE social_media_bindings.execution=excluded.execution AND social_media_bindings.peer_reference=excluded.peer_reference RETURNING id")
                .bind(reference).bind(version).bind(exec).fetch_one(conn).await?;
            Ok(json!({"bound":true}))
        }
        Change::Result {
            exec,
            actor,
            op,
            value,
            proof,
            success,
        } => tools::finish(conn, exec, actor, op, value, proof.as_ref(), *success).await,
        Change::Gc => handles::expire(conn, handles::now()?).await,
        Change::Abandon(rel) => {
            sqlx::query("UPDATE media_transfer_files SET state='stale' WHERE relative=? AND state='pending'").bind(rel).execute(conn).await?;
            Ok(Value::Null)
        }
        Change::Reaped(rel) => {
            sqlx::query("DELETE FROM media_transfer_files WHERE relative=? AND (state='stale' OR NOT EXISTS(SELECT 1 FROM media_versions WHERE id=media_transfer_files.version AND digest IS NOT NULL))").bind(rel).execute(conn).await?;
            Ok(Value::Null)
        }
        Change::Startup => {
            sqlx::query("UPDATE media_transfer_files SET state='stale' WHERE state='pending'")
                .execute(conn)
                .await?;
            Ok(Value::Null)
        }
    }
}
async fn write(store: &Store, change: Change) -> Result<Value> {
    store.write(Mutation::Media(Box::new(change))).await
}
async fn acquire(
    store: &Store,
    hub: &Hub,
    exec: &str,
    actor: &str,
    event: i64,
    slot: i64,
    local: Option<(&str, &str, &str)>,
) -> Result<Value> {
    let mut proof = handles::original(
        &mut *store.readers().acquire().await?,
        exec,
        actor,
        event,
        slot,
    )
    .await?;
    if proof["state"] == "expired" {
        return Err("media expired; metadata retained".into());
    }
    if let Some((stage, source, _)) = local
        && (proof["stage"] != stage || proof["origin"] != source)
    {
        return Err("source or receiving stage mismatch".into());
    }
    if proof["state"] == "cached" {
        return Ok(proof);
    }
    let rt = runtime(store)?;
    let snapshot = if let Some((_, _, path)) = local {
        let path = path.to_owned();
        let root = rt.inputs.clone();
        tokio::task::spawn_blocking(move || root.snapshot(&path)).await??
    } else {
        rt.receive(hub, &proof).await?
    };
    proof["acquired_via"] = json!(if local.is_some() {
        "trusted_input_file"
    } else {
        "original_receiver"
    });
    write(
        store,
        Change::Cache {
            exec: exec.into(),
            actor: actor.into(),
            event,
            slot,
            proof,
            data: snapshot,
        },
    )
    .await
}
pub(crate) async fn bind_version_sources(
    conn: &mut SqliteConnection,
    exec: &str,
    version: &str,
) -> Result<()> {
    let producer: String = sqlx::query_scalar(
        "SELECT producer FROM media_version_readers WHERE reader=? AND version=?",
    )
    .bind(exec)
    .bind(version)
    .fetch_one(&mut *conn)
    .await?;
    let src: Option<i64> = sqlx::query_scalar("SELECT source FROM executions WHERE id=?")
        .bind(&producer)
        .fetch_one(&mut *conn)
        .await?;
    if crate::privacy::source::execution_erased(conn, &producer, src).await? {
        return Err("original media source expired".into());
    }
    sqlx::query("INSERT OR IGNORE INTO main_dependencies SELECT ?1,e.id FROM events e WHERE e.id IN(SELECT source FROM media_versions WHERE id=?2 UNION SELECT source FROM executions WHERE id=?3 UNION SELECT event FROM main_dependencies WHERE execution=?3)").bind(exec).bind(version).bind(producer).execute(conn).await?;
    Ok(())
}
pub(crate) async fn prepare_host_delivery(
    store: &Store,
    exec: &str,
    actor: &str,
    args: &str,
) -> Result<()> {
    let message: Value = serde_json::from_str(args)?;
    let Some(id) = message["artifact_id"].as_str() else {
        return Ok(());
    };
    let mut tx = store.readers().begin().await?;
    effects::authorize(&mut tx, exec, actor, true).await?;
    let value=sqlx::query_scalar::<_,String>("SELECT v.kind FROM host_media_uploads h JOIN media_versions v ON v.id=h.version AND v.digest=h.digest JOIN media_version_readers r ON r.version=v.id WHERE h.version=? AND r.reader=?").bind(id).bind(exec).fetch_optional(&mut *tx).await?;
    tx.commit().await?;
    if let Some(kind) = value {
        bind(store, exec, actor, &json!({"artifact_id":id,"kind":kind})).await?;
    }
    Ok(())
}
async fn bind(store: &Store, exec: &str, actor: &str, v: &Value) -> Result<()> {
    let id = v["artifact_id"].as_str().ok_or("missing version")?;
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM social_media_bindings b JOIN media_version_readers r ON r.version=b.id AND r.producer=b.execution WHERE b.id=? AND r.reader=?)",
    )
    .bind(id)
    .bind(exec)
    .fetch_one(store.readers())
    .await?
    {
        return Ok(());
    }
    let rt = runtime(store)?;
    let bytes:Vec<u8>=sqlx::query_scalar("SELECT b.bytes FROM media_versions v JOIN media_objects b ON b.digest=v.digest JOIN media_version_readers r ON r.version=v.id WHERE v.id=? AND r.reader=?").bind(id).bind(exec).fetch_one(store.readers()).await?;
    let (relative, reference) = if v["kind"] == "image" {
        (None, format!("base64://{}", STANDARD.encode(bytes)))
    } else {
        let Some(base) = &rt.gateway else {
            return Ok(());
        };
        let stage = write(
            store,
            Change::Stage {
                exec: exec.into(),
                actor: actor.into(),
                version: id.into(),
            },
        )
        .await?;
        let rel = stage["relative"]
            .as_str()
            .ok_or("missing reserved transfer path")?
            .to_owned();
        let root = rt.transfer.clone();
        let name = rel.clone();
        let staged = tokio::task::spawn_blocking(move || root.stage(&name, &bytes))
            .await
            .unwrap_or_else(|e| Err(e.into()));
        if let Err(error) = staged {
            write(store, Change::Abandon(rel)).await?;
            return Err(error);
        }
        (
            Some(rel.clone()),
            format!("{}/{}", base.trim_end_matches(['/', '\\']), rel),
        )
    };
    let bound = write(
        store,
        Change::Bind {
            exec: exec.into(),
            actor: actor.into(),
            version: id.into(),
            relative: relative.clone(),
            reference,
        },
    )
    .await;
    if let (Err(_), Some(rel)) = (&bound, relative) {
        write(store, Change::Abandon(rel)).await?;
    }
    bound.map(|_| ())
}
pub struct FileInput<'a> {
    pub exec: &'a str,
    pub actor: &'a str,
    pub operation: &'a str,
    pub relative: &'a str,
    pub filename: &'a str,
    pub kind: &'a str,
    pub named: Option<&'a str>,
}
pub async fn snapshot(store: &Store, input: FileInput<'_>) -> Result<Value> {
    let FileInput {
        exec,
        actor,
        operation: op,
        relative,
        filename,
        kind,
        named,
    } = input;
    parts_authorized(store, exec, actor).await?;
    let basis = fingerprint(&json!(["root_snapshot", relative, filename, kind, named]));
    if let Some(v) = existing(store, exec, op, &basis).await? {
        bind(store, exec, actor, &v).await?;
        return Ok(v);
    }
    let input = runtime(store)?.inputs.clone();
    let relative = relative.to_owned();
    let data = tokio::task::spawn_blocking(move || input.snapshot(&relative)).await??;
    let v = write(
        store,
        Change::Version {
            exec: exec.into(),
            actor: actor.into(),
            op: op.into(),
            requested: named.map(str::to_owned),
            basis,
            filename: filename.into(),
            kind: kind.into(),
            data,
            source: None,
        },
    )
    .await?;
    bind(store, exec, actor, &v).await?;
    Ok(v)
}
fn fingerprint(source: &Value) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(source.to_string().as_bytes()))
}
async fn existing(store: &Store, exec: &str, op: &str, basis: &str) -> Result<Option<Value>> {
    let row=sqlx::query("SELECT id,digest,filename,kind,mime,size,basis FROM media_versions WHERE execution=? AND operation=?").bind(exec).bind(op).fetch_optional(store.readers()).await?;
    let Some(r) = row else { return Ok(None) };
    if r.get::<String, _>("basis") != basis || r.try_get::<Option<String>, _>("digest")?.is_none() {
        return Err("original version request conflicts or was erased".into());
    }
    Ok(Some(
        json!({"artifact_id":r.get::<String,_>("id"),"sha256":r.get::<String,_>("digest"),"filename":r.get::<String,_>("filename"),"kind":r.get::<String,_>("kind"),"mime":r.get::<String,_>("mime"),"bytes":r.get::<i64,_>("size")}),
    ))
}
async fn parts_authorized(store: &Store, exec: &str, actor: &str) -> Result<()> {
    effects::authorize(&mut *store.readers().acquire().await?, exec, actor, true).await?;
    Ok(())
}
pub async fn invoke(store: &Store, hub: &Hub, act: Action) -> Result<Value> {
    match act {
        Action::Metadata { exec, actor, event } => {
            let mut read = store.readers().begin().await?;
            let scope = crate::history::query::scope(&mut read, &exec, &actor).await?;
            let entry:String=sqlx::query_scalar("SELECT entry_id FROM events WHERE id=? AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=?)").bind(event).bind(event).fetch_one(&mut *read).await?;
            if !scope.entries.contains(&entry) {
                return Err("media entry unavailable".into());
            }
            Ok(json!({"event":event,"attachments":handles::display(&mut read,event).await?}))
        }
        Action::CacheFile {
            exec,
            actor,
            event,
            slot,
            stage,
            reference,
            relative,
        } => {
            acquire(
                store,
                hub,
                &exec,
                &actor,
                event,
                slot,
                Some((&stage, &reference, &relative)),
            )
            .await
        }
        Action::Snapshot {
            exec,
            actor,
            operation,
            relative,
            filename,
            kind,
        } => {
            snapshot(
                store,
                FileInput {
                    exec: &exec,
                    actor: &actor,
                    operation: &operation,
                    relative: &relative,
                    filename: &filename,
                    kind: &kind,
                    named: None,
                },
            )
            .await
        }
        Action::Pin {
            exec,
            actor,
            event,
            slot,
            operation,
            filename,
            kind,
        } => {
            parts_authorized(store, &exec, &actor).await?;
            let basis = fingerprint(&json!(["attachment_pin", event, slot, filename, kind]));
            if let Some(v) = existing(store, &exec, &operation, &basis).await? {
                bind(store, &exec, &actor, &v).await?;
                return Ok(v);
            }
            let proof = acquire(store, hub, &exec, &actor, event, slot, None).await?;
            let bytes: Vec<u8> =
                sqlx::query_scalar("SELECT bytes FROM media_objects WHERE digest=?")
                    .bind(proof["digest"].as_str())
                    .fetch_one(store.readers())
                    .await?;
            let v = write(
                store,
                Change::Version {
                    exec: exec.clone(),
                    actor: actor.clone(),
                    op: operation,
                    requested: None,
                    basis,
                    filename,
                    kind,
                    data: Snapshot::new(bytes)?,
                    source: Some((event, slot)),
                },
            )
            .await?;
            bind(store, &exec, &actor, &v).await?;
            Ok(v)
        }
        Action::Gc => gc(store).await,
    }
}
pub async fn gc(store: &Store) -> Result<Value> {
    let mut result = write(store, Change::Gc).await?;
    let rows:Vec<String>=sqlx::query_scalar("SELECT f.relative FROM media_transfer_files f JOIN media_versions v ON v.id=f.version WHERE f.state='stale' OR v.digest IS NULL").fetch_all(store.readers()).await?;
    let mut removed = 0;
    for name in rows {
        let root = runtime(store)?.transfer.clone();
        let path = name.clone();
        tokio::task::spawn_blocking(move || root.unlink(&path)).await??;
        write(store, Change::Reaped(name)).await?;
        removed += 1;
    }
    result["transfer_files_removed"] = json!(removed);
    Ok(result)
}
pub async fn recover(store: &Store) -> Result<()> {
    if sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM media_transfer_files WHERE state='pending')")
        .fetch_one(store.readers())
        .await?
    {
        write(store, Change::Startup).await?;
        gc(store).await?;
    }
    Ok(())
}
pub fn start(store: Store) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            timer.tick().await;
            let needed=sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM media_sources WHERE state='cached' AND expires<=CAST(unixepoch('subsec')*1000 AS INTEGER)) OR EXISTS(SELECT 1 FROM media_transfer_files f JOIN media_versions v ON v.id=f.version WHERE f.state='stale' OR v.digest IS NULL) OR EXISTS(SELECT 1 FROM media_objects WHERE digest NOT IN(SELECT digest FROM media_sources WHERE digest IS NOT NULL UNION SELECT digest FROM media_versions WHERE digest IS NOT NULL UNION SELECT digest FROM media_reads WHERE digest IS NOT NULL))").fetch_one(store.readers()).await;
            if matches!(needed, Ok(true))
                && let Err(e) = gc(&store).await
            {
                eprintln!("Media collection pending: {e}");
            }
        }
    })
}
