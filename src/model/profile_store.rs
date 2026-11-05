mod boot;
mod catalog;
mod files;
pub(crate) mod http;
pub(crate) mod runtime;
pub(crate) mod wire;

use crate::{
    Result,
    identity::new_id,
    media::blob::Root,
    operator::config::{self, Actor, Authority},
    store::{Mutation, Store},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) fn task_supported(task: &str) -> bool {
    catalog::TASKS.contains(&task)
}

pub(crate) struct Service {
    root: Root,
    filename: String,
    target: String,
    operators: Option<PathBuf>,
    baseline: crate::runtime::main::Config,
    allow_native: Option<bool>,
    connections_suspended: bool,
    boot_revision: std::sync::RwLock<Option<u64>>,
    credentials: std::sync::RwLock<std::collections::BTreeMap<String, Arc<runtime::Credentials>>>,
}

pub(crate) enum Request {
    Start,
    Read(Authority),
    Prepare(Authority, Value),
    Secret(Authority, Value),
    Commit(Authority, String),
    Receipt(Authority, String),
}

#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    target: String,
    digest: String,
    actor: String,
    source_person: Option<String>,
    plan: Option<files::Prepared>,
    configurations: std::collections::BTreeMap<String, crate::runtime::main::Config>,
    connections: String,
    receipt: Value,
}

impl Service {
    pub(crate) fn connections_suspended(&self) -> bool {
        self.connections_suspended
    }

    pub(crate) fn open(
        data: &Path,
        path: &Path,
        operators: Option<&Path>,
        baseline: crate::runtime::main::Config,
    ) -> Result<Arc<Self>> {
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            data.join(path)
        };
        let parent = path
            .parent()
            .ok_or("configuration_unavailable")?
            .canonicalize()?;
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("configuration_unavailable")?
            .to_owned();
        let target = parent.join(&filename).to_string_lossy().into_owned();
        #[cfg(windows)]
        let target = target.to_lowercase();
        Ok(Arc::new(Self {
            root: Root::open(&parent)?,
            filename,
            target,
            baseline,
            allow_native: None,
            connections_suspended: false,
            boot_revision: Default::default(),
            credentials: Default::default(),
            operators: operators.map(|path| {
                if path.is_absolute() {
                    path.to_owned()
                } else {
                    data.join(path)
                }
            }),
        }))
    }
}

fn request_id(body: &Value) -> Result<String> {
    let id = body["request_id"]
        .as_str()
        .ok_or("validation_error")?
        .to_ascii_lowercase();
    crate::host::personal::credentials::uuid(&id)?;
    Ok(id)
}

fn key(actor: &Actor, id: &str) -> String {
    format!(
        "model-profile-operation:{}",
        crate::memory::scope::hash(&json!([actor.namespace, id]))
    )
}

async fn read_record(db: &mut SqliteConnection, key: &str) -> Result<Option<Record>> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM core_meta WHERE key=?")
        .bind(key)
        .fetch_optional(db)
        .await?;
    let record: Option<Record> = value.as_deref().map(serde_json::from_str).transpose()?;
    if record.as_ref().is_some_and(|record| record.version != 1) {
        return Err("original_configuration_operation_unavailable".into());
    }
    Ok(record)
}

async fn store_record(db: &mut SqliteConnection, key: &str, record: &Record) -> Result<()> {
    sqlx::query("INSERT INTO core_meta(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value,revision=core_meta.revision+1")
        .bind(key).bind(serde_json::to_string(record)?).execute(db).await?;
    Ok(())
}

async fn prepare(
    db: &mut SqliteConnection,
    service: &Service,
    actor: Actor,
    body: &Value,
    secret: bool,
) -> Result<Value> {
    catalog::validate(if secret { "SecretBody" } else { "ModelsBody" }, body)?;
    let id = request_id(body)?;
    let mut canonical = body.clone();
    canonical["request_id"] = json!(id);
    let digest = crate::memory::scope::hash(&json!([secret, canonical]));
    let key = key(&actor, &id);
    if let Some(original) = read_record(db, &key).await? {
        if original.digest != digest {
            return Err("request_conflict".into());
        }
        return Ok(json!({"request_id":id,"complete":original.plan.is_none()}));
    }
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM core_meta WHERE key='model-profile-active-operation')",
    )
    .fetch_one(&mut *db)
    .await?
    {
        return Err("version_conflict".into());
    }
    if !service.root.writable()? {
        return Err("configuration_not_writable".into());
    }
    let bundle = files::Bundle::read(&service.root, &service.filename)?;
    if secret {
        let snapshot = bundle
            .configuration
            .as_ref()
            .ok_or("configuration_unavailable")?;
        let document = catalog::Catalog::parse(std::str::from_utf8(&snapshot.data)?)?.public;
        let name = canonical["name"]
            .as_str()
            .ok_or("validation_error")?
            .to_owned();
        canonical = json!({"request_id":id,"expected_revision":canonical["expected_revision"],
            "document":document,"api_keys":{name:canonical["value"]}});
    }
    let plan = bundle.prepare(&canonical)?;
    let catalog = catalog::Catalog::parse(std::str::from_utf8(&plan.configuration)?)?;
    let keys = files::keys(plan.secrets.as_deref())?;
    let configurations = runtime::compile(service, &catalog, &keys)?;
    let connections = connection_identity(&catalog.public, &keys);
    let audit = new_id(db).await?;
    let record = Record {
        version: 1,
        target: service.target.clone(),
        digest,
        actor: actor.namespace,
        source_person: actor.source_person,
        plan: Some(plan),
        configurations,
        connections,
        receipt: json!({"success":false,"resource_id":"model_profiles","revision":bundle.revision,
            "audit_id":audit,"effective_state":{"status":"prepared"},"operation":null}),
    };
    store_record(db, &key, &record).await?;
    sqlx::query("INSERT INTO core_meta(key,value) VALUES('model-profile-active-operation',?)")
        .bind(&key)
        .execute(db)
        .await?;
    Ok(json!({"request_id":id,"complete":false}))
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

fn replace_part(
    root: &Root,
    name: &str,
    staged: &str,
    before: Option<&str>,
    after: Option<&[u8]>,
) -> Result<()> {
    let current = root.snapshot_optional(name)?;
    let current = current.as_ref().map(|snapshot| snapshot.digest.as_str());
    let desired = after.map(digest);
    if current == desired.as_deref() {
        return Ok(());
    }
    if current != before {
        return Err("version_conflict".into());
    }
    let bytes = after.ok_or("original_configuration_operation_unavailable")?;
    match root.snapshot_optional(staged)? {
        Some(staging) if Some(staging.digest.as_str()) != desired.as_deref() => {
            return Err("precondition_failed".into());
        }
        Some(_) => {}
        None => root.stage(staged, bytes)?,
    }
    root.replace(staged, name)
}

async fn commit(
    db: &mut SqliteConnection,
    service: &Service,
    actor: &Actor,
    id: &str,
) -> Result<Value> {
    crate::host::personal::credentials::uuid(id)?;
    let key = key(actor, id);
    let mut record = read_record(db, &key).await?.ok_or("not_found")?;
    let Some(plan) = &record.plan else {
        return Ok(record.receipt);
    };
    if record.target != service.target {
        return Err("original_configuration_operation_unavailable".into());
    }
    let active: Option<String> = sqlx::query_scalar(
        "SELECT value FROM core_meta WHERE key='model-profile-active-operation'",
    )
    .fetch_optional(&mut *db)
    .await?;
    if active.as_deref() != Some(&key) {
        return Err("original_configuration_operation_unavailable".into());
    }
    let current = files::Bundle::read(&service.root, &service.filename)?;
    let desired_configuration = digest(&plan.configuration);
    let desired_secrets = plan.secrets.as_deref().map(digest);
    for (current, before, after) in [
        (
            current.configuration.as_ref().map(|s| s.digest.as_str()),
            plan.before_configuration.as_deref(),
            Some(desired_configuration.as_str()),
        ),
        (
            current.secrets.as_ref().map(|s| s.digest.as_str()),
            plan.before_secrets.as_deref(),
            desired_secrets.as_deref(),
        ),
    ] {
        if current != before && current != after {
            return Err("version_conflict".into());
        }
    }
    replace_part(
        &service.root,
        files::SECRET_FILE,
        &format!(".profile-{id}-secret.tmp"),
        plan.before_secrets.as_deref(),
        plan.secrets.as_deref(),
    )?;
    #[cfg(test)]
    crate::store::tests::commit_barrier("profile-secret-renamed").await;
    replace_part(
        &service.root,
        &service.filename,
        &format!(".profile-{id}-config.tmp"),
        plan.before_configuration.as_deref(),
        Some(&plan.configuration),
    )?;
    #[cfg(test)]
    crate::store::tests::commit_barrier("profile-config-renamed").await;
    let applied = files::Bundle::read(&service.root, &service.filename)?;
    if applied.revision != plan.revision {
        return Err("precondition_failed".into());
    }
    record.receipt["success"] = json!(true);
    record.receipt["revision"] = json!(applied.revision);
    let loaded: String =
        sqlx::query_scalar("SELECT value FROM core_meta WHERE key='model-profile-connections'")
            .fetch_one(&mut *db)
            .await?;
    let catalog = catalog::Catalog::parse(std::str::from_utf8(
        &applied
            .configuration
            .as_ref()
            .ok_or("configuration_unavailable")?
            .data,
    )?)?;
    let compiled = runtime::compile(service, &catalog, &applied.keys);
    let hot = !service.connections_suspended
        && loaded == record.connections
        && compiled.as_ref().is_ok_and(|cfg| {
            crate::memory::scope::hash(&json!(cfg))
                == crate::memory::scope::hash(&json!(record.configurations))
        });
    if hot {
        put(
            db,
            "model-profile-routes",
            &serde_json::to_string(&record.configurations)?,
        )
        .await?;
        put(
            db,
            "model-profile-current",
            &serde_json::to_string(&record.configurations["chat_agent"])?,
        )
        .await?;
        put(
            db,
            "model-profile-loaded-revision",
            &applied.revision.to_string(),
        )
        .await?;
    }
    record.receipt["effective_state"] = json!({"resource":"model_profiles","revision":applied.revision,
        "status":if hot {"applied"} else {"saved_pending_restart"},
        "apply_mode":if hot {"hot_reload"} else {"restart"}});
    record.plan = None;
    store_record(db, &key, &record).await?;
    sqlx::query("DELETE FROM core_meta WHERE key='model-profile-active-operation' AND value=?")
        .bind(key)
        .execute(db)
        .await?;
    Ok(record.receipt)
}

pub(crate) async fn apply(
    db: &mut SqliteConnection,
    service: &Service,
    request: &Request,
) -> Result<Value> {
    if matches!(request, Request::Start) {
        return start(db, service).await;
    }
    let (authority, capability) = match request {
        Request::Read(authority) | Request::Receipt(authority, _) => {
            (authority, config::READ_MODELS)
        }
        Request::Prepare(authority, _)
        | Request::Secret(authority, _)
        | Request::Commit(authority, _) => (authority, config::WRITE_MODELS),
        Request::Start => unreachable!(),
    };
    let actor = config::authorize(db, service.operators.as_deref(), authority, capability).await?;
    match request {
        Request::Read(_) => view(db, service).await,
        Request::Prepare(_, body) => prepare(db, service, actor, body, false).await,
        Request::Secret(_, body) => prepare(db, service, actor, body, true).await,
        Request::Commit(_, id) => commit(db, service, &actor, id).await,
        Request::Receipt(_, id) => {
            crate::host::personal::credentials::uuid(id)?;
            Ok(read_record(db, &key(&actor, id))
                .await?
                .ok_or("not_found")?
                .receipt)
        }
        Request::Start => unreachable!(),
    }
}

async fn put(db: &mut SqliteConnection, key: &str, value: &str) -> Result<()> {
    sqlx::query("INSERT INTO core_meta(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value,revision=core_meta.revision+1")
        .bind(key).bind(value).execute(db).await?;
    Ok(())
}

fn connection_identity(
    document: &Value,
    keys: &std::collections::BTreeMap<String, String>,
) -> String {
    let mut connections = serde_json::Map::new();
    for name in [
        "embedding",
        "vision",
        "asr",
        "search",
        "semantic_observation",
    ] {
        let connection = &document[name];
        let mut secret_hashes = serde_json::Map::new();
        for alias in files::referenced_keys(&json!({name:connection})) {
            let secret = keys
                .get(&alias)
                .cloned()
                .or_else(|| std::env::var(&alias).ok())
                .unwrap_or_default();
            secret_hashes.insert(alias, json!(digest(secret.as_bytes())));
        }
        connections.insert(name.into(), json!([connection, secret_hashes]));
    }
    crate::memory::scope::hash(&json!(connections))
}

async fn start(db: &mut SqliteConnection, service: &Service) -> Result<Value> {
    let bundle = files::Bundle::read(&service.root, &service.filename)?;
    if service
        .boot_revision
        .read()
        .map_err(|_| "configuration_unavailable")?
        .is_some_and(|revision| revision != bundle.revision)
    {
        return Err("configuration_changed_during_boot".into());
    }
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM core_meta WHERE key='model-profile-active-operation')",
    )
    .fetch_one(&mut *db)
    .await?;
    if pending {
        // A partially renamed bundle cannot become a fresh runtime profile at boot.
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM core_meta WHERE key='model-profile-current')",
        )
        .fetch_one(db)
        .await?;
        if !exists {
            return Err("original_configuration_operation_unavailable".into());
        }
        return Ok(json!({"status":"original_operation_pending"}));
    }
    let snapshot = bundle
        .configuration
        .as_ref()
        .ok_or("configuration_unavailable")?;
    let catalog = catalog::Catalog::parse(std::str::from_utf8(&snapshot.data)?)?;
    let cfg = runtime::compile(service, &catalog, &bundle.keys)?;
    put(
        db,
        "model-profile-current",
        &serde_json::to_string(&cfg["chat_agent"])?,
    )
    .await?;
    put(db, "model-profile-routes", &serde_json::to_string(&cfg)?).await?;
    put(
        db,
        "model-profile-loaded-revision",
        &bundle.revision.to_string(),
    )
    .await?;
    put(
        db,
        "model-profile-connections",
        &connection_identity(&catalog.public, &bundle.keys),
    )
    .await?;
    Ok(json!({"status":"loaded"}))
}

async fn view(db: &mut SqliteConnection, service: &Service) -> Result<Value> {
    let bundle = files::Bundle::read(&service.root, &service.filename)?;
    let metadata: Value =
        serde_json::from_str(include_str!("../../protocol/admin/model-metadata.json"))?;
    let loaded: String =
        sqlx::query_scalar("SELECT value FROM core_meta WHERE key='model-profile-loaded-revision'")
            .fetch_one(&mut *db)
            .await?;
    let mut view = json!({"file_id":"model_profiles","exists":bundle.configuration.is_some(),
        "revision":bundle.revision,"apply_mode":"hot_reload","matches_loaded":loaded == bundle.revision.to_string(),
        "semantic_matches_loaded":null,"semantic_valid":true,"semantic_providers":metadata["semantic_providers"],
        "provider_presets":metadata["provider_presets"],"task_requirements":metadata["task_requirements"],
        "writable_directory":service.root.writable()?,"valid":false,"error_category":"validation_error"});
    let parsed = bundle
        .configuration
        .as_ref()
        .ok_or("configuration_unavailable")
        .and_then(|snapshot| std::str::from_utf8(&snapshot.data).map_err(|_| "validation_error"))
        .and_then(|text| catalog::Catalog::parse(text).map_err(|_| "validation_error"));
    let Ok(catalog) = parsed else {
        return Ok(view);
    };
    if runtime::validate_profiles(&catalog, &bundle.keys).is_err() {
        return Ok(view);
    }
    let connections: String =
        sqlx::query_scalar("SELECT value FROM core_meta WHERE key='model-profile-connections'")
            .fetch_one(&mut *db)
            .await?;
    let hot = connections == connection_identity(&catalog.public, &bundle.keys);
    view["apply_mode"] = json!(if hot { "hot_reload" } else { "restart" });
    // The optional semantic observer is owned by its installed runtime.
    // No observer has reported a loaded semantic configuration to this service yet.
    view["semantic_matches_loaded"] = Value::Null;
    view["valid"] = json!(true);
    view.as_object_mut().unwrap().remove("error_category");
    view["search_backend"] = catalog
        .public
        .get("search_backend")
        .cloned()
        .unwrap_or(json!("model"));
    view["profile_schema"] = metadata["profile_schema"].clone();
    view["connection_schemas"] = metadata["connection_schemas"].clone();
    view["tasks"] = json!(catalog::TASKS);
    let saved = |profiles: &Value| -> Vec<String> {
        profiles
            .as_object()
            .into_iter()
            .flat_map(|p| p.iter())
            .filter_map(|(name, profile)| {
                profile["api_key_env"]
                    .as_str()
                    .filter(|key| bundle.keys.contains_key(*key))
                    .map(|_| name.clone())
            })
            .collect()
    };
    view["saved_api_key_profiles"] = json!(saved(&catalog.public["profiles"]));
    view["saved_semantic_keys"] = json!(saved(
        &catalog.public["semantic_observation"]["connections"]
    ));
    view["saved_connection_keys"] = json!(
        ["embedding", "vision", "asr", "search"]
            .into_iter()
            .filter(
                |name| files::referenced_keys(&json!({*name:catalog.public[*name]}))
                    .iter()
                    .any(|key| bundle.keys.contains_key(key))
            )
            .collect::<Vec<_>>()
    );
    let resolved = catalog.public["profiles"].as_object().unwrap().iter().map(|(name,profile)| {
        let profile = runtime::resolve(profile)?;
        Ok((name.clone(),json!({"base_url":profile["base_url"],"model":profile["model"],"reasoning_effort":profile["reasoning_effort"]})))
    }).collect::<Result<serde_json::Map<String,Value>>>();
    view["resolved_profiles"] = resolved.map(Value::Object).unwrap_or(Value::Null);
    view["document"] = catalog.public;
    catalog::validate("ValidModelBundle", &view)?;
    Ok(view)
}

#[derive(clap::Subcommand, Deserialize, Serialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    Read,
    Apply { body: String },
    Secret { body: String },
    Receipt { request_id: String },
}

pub(crate) async fn save(store: &Store, authority: Authority, body: Value) -> Result<Value> {
    let prepared = invoke(store, Request::Prepare(authority.clone(), body)).await?;
    let id = prepared["request_id"]
        .as_str()
        .ok_or("original_configuration_operation_unavailable")?
        .to_owned();
    #[cfg(test)]
    crate::store::tests::commit_barrier("profile-prepared-before-rename").await;
    invoke(store, Request::Commit(authority, id)).await
}

pub(crate) async fn save_secret(store: &Store, authority: Authority, body: Value) -> Result<Value> {
    let prepared = invoke(store, Request::Secret(authority.clone(), body)).await?;
    let id = prepared["request_id"]
        .as_str()
        .ok_or("original_configuration_operation_unavailable")?
        .to_owned();
    invoke(store, Request::Commit(authority, id)).await
}

pub(crate) async fn operator(store: &Store, action: Action) -> Result<Value> {
    match action {
        Action::Read => invoke(store, Request::Read(Authority::Os)).await,
        Action::Apply { body } => save(store, Authority::Os, serde_json::from_str(&body)?).await,
        Action::Secret { body } => {
            save_secret(store, Authority::Os, serde_json::from_str(&body)?).await
        }
        Action::Receipt { request_id } => {
            invoke(store, Request::Receipt(Authority::Os, request_id)).await
        }
    }
}

pub(crate) async fn invoke(store: &Store, request: Request) -> Result<Value> {
    let service = store
        .profile_files
        .as_ref()
        .ok_or("configuration_unavailable")?;
    store
        .write(Mutation::ProfileConfig(service.clone(), Box::new(request)))
        .await
}
