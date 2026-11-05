use super::{Service, catalog::Catalog};
use crate::{Result, model::wire::Protocol, runtime::main::Config, store::Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub id: String,
    pub profile: Value,
    pub credential_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_profile: Option<Box<Config>>,
}

#[derive(Serialize)]
pub(crate) struct Credentials {
    pub key: Option<String>,
    pub headers: BTreeMap<String, String>,
}

pub(super) fn resolve(profile: &Value) -> Result<Value> {
    let mut resolved = profile.clone();
    for field in ["base_url", "model"] {
        let alias = format!("{field}_env");
        if let Some(name) = profile[&alias].as_str() {
            let value = std::env::var(name)
                .ok()
                .filter(|value| !value.is_empty())
                .ok_or("profile_environment_unavailable")?;
            resolved[field] = json!(value);
        }
        resolved
            .as_object_mut()
            .ok_or("validation_error")?
            .remove(&alias);
    }
    if let Some(alias) = profile["reasoning_effort_env"].as_str()
        && let Ok(value) = std::env::var(alias)
        && !value.is_empty()
    {
        resolved["reasoning_effort"] = json!(value);
    }
    let object = resolved.as_object_mut().ok_or("validation_error")?;
    object.remove("reasoning_effort_env");
    object.entry("reasoning_effort").or_insert(json!("medium"));
    object.entry("thinking_enabled").or_insert(json!(true));
    if let Some(mode) = object.remove("thinking_mode") {
        resolved["thinking_enabled"] = match mode.as_str() {
            Some("enabled") => json!(true),
            Some("disabled") => json!(false),
            Some("configurable") => Value::Null,
            _ => return Err("validation_error".into()),
        };
    }
    Ok(resolved)
}

fn options(profile: &Value) -> Result<Value> {
    let vendor = profile["provider"]
        .as_str()
        .ok_or("validation_error")?
        .to_ascii_lowercase();
    let mut options = json!({"reasoning":"effort","token_field":"max_tokens",
        "send_temperature":false,"send_tool_choice":true,"replay_reasoning":true,
        "reasoning_split":false,"send_reasoning_effort":false,"native_web_search":false,
        "thinking_budget_tokens":4096,"include_reasoning":null,"reasoning_format":null});
    match vendor.as_str() {
        "openai" | "azure_openai" => {
            options["token_field"] = json!("max_completion_tokens");
            options["replay_reasoning"] = json!(false);
        }
        "deepseek" => {
            options["reasoning"] = json!("thinking");
            options["send_tool_choice"] = json!(false);
            options["send_reasoning_effort"] = json!(true);
        }
        "qwen" => options["reasoning"] = json!("enable_thinking"),
        "moonshot" | "zhipu" | "doubao" => {
            options["reasoning"] = json!("thinking");
            options["send_reasoning_effort"] = json!(vendor == "doubao");
        }
        "minimax" => {
            options["reasoning"] = json!("builtin");
            options["reasoning_split"] = json!(true);
        }
        "openrouter" => options["reasoning"] = json!("openrouter"),
        "groq" => {
            options["token_field"] = json!("max_completion_tokens");
            options["include_reasoning"] = json!(true);
        }
        "gemini" => options["reasoning"] = json!("gemini"),
        _ => {}
    }
    if let Some(overrides) = profile["wire_options"].as_object() {
        for (name, value) in overrides {
            options[name] = value.clone();
        }
    }
    Ok(options)
}

pub(super) fn validate_profiles(catalog: &Catalog, keys: &BTreeMap<String, String>) -> Result<()> {
    let metadata: Value =
        serde_json::from_str(include_str!("../../../protocol/admin/model-metadata.json"))?;
    let validator = jsonschema::validator_for(&metadata["profile_schema"])?;
    for (id, profile) in catalog.public["profiles"]
        .as_object()
        .ok_or("validation_error")?
    {
        let mut profile = resolve(profile)?;
        profile["id"] = json!(id);
        if !validator.is_valid(&profile) {
            return Err("validation_error".into());
        }
        let timeout = profile["timeout_seconds"]
            .as_f64()
            .ok_or("validation_error")?;
        std::time::Duration::try_from_secs_f64(timeout).map_err(|_| "validation_error")?;
        if let Some(base) = profile["base_url"]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            let url = reqwest::Url::parse(base).map_err(|_| "validation_error")?;
            if !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err("validation_error".into());
            }
        }
        if let Some(alias) = profile["api_key_env"]
            .as_str()
            .filter(|alias| !alias.is_empty())
            && keys
                .get(alias)
                .cloned()
                .or_else(|| std::env::var(alias).ok())
                .is_none_or(|secret| secret.is_empty())
        {
            return Err("provider_credential_unavailable".into());
        }
        if let Some(options) = profile["wire_options"].as_object() {
            let (allowed, modes): (&[&str], &[&str]) =
                match profile["protocol"].as_str().unwrap_or("chat_completions") {
                    "responses" => return Err("validation_error".into()),
                    "anthropic_messages" => (
                        &["reasoning", "thinking_budget_tokens"],
                        &["effort", "budget"],
                    ),
                    "gemini" => (
                        &["reasoning", "thinking_budget_tokens", "send_temperature"],
                        &["gemini", "budget"],
                    ),
                    _ => (
                        &[],
                        &[
                            "effort",
                            "thinking",
                            "enable_thinking",
                            "openrouter",
                            "builtin",
                        ],
                    ),
                };
            if !allowed.is_empty() && options.keys().any(|key| !allowed.contains(&key.as_str()))
                || options
                    .get("reasoning")
                    .and_then(Value::as_str)
                    .is_some_and(|mode| !modes.contains(&mode))
            {
                return Err("validation_error".into());
            }
        }
    }
    for (name, schema) in metadata["connection_schemas"]
        .as_object()
        .ok_or("validation_error")?
    {
        let Some(connection) = catalog.public.get(name).filter(|value| !value.is_null()) else {
            continue;
        };
        if !jsonschema::validator_for(schema)?.is_valid(connection) {
            return Err("validation_error".into());
        }
        if name != "search"
            && connection["enabled"] != false
            && (connection["base_url"].as_str().is_none_or(str::is_empty)
                || connection["model"].as_str().is_none_or(str::is_empty))
        {
            return Err("validation_error".into());
        }
    }
    Ok(())
}

pub(super) fn compile(
    service: &Service,
    catalog: &Catalog,
    keys: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, Config>> {
    validate_profiles(catalog, keys)?;
    let mut profiles = BTreeMap::new();
    for id in catalog.public["profiles"]
        .as_object()
        .ok_or("validation_error")?
        .keys()
    {
        profiles.insert(id.clone(), compile_profile(service, catalog, keys, id)?);
    }
    let original = profiles.clone();
    for (id, cfg) in &mut profiles {
        let connection = catalog.public["profiles"][id]["search_connection"]
            .as_str()
            .or_else(|| catalog.public["search_connection"].as_str());
        if let Some(name) = connection {
            cfg.profile_binding
                .as_mut()
                .ok_or("validation_error")?
                .search_profile = Some(Box::new(
                original.get(name).ok_or("validation_error")?.clone(),
            ));
        }
    }
    let mut routes = BTreeMap::new();
    for task in super::catalog::TASKS {
        let id = catalog.public["routes"][task]
            .as_str()
            .ok_or("validation_error")?;
        routes.insert(
            task.to_owned(),
            profiles.get(id).ok_or("validation_error")?.clone(),
        );
    }
    for (id, profile) in profiles {
        routes.insert(format!("profile:{id}"), profile);
    }
    Ok(routes)
}

fn compile_profile(
    service: &Service,
    catalog: &Catalog,
    keys: &BTreeMap<String, String>,
    id: &str,
) -> Result<Config> {
    let mut profile = resolve(&catalog.public["profiles"][id])?;
    let mut cfg = service.baseline.clone();
    let provider = profile["provider"]
        .as_str()
        .ok_or("validation_error")?
        .to_ascii_lowercase();
    cfg.protocol = match profile["protocol"].as_str().unwrap_or("chat_completions") {
        "chat_completions" => Protocol::Chat,
        // The fixed v4 DeepSeek adapter omits tool_choice. Other Responses
        // providers use the original standard store/include policy.
        "responses" if provider == "deepseek" => Protocol::DeepseekResponsesLegacy,
        "responses" => Protocol::OpenaiResponses,
        "anthropic_messages" => Protocol::AnthropicMessages,
        "gemini" => Protocol::Gemini,
        _ => return Err("validation_error".into()),
    };
    cfg.model = profile["model"]
        .as_str()
        .ok_or("validation_error")?
        .to_owned();
    let base = profile["base_url"]
        .as_str()
        .ok_or("validation_error")?
        .trim_end_matches('/');
    cfg.endpoint = if cfg.protocol.is_gemini() {
        format!(
            "{base}/models/{}:generateContent",
            cfg.model.strip_prefix("models/").unwrap_or(&cfg.model)
        )
    } else {
        let suffix = if cfg.protocol.is_chat() {
            "chat/completions"
        } else if cfg.protocol.is_messages() {
            "messages"
        } else {
            "responses"
        };
        format!("{base}/{suffix}")
    };
    cfg.max_output = profile["default_max_output_tokens"]
        .as_i64()
        .ok_or("validation_error")?;
    if let Some(limit) = profile["max_output_tokens_limit"].as_i64() {
        cfg.max_output = cfg.max_output.min(limit);
    }
    cfg.max_input_tokens = profile["max_input_tokens"].as_u64();
    cfg.context_window = profile["context_window_tokens"].as_u64();
    let thinking = profile["thinking_enabled"] == true;
    cfg.reasoning_effort = if thinking {
        profile["reasoning_effort"].as_str().map(str::to_owned)
    } else {
        None
    };
    cfg.messages_thinking = cfg.protocol.is_messages() && thinking;
    cfg.thinking_budget = None;
    let options = options(&profile)?;
    if cfg.messages_thinking && options["reasoning"] == "budget" {
        cfg.thinking_budget = options["thinking_budget_tokens"].as_i64();
        cfg.reasoning_effort = None;
    }
    if cfg.protocol.is_chat() {
        cfg.reasoning_effort = None;
    }
    cfg.vision_input = profile["capabilities"]
        .as_array()
        .is_some_and(|caps| caps.iter().any(|cap| cap == "image_input"));
    let native = service.allow_native.unwrap_or(true)
        && matches!(profile["search_mode"].as_str(), Some("native" | "both"));
    if native && (cfg.protocol.is_chat() || provider == "deepseek" && !cfg.protocol.is_messages()) {
        return Err("unsupported_profile_feature".into());
    }
    cfg.native_web_search = native;
    cfg.gemini_mixed_tools = cfg.protocol.is_gemini() && native;
    cfg.bridge_search = service.baseline.bridge_search
        && !matches!(profile["search_mode"].as_str(), Some("native" | "disabled"));
    profile["wire_options"] = options;
    let alias = profile["api_key_env"]
        .as_str()
        .filter(|value| !value.is_empty());
    let key = alias.and_then(|name| keys.get(name).cloned().or_else(|| std::env::var(name).ok()));
    if alias.is_some() && key.as_ref().is_none_or(|key| key.is_empty()) {
        return Err("provider_credential_unavailable".into());
    }
    let headers: BTreeMap<String, String> = catalog.raw["profiles"][id]
        .get("headers")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    for (name, value) in &headers {
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "authorization"
                | "api-key"
                | "x-api-key"
                | "x-goog-api-key"
                | "host"
                | "content-length"
        ) {
            return Err("validation_error".into());
        }
        reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|_| "validation_error")?;
        reqwest::header::HeaderValue::from_str(value).map_err(|_| "validation_error")?;
    }
    let credentials = Arc::new(Credentials { key, headers });
    let hash = super::digest(&serde_json::to_vec(&*credentials)?);
    service
        .credentials
        .write()
        .map_err(|_| "configuration_unavailable")?
        .insert(hash.clone(), credentials.clone());
    cfg.key_env = None;
    cfg.credentials = Some(credentials);
    cfg.profile_binding = Some(Binding {
        id: id.into(),
        profile,
        credential_hash: hash,
        search_profile: None,
    });
    cfg.validate()?;
    // Validate the complete provider request before accepting an editable profile.
    cfg.protocol.request(
        json!({"model":cfg.model,"max_tokens":cfg.max_output,"messages":[],"stream":false}),
        &cfg,
        None,
    )?;
    Ok(cfg)
}

pub(crate) fn hydrate(service: &Service, cfg: &mut Config) -> Result<()> {
    if let Some(binding) = &cfg.profile_binding {
        cfg.credentials = service
            .credentials
            .read()
            .map_err(|_| "configuration_unavailable")?
            .get(&binding.credential_hash)
            .cloned();
    }
    Ok(())
}

/// A task changes model selection only; the installed consumer retains its budgets,
/// network authority and tool policy. This runs before reserving the logical request.
pub(crate) async fn select(
    db: &mut SqliteConnection,
    baseline: &Config,
    task: &str,
) -> Result<Config> {
    if !super::catalog::TASKS.contains(&task) {
        return Err("unsupported_model_task".into());
    }
    let value: Option<String> =
        sqlx::query_scalar("SELECT value FROM core_meta WHERE key='model-profile-routes'")
            .fetch_optional(db)
            .await?;
    let Some(value) = value else {
        return if baseline.profile_task.is_some() {
            Err("model_profile_catalog_unavailable".into())
        } else {
            Ok(baseline.clone())
        };
    };
    let routes: BTreeMap<String, Config> = serde_json::from_str(&value)?;
    let selected = routes.get(task).ok_or("model_profile_route_unavailable")?;
    project(baseline, selected)
}

fn project(baseline: &Config, selected: &Config) -> Result<Config> {
    let mut cfg = baseline.clone();
    cfg.protocol = selected.protocol;
    cfg.endpoint = selected.endpoint.clone();
    cfg.model = selected.model.clone();
    cfg.key_env = None;
    cfg.profile_binding = selected.profile_binding.clone();
    cfg.credentials = selected.credentials.clone();
    cfg.reasoning_effort = selected.reasoning_effort.clone();
    cfg.messages_thinking = selected.messages_thinking;
    cfg.thinking_budget = selected.thinking_budget;
    cfg.vision_input = selected.vision_input;
    cfg.native_web_search = selected.native_web_search;
    cfg.gemini_mixed_tools = selected.gemini_mixed_tools;
    cfg.max_input_tokens = selected.max_input_tokens;
    cfg.context_window = selected.context_window;
    let profile = &selected
        .profile_binding
        .as_ref()
        .ok_or("model_profile_route_unavailable")?
        .profile;
    cfg.max_output = profile["default_max_output_tokens"]
        .as_i64()
        .unwrap_or(baseline.max_output)
        .min(baseline.max_output);
    if let Some(limit) = profile["max_output_tokens_limit"].as_i64() {
        cfg.max_output = cfg.max_output.min(limit);
    }
    cfg.validate()?;
    Ok(cfg)
}

pub(crate) async fn search(
    store: &Store,
    execution: &str,
    baseline: &crate::model::search::Config,
) -> Result<crate::model::search::Config> {
    let mut cfg = baseline.clone();
    if store.profile_files.is_none() {
        return Ok(cfg);
    }
    let main = current(store).await?.ok_or("main_profile_unavailable")?;
    let main = original(store, execution, main).await?;
    let selected = main
        .profile_binding
        .as_ref()
        .and_then(|b| b.search_profile.as_deref());
    if let Some(selected) = selected {
        let policy = baseline
            .connection
            .as_ref()
            .ok_or("independent_search_policy_unavailable")?;
        let mut profile = project(policy, selected)?;
        profile.native_web_search = true;
        profile.gemini_mixed_tools = false;
        let binding = profile
            .profile_binding
            .as_ref()
            .ok_or("search_profile_unavailable")?;
        if !binding.profile["capabilities"]
            .as_array()
            .is_some_and(|caps| caps.iter().any(|c| c == "native_web_search"))
        {
            return Err("search_profile_capability_unavailable".into());
        }
        cfg.connection_id = Some(binding.id.clone());
        cfg.credential_namespace = Some(binding.credential_hash.clone());
        ready(store, &mut profile)?;
        cfg.connection = Some(profile);
        cfg.validate()?;
    }
    Ok(cfg)
}

pub(crate) fn structured_mode(
    cfg: &Config,
    default: &crate::model::chat::Mode,
) -> Result<crate::model::chat::Mode> {
    match cfg
        .profile_binding
        .as_ref()
        .and_then(|b| b.profile.get("structured_output_mode"))
    {
        Some(value) if !value.is_null() => Ok(serde_json::from_value(value.clone())?),
        _ => Ok(default.clone()),
    }
}

pub(crate) fn ready(store: &Store, cfg: &mut Config) -> Result<()> {
    if let Some(service) = &store.profile_files {
        hydrate(service, cfg)?;
    }
    if cfg.profile_binding.is_some() && cfg.credentials.is_none() {
        return Err("original_profile_credentials_unavailable".into());
    }
    Ok(())
}

pub(crate) async fn current(store: &Store) -> Result<Option<Arc<Config>>> {
    let Some(service) = &store.profile_files else {
        return Ok(store.main.clone());
    };
    let value: String =
        sqlx::query_scalar("SELECT value FROM core_meta WHERE key='model-profile-current'")
            .fetch_one(store.readers())
            .await?;
    let mut cfg: Config = serde_json::from_str(&value)?;
    hydrate(service, &mut cfg)?;
    Ok(Some(Arc::new(cfg)))
}

pub(crate) async fn pin(db: &mut SqliteConnection, execution: &str, cfg: &Config) -> Result<()> {
    let key = format!("model-profile-contract:{}", cfg.contract());
    sqlx::query("INSERT INTO core_meta(key,value) VALUES(?,?) ON CONFLICT(key) DO NOTHING")
        .bind(key)
        .bind(serde_json::to_string(cfg)?)
        .execute(&mut *db)
        .await?;
    let key = format!("model-profile-execution:{execution}");
    let original: String = sqlx::query_scalar("INSERT INTO core_meta(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=core_meta.value RETURNING value")
        .bind(key).bind(cfg.contract()).fetch_one(db).await?;
    if original != cfg.contract() {
        return Err("original_profile_unavailable".into());
    }
    Ok(())
}

pub(crate) async fn original(
    store: &Store,
    execution: &str,
    fallback: Arc<Config>,
) -> Result<Arc<Config>> {
    let contract: Option<String> = sqlx::query_scalar("SELECT value FROM core_meta WHERE key=?1 UNION ALL SELECT profile_contract FROM activations WHERE id=?2 LIMIT 1")
        .bind(format!("model-profile-execution:{execution}")).bind(format!("main:{execution}")).fetch_optional(store.readers()).await?;
    let Some(contract) = contract else {
        let mut cfg = (*fallback).clone();
        if let Some(service) = &store.profile_files {
            hydrate(service, &mut cfg)?;
        }
        return Ok(Arc::new(cfg));
    };
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM core_meta WHERE key=?")
        .bind(format!("model-profile-contract:{contract}"))
        .fetch_optional(store.readers())
        .await?;
    let Some(value) = value else {
        return if fallback.contract() == contract {
            Ok(fallback)
        } else {
            Err("original_profile_unavailable".into())
        };
    };
    let mut cfg: Config = serde_json::from_str(&value)?;
    if cfg.contract() != contract {
        return Err("original_profile_unavailable".into());
    }
    if let Some(service) = &store.profile_files {
        hydrate(service, &mut cfg)?;
    }
    Ok(Arc::new(cfg))
}
