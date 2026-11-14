//! Shared management domain. Adapters supply authenticated authority, never roles.
mod bindings;
mod character;
mod health;
pub(crate) mod http;
mod identity;
mod plugins;
mod presences;
pub(crate) mod receipts;
mod soul;
pub(crate) mod trace;
mod usage;

use super::config::{self, Actor, Authority};
use crate::{
    Result,
    media::blob::Root,
    store::{Mutation, Store},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

static OPENAPI: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../protocol/baseline-v4/admin-openapi.json"
    ))
    .expect("fixed Admin DTOs")
});

pub(crate) struct Service {
    root: Root,
    operators: Option<PathBuf>,
    name: String,
    aliases: Vec<String>,
    workers: health::Workers,
}
impl Service {
    pub(crate) fn open(
        data: &Path,
        operators: Option<&Path>,
        name: &str,
        aliases: &[String],
    ) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            root: Root::open(data)?,
            operators: operators.map(|path| {
                if path.is_absolute() {
                    path.to_owned()
                } else {
                    data.join(path)
                }
            }),
            name: name.into(),
            aliases: aliases
                .iter()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect(),
            workers: Default::default(),
        }))
    }
}

#[derive(Clone)]
pub(crate) enum Request {
    Start,
    Read(Authority, String, Value),
    Write(Authority, String, Value),
    CommitSoul(Authority, String),
    PluginSettle(Authority, String, Value, bool),
    PublishWork(Authority, Vec<String>),
    Discover(Authority, Value, Vec<crate::host::Install>),
    Discovered(Authority, Value),
    DiscoverAuth(Authority, Value),
    Failed(Authority, String, Value, String),
}

fn validate(name: &str, value: &Value) -> Result<()> {
    let schema =
        json!({"components":OPENAPI["components"],"$ref":format!("#/components/schemas/{name}")});
    if !jsonschema::validator_for(&schema)?.is_valid(value) {
        return Err(if name.ends_with("Body") || name == "IdentityRequest" {
            "invalid_command_body"
        } else {
            "validation_error"
        }
        .into());
    }
    Ok(())
}
fn operation(body: &Value) -> Result<String> {
    let id = body["request_id"]
        .as_str()
        .ok_or("validation_error")?
        .to_ascii_lowercase();
    crate::host::personal::credentials::uuid(&id)?;
    Ok(id)
}
fn receipt_key(actor: &Actor, id: &str) -> String {
    format!(
        "control-receipt:{}",
        crate::memory::scope::hash(&json!([actor.namespace, id]))
    )
}
fn command_digest(method: &str, body: &Value) -> Result<String> {
    let mut canonical = body.clone();
    canonical["request_id"] = json!(operation(body)?);
    Ok(crate::memory::scope::hash(&json!([method, canonical])))
}
pub(crate) async fn get(db: &mut SqliteConnection, key: &str) -> Result<Option<Value>> {
    let raw: Option<String> = sqlx::query_scalar("SELECT value FROM core_meta WHERE key=?")
        .bind(key)
        .fetch_optional(db)
        .await?;
    raw.map(|s| serde_json::from_str(&s).map_err(Into::into))
        .transpose()
}
pub(crate) async fn put(db: &mut SqliteConnection, key: &str, value: &Value) -> Result<()> {
    sqlx::query("INSERT INTO core_meta(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value,revision=core_meta.revision+1")
        .bind(key).bind(value.to_string()).execute(db).await?;
    Ok(())
}
async fn previous(
    db: &mut SqliteConnection,
    actor: &Actor,
    method: &str,
    body: &Value,
) -> Result<Option<Value>> {
    let id = operation(body)?;
    if let Some(record) = get(db, &receipt_key(actor, &id)).await? {
        if record["digest"] != command_digest(method, body)? {
            return Err("request_conflict".into());
        }
        return Ok(Some(record));
    }
    Ok(None)
}
async fn record(
    db: &mut SqliteConnection,
    actor: &Actor,
    method: &str,
    body: &Value,
    result: &Value,
) -> Result<()> {
    let id = operation(body)?;
    let now =
        crate::host::personal::credentials::stamp(crate::host::personal::credentials::now()?)?;
    let principal = receipts::principal(db, actor).await?;
    let mut targets = Vec::<String>::new();
    for value in [
        body.get("id"),
        body.get("to_person"),
        result.get("resource_id"),
        result.get("person_id"),
        result.get("binding_id"),
    ] {
        if let Some(id) = value.and_then(Value::as_str) {
            let owners:Vec<String>=sqlx::query_scalar("SELECT id FROM persons WHERE id=?1 UNION SELECT original_person FROM assignments WHERE id=?1 AND original_person IS NOT NULL UNION SELECT original_person FROM device_origins WHERE device_id=?1 UNION SELECT p.id FROM work_nodes w JOIN persons p ON p.id=w.principal WHERE w.id=?1")
                .bind(id).fetch_all(&mut *db).await?;
            targets.extend(owners);
        }
    }
    targets.sort();
    targets.dedup();
    let prior = get(db, &receipt_key(actor, &id)).await?;
    let created = prior
        .as_ref()
        .and_then(|v| v["created_at"].as_str())
        .unwrap_or(&now);
    put(db,&receipt_key(actor,&id),&json!({"method":method,"digest":command_digest(method,body)?,
        "request_id":id,"principal":actor.namespace,"principal_id":principal,"source_person":actor.source_person,
        "target_people":targets,"target_owner":result.get("owner_id").or_else(||result.get("person_id")).or_else(||body.get("id")),"status":"succeeded","result":result,"created_at":created,"updated_at":now,
        "expected_revision":body["expected_revision"],"result_resource_id":result.get("resource_id").or_else(||result.get("id")).or_else(||result.get("plugin_id")).or_else(||result.get("person_id")).or_else(||result.get("device_id")).or_else(||result.get("binding_id")),
        "result_revision":result["revision"]})).await
}

pub(crate) async fn model_prepared(
    db: &mut SqliteConnection,
    actor: &Actor,
    method: &str,
    body: &Value,
    result: &Value,
) -> Result<()> {
    previous(db, actor, method, body).await?;
    record(db, actor, method, body, result).await?;
    let key = receipt_key(actor, &operation(body)?);
    let mut saved = get(db, &key).await?.ok_or("not_found")?;
    saved["status"] = json!("pending");
    put(db, &key, &saved).await
}

pub(crate) async fn model_settled(
    db: &mut SqliteConnection,
    actor: &Actor,
    id: &str,
    result: &Value,
) -> Result<()> {
    let key = receipt_key(actor, id);
    let Some(mut saved) = get(db, &key).await? else {
        // Older retained profile operations are readable through the original
        // record; they have no observed control timestamps to backfill.
        return Ok(());
    };
    saved["status"] = json!("succeeded");
    saved["result"] = result.clone();
    saved["result_resource_id"] = result["resource_id"].clone();
    saved["result_revision"] = result["revision"].clone();
    saved["updated_at"] = json!(crate::host::personal::credentials::stamp(
        crate::host::personal::credentials::now()?
    )?);
    put(db, &key, &saved).await
}

pub(crate) async fn apply(
    db: &mut SqliteConnection,
    service: &Service,
    request: &Request,
) -> Result<Value> {
    if matches!(request, Request::Start) {
        character::start(db, service).await?;
        return soul::start(db, service).await;
    }
    let (authority, method) = match request {
        Request::Read(a, m, _)
        | Request::Write(a, m, _)
        | Request::PluginSettle(a, m, _, _)
        | Request::Failed(a, m, _, _) => (a, m.as_str()),
        Request::CommitSoul(a, _) => (a, "character.soul.save"),
        Request::PublishWork(a, _) => (a, "work.list"),
        Request::Discover(a, _, _) | Request::Discovered(a, _) | Request::DiscoverAuth(a, _) => {
            (a, "plugin.discover")
        }
        Request::Start => unreachable!(),
    };
    let capability = match method {
        "character.get" | "character.soul.read" => "control.character.read",
        "character.public" | "character.identity" => "control.preference.read",
        "character.update" | "character.soul.save" | "character.soul.reload" => {
            "control.character.mutate"
        }
        "operation.get" => "control.operation.read",
        "trace.read" | "trace.export" => "control.execution.metadata.read",
        "plugin.configuration.read" => "control.plugin.config.content.read",
        "plugin.configuration.save" => "control.plugin.config.mutate",
        "plugin.observe" => "control.plugin.config.content.read",
        "plugin.list" | "plugin.get" | "plugin.outbox.get" => "control.plugin.read",
        "plugin.enable"
        | "plugin.disable"
        | "plugin.approve"
        | "plugin.doctor"
        | "plugin.discover"
        | "plugin.outbox.retry" => "control.plugin.mutate",
        "model.stats" => config::READ_MODELS,
        "health.read" => "control.health.read",
        "work.list" | "work.get" => "control.work.read",
        "work.cancel" | "work.resume" => "control.work.mutate",
        _ => identity::capability(method)
            .or_else(|| bindings::capability(method))
            .or_else(|| presences::capability(method))
            .ok_or("unsupported_control_method")?,
    };
    let actor = config::authorize(db, service.operators.as_deref(), authority, capability).await?;
    if let Request::Read(_, method, args) = request
        && method == "binding.list"
        && (args["platform"].is_string() || args["external_account_id"].is_string())
    {
        config::authorize(
            db,
            service.operators.as_deref(),
            authority,
            "identity.binding.read_external",
        )
        .await?;
    }
    if let Request::Read(_, method, args) = request
        && matches!(method.as_str(), "trace.read" | "trace.export")
        && args["content"] == true
    {
        config::authorize(
            db,
            service.operators.as_deref(),
            authority,
            "control.execution.content.read",
        )
        .await?;
    }
    let command = match request {
        Request::Write(_, method, body) | Request::Failed(_, method, body, _) => {
            Some((method.as_str(), body))
        }
        Request::Discover(_, body, _) => Some(("plugin.discover", body)),
        _ => None,
    };
    if let Some((method, body)) = command
        && let Some(old) = receipts::begin(db, &actor, method, body).await?
    {
        return Ok(old);
    }
    let result = async {
        match request {
            Request::Failed(_, _, _, reason) => Err(reason.clone().into()),
            Request::Read(_, m, args) => match m.as_str() {
                "model.stats" | "health.read" | "work.list" | "work.get" => {
                    Ok(json!({"authorized":true}))
                }
                "trace.read" => trace::read(db, args, authority).await,
                "trace.export" => trace::export(db, args, authority).await,
                "plugin.configuration.read" => crate::host::configuration::read(db, args).await,
                "plugin.list" | "plugin.get" => crate::host::configuration::status(db, args).await,
                "plugin.observe" => crate::host::configuration::observation(db, args).await,
                "plugin.outbox.get" => {
                    crate::host::notification::retry::read(db, plugins::outbox_id(args)?).await
                }
                "character.get" | "character.public" => character::read(db).await,
                "character.identity" => character::identity(db, service).await,
                "character.soul.read" => soul::read(db, service).await,
                "operation.get" => receipts::read(db, &actor, args).await,
                "person.list" | "person.get" => identity::read(db, m, args).await,
                "binding.list" | "binding.get" => bindings::read(db, m, args).await,
                "presence.list" | "presence.get" => presences::read(db, m, args).await,
                _ => Err("unsupported_control_method".into()),
            },
            Request::Write(_, m, body) => match m.as_str() {
                "plugin.outbox.retry" => plugins::retry_outbox(db, &actor, body).await,
                "plugin.enable" | "plugin.disable" | "plugin.approve" | "plugin.doctor" => {
                    plugins::prepare(db, &actor, m, body).await
                }
                "work.cancel" | "work.resume" => {
                    identity::dto(body, "CommandBody")?;
                    if let Some(old) = previous(db, &actor, m, body).await? {
                        return Ok(old["result"].clone());
                    }
                    let result = crate::work::management::mutate(
                        db,
                        &identity::id(body)?,
                        m,
                        &operation(body)?,
                        body["expected_revision"]
                            .as_i64()
                            .ok_or("validation_error")?,
                    )
                    .await?;
                    record(db, &actor, m, body, &result).await?;
                    Ok(result)
                }
                "plugin.configuration.save" => {
                    let mut dto = body.clone();
                    dto.as_object_mut()
                        .ok_or("validation_error")?
                        .remove("plugin_id");
                    validate("ConfigBody", &dto)?;
                    if let Some(old) = previous(db, &actor, m, body).await? {
                        return Ok(old["result"].clone());
                    }
                    let result = crate::host::configuration::save(db, body).await?;
                    record(db, &actor, m, body, &result).await?;
                    Ok(result)
                }
                "character.update" => character::update(db, &actor, body).await,
                "character.soul.save" => soul::prepare(db, service, &actor, body).await,
                "character.soul.reload" => soul::reload(db, service, &actor, body).await,
                "person.create"
                | "person.rename"
                | "person.merge"
                | "person.enable"
                | "person.disable"
                | "person.superuser.grant"
                | "person.superuser.revoke"
                | "person.forget"
                | "person.client-pair"
                | "device.revoke" => identity::write(db, &actor, m, body).await,
                "binding.attach" | "binding.enable" | "binding.disable" | "binding.unbind"
                | "binding.transfer" => bindings::write(db, &actor, m, body).await,
                "presence.register"
                | "presence.state"
                | "presence.unregister"
                | "presence.transfer" => presences::write(db, &actor, m, body).await,
                _ => Err("unsupported_control_method".into()),
            },
            Request::CommitSoul(_, id) => soul::commit(db, service, &actor, id).await,
            Request::PluginSettle(_, m, body, failed) => {
                plugins::settle(db, &actor, m, body, *failed).await
            }
            Request::PublishWork(_, ids) => {
                crate::work::management::publishable(db, ids).await?;
                Ok(Value::Null)
            }
            Request::Discover(_, body, installs) => {
                plugins::discover(db, &actor, body, installs).await
            }
            Request::Discovered(_, body) => plugins::discovered(db, &actor, body).await,
            Request::DiscoverAuth(_, body) => plugins::discovery_request(db, &actor, body).await,
            Request::Start => unreachable!(),
        }
    }
    .await;
    if let Some((method, body)) = command {
        return receipts::finish(db, &actor, method, body, result).await;
    }
    result
}

pub(crate) async fn invoke(store: &Store, request: Request) -> Result<Value> {
    if let Request::Read(authority, method, _) = &request
        && method == "model.read"
    {
        return crate::model::profile_store::invoke(
            store,
            crate::model::profile_store::Request::Read(authority.clone()),
        )
        .await;
    }
    let service = store.control.as_ref().ok_or("control_unavailable")?;
    let observation = match &request {
        Request::Read(_, method, args) if method == "plugin.observe" => Some(args.clone()),
        _ => None,
    };
    let result = store
        .write(Mutation::Control(
            service.clone(),
            Box::new(request.clone()),
        ))
        .await?;
    let result = receipts::outcome(result)?;
    if let Request::Read(authority, method, args) = &request
        && matches!(
            method.as_str(),
            "model.stats" | "health.read" | "work.list" | "work.get"
        )
    {
        let value = if method == "health.read" {
            health::read(store, service).await?
        } else {
            let mut tx = store.readers().begin().await?;
            if method == "model.stats" {
                usage::read(&mut tx, args).await?
            } else {
                crate::work::management::read(&mut tx, method, args).await?
            }
        };
        // The aggregate uses a reader snapshot; revoke still wins before publication.
        let publication = if matches!(method.as_str(), "work.list" | "work.get") {
            let ids = if method == "work.get" {
                vec![value["id"].as_str().ok_or("state_mismatch")?.to_owned()]
            } else {
                value["items"]
                    .as_array()
                    .ok_or("state_mismatch")?
                    .iter()
                    .map(|v| {
                        v["id"]
                            .as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| "state_mismatch".into())
                    })
                    .collect::<Result<Vec<_>>>()?
            };
            Request::PublishWork(authority.clone(), ids)
        } else {
            request
        };
        store
            .write(Mutation::Control(service.clone(), Box::new(publication)))
            .await?;
        return Ok(value);
    }
    if let Some(args) = observation {
        let hub = store.host.as_ref().ok_or("observation_unavailable")?;
        let output = hub
            .observe(
                store,
                &args,
                result["generation"]
                    .as_str()
                    .ok_or("observation_unavailable")?,
            )
            .await?;
        let current = store
            .write(Mutation::Control(service.clone(), Box::new(request)))
            .await?;
        if current != result {
            return Err("observation_changed".into());
        }
        return Ok(output);
    }
    Ok(result)
}
pub(crate) fn save(
    store: &Store,
    authority: Authority,
    method: String,
    body: Value,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value>> + Send + '_>> {
    Box::pin(async move {
        if method == "plugin.discover" {
            return store
                .host
                .as_ref()
                .ok_or("control_unavailable")?
                .discover(store, authority, body)
                .await;
        }
        if method == "model.apply" {
            return crate::model::profile_store::save(store, authority, body).await;
        }
        if method == "model.secret" {
            return crate::model::profile_store::save_secret(store, authority, body).await;
        }
        if matches!(method.as_str(), "model.test" | "model.discover") {
            return crate::model::profile_store::probe::run(store, authority, &method, &body).await;
        }
        let value = invoke(
            store,
            Request::Write(authority.clone(), method.clone(), body.clone()),
        )
        .await?;
        if matches!(
            method.as_str(),
            "plugin.enable" | "plugin.disable" | "plugin.approve" | "plugin.doctor"
        ) {
            let value = if value["prepared"] == true {
                #[cfg(test)]
                crate::store::tests::commit_barrier("plugin-management-prepared").await;
                let failed = if let Some(hub) = &store.host {
                    hub.reconcile(
                        store,
                        body["plugin_id"].as_str().ok_or("validation_error")?,
                        false,
                    )
                    .await
                    .is_err()
                } else {
                    true
                };
                invoke(
                    store,
                    Request::PluginSettle(authority, method, body, failed),
                )
                .await?
            } else {
                value
            };
            if let Some(error) = value["error_category"].as_str() {
                return Err(error.to_owned().into());
            }
            return Ok(value);
        }
        if method == "character.soul.save" && value["prepared"] == true {
            #[cfg(test)]
            crate::store::tests::commit_barrier("soul-prepared-before-rename").await;
            return invoke(
                store,
                Request::CommitSoul(
                    authority,
                    value["request_id"]
                        .as_str()
                        .ok_or("missing_request")?
                        .into(),
                ),
            )
            .await;
        }
        Ok(value)
    })
}
#[derive(clap::Subcommand, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Action {
    Read {
        method: String,
        #[arg(default_value = "{}")]
        args: String,
    },
    Write {
        method: String,
        body: String,
    },
}
pub(crate) async fn operator(store: &Store, action: Action) -> Result<Value> {
    match action {
        Action::Read { method, args } => {
            invoke(
                store,
                Request::Read(Authority::Os, method, serde_json::from_str(&args)?),
            )
            .await
        }
        Action::Write { method, body } => {
            save(store, Authority::Os, method, serde_json::from_str(&body)?).await
        }
    }
}

/// The private stream exposes only SOUL revisions, and only to the original
/// superuser. Neither the document nor its management record enters a frame.
pub(crate) async fn personal_soul_revisions(
    db: &mut SqliteConnection,
    service: &Service,
    source: &str,
) -> Result<Option<Value>> {
    if !sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM persons WHERE id=? AND enabled AND admin)",
    )
    .bind(source)
    .fetch_one(&mut *db)
    .await?
        || get(db, "character-soul-active").await?.is_none()
    {
        return Ok(None);
    }
    let current = soul::read(db, service).await?;
    Ok(Some(
        json!({"revision":current["revision"],"active_revision":current["active_revision"]}),
    ))
}
pub(crate) async fn current_name(db: &mut SqliteConnection) -> Result<Option<String>> {
    if get(db, "character-name-managed").await?.is_none() {
        return Ok(None);
    }
    Ok(get(db, "character-profile")
        .await?
        .and_then(|v| v["display_name"].as_str().map(str::to_owned)))
}
pub(crate) async fn original_soul(
    db: &mut SqliteConnection,
    execution: &str,
    fallback: &str,
) -> Result<String> {
    let key = format!("character-soul-execution:{execution}");
    if let Some(value) = get(db, &key).await? {
        return Ok(value["content"]
            .as_str()
            .ok_or("original_soul_unavailable")?
            .into());
    }
    let value = get(db, "character-soul-active")
        .await?
        .unwrap_or(json!({"content":fallback}));
    put(db, &key, &value).await?;
    Ok(value["content"].as_str().ok_or("soul_unavailable")?.into())
}
