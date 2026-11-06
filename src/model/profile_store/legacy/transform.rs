use super::super::catalog::{self, Catalog};
use super::{RowValue, digest};
use crate::Result;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const PARAMETERS: [(&str, &str, &str); 3] = [
    ("llm.temperature", "default_temperature", "LLM_TEMPERATURE"),
    (
        "llm.max_output_tokens",
        "default_max_output_tokens",
        "LLM_MAX_OUTPUT_TOKENS",
    ),
    (
        "llm.thinking_enabled",
        "thinking_enabled",
        "LLM_THINKING_ENABLED",
    ),
];
const RESTART: [(&str, &str); 9] = [
    ("llm.model", "LLM_MODEL"),
    ("memory.embedding_enabled", "MEMORY_EMBEDDING_ENABLED"),
    ("vision.enabled", "VISION_ENABLED"),
    ("vision.base_url", "VISION_BASE_URL"),
    ("vision.model", "VISION_MODEL"),
    ("asr.enabled", "ASR_ENABLED"),
    ("asr.base_url", "ASR_BASE_URL"),
    ("asr.model", "ASR_MODEL"),
    ("asr.protocol", "ASR_PROTOCOL"),
];
const AGENTS: [&str; 2] = ["chat_agent", "plugin_agent_session"];
pub(super) fn retired(key: &str) -> bool {
    key == "llm.model" || PARAMETERS.iter().any(|v| v.0 == key)
}
pub(super) fn relevant(key: &str) -> bool {
    retired(key) || RESTART.iter().any(|v| v.0 == key)
}
pub(super) struct Converted {
    pub document: Value,
    pub keys: Value,
    pub consumed: Vec<String>,
    pub migrated: Vec<String>,
    pub comparison: Value,
    pub environment_sources: BTreeMap<String, String>,
    environment: BTreeMap<String, Value>,
}
fn dotenv(raw: Option<&[u8]>) -> Result<BTreeMap<String, Value>> {
    let mut values = BTreeMap::new();
    for line in std::str::from_utf8(raw.unwrap_or_default())
        .map_err(|_| "invalid_dotenv")?
        .lines()
    {
        let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, raw) = line.split_once('=').ok_or("unsupported_dotenv_syntax")?;
        let key = key.trim().to_ascii_uppercase();
        if key.is_empty()
            || !key
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err("invalid_dotenv_name".into());
        }
        let raw = raw.trim();
        // Reject interpolation rather than silently resolving it with this process's secrets.
        if raw.contains('$') || raw.contains('`') {
            return Err("unsupported_dotenv_interpolation".into());
        }
        let value = if raw.starts_with('"') {
            serde_json::from_str::<String>(raw).map_err(|_| "unsupported_dotenv_quotes")?
        } else if raw.starts_with('\'') {
            raw.strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
                .ok_or("unsupported_dotenv_quotes")?
                .into()
        } else {
            raw.split(" #").next().unwrap_or("").trim().into()
        };
        values.insert(key, json!(value));
    }
    Ok(values)
}
fn typed(value: &Value, default: &Value, name: &str) -> Result<Value> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    if let Some(raw) = value.as_str() {
        if default.is_boolean() || name == "LLM_THINKING_ENABLED" {
            return match raw.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => Ok(json!(true)),
                "false" | "0" | "no" | "off" => Ok(json!(false)),
                _ => Err("invalid_legacy_boolean".into()),
            };
        }
        if default.is_number() || matches!(name, "LLM_TEMPERATURE" | "LLM_MAX_OUTPUT_TOKENS") {
            return serde_json::from_str(raw).map_err(|_| "invalid_legacy_number".into());
        }
    }
    Ok(value.clone())
}
fn text<'a>(env: &'a BTreeMap<String, Value>, key: &str) -> Result<&'a str> {
    env.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| "invalid_legacy_setting_type".into())
}
fn enabled(env: &BTreeMap<String, Value>, key: &str) -> Result<bool> {
    env.get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| "invalid_legacy_setting_type".into())
}
fn alias(
    keys: &mut BTreeMap<String, String>,
    updates: &mut serde_json::Map<String, Value>,
    name: &str,
    value: &str,
) -> String {
    if value.is_empty() {
        return String::new();
    }
    let base = format!("YUKI_LEGACY_{}_KEY", name.to_ascii_uppercase());
    let mut selected = base.clone();
    let mut index = 2;
    while keys.get(&selected).is_some_and(|s| s != value) {
        selected = format!("{base}_{index}");
        index += 1;
    }
    if !keys.contains_key(&selected) {
        keys.insert(selected.clone(), value.into());
        updates.insert(selected.clone(), json!(value));
    }
    selected
}
fn resolve(profile: &Value, env: &BTreeMap<String, Value>) -> Result<Value> {
    let mut p = profile.clone();
    for field in ["base_url", "model", "reasoning_effort"] {
        let alias = format!("{field}_env");
        if let Some(name) = p[&alias].as_str() {
            let value = env
                .get(name)
                .filter(|v| !v.is_null() && v.as_str() != Some(""));
            if let Some(value) = value {
                p[field] = value.clone();
            } else if field != "reasoning_effort" {
                return Err("profile_environment_unavailable".into());
            }
        }
        p.as_object_mut().ok_or("invalid_profile")?.remove(&alias);
    }
    let object = p.as_object_mut().ok_or("invalid_profile")?;
    object.entry("thinking_enabled").or_insert(json!(true));
    object.entry("reasoning_effort").or_insert(json!("medium"));
    if let Some(mode) = object.remove("thinking_mode") {
        p["thinking_enabled"] = match mode.as_str() {
            Some("enabled") => json!(true),
            Some("disabled") => json!(false),
            Some("configurable") => Value::Null,
            _ => return Err("invalid_thinking_mode".into()),
        };
    }
    Ok(p)
}
fn parameters(values: &BTreeMap<String, Value>) -> Result<()> {
    for (key, _, _) in PARAMETERS {
        if let Some(v) = values.get(key) {
            let valid = match key {
                "llm.temperature" => v.as_f64().is_some_and(|n| (0.0..=2.0).contains(&n)),
                "llm.max_output_tokens" => v.as_i64().is_some_and(|n| n > 0),
                _ => v.is_boolean(),
            };
            if !valid {
                return Err("invalid_legacy_parameter".into());
            }
        }
    }
    Ok(())
}
fn compare(
    before: &Value,
    after: &Value,
    env: &BTreeMap<String, Value>,
    values: &BTreeMap<String, Value>,
) -> Result<Value> {
    let mut result = serde_json::Map::new();
    for task in catalog::TASKS {
        let original = before["routes"][task].as_str().ok_or("route_missing")?;
        let selected = after["routes"][task].as_str().ok_or("route_missing")?;
        let mut expected = resolve(&before["profiles"][original], env)?;
        if before["profiles"][original]["model_env"] == "LLM_MODEL"
            && let Some(value) = values.get("llm.model")
        {
            expected["model"] = value.clone();
        }
        if AGENTS.contains(&task) {
            for (key, field, _) in PARAMETERS {
                if let Some(value) = values.get(key) {
                    expected[field] = value.clone();
                }
            }
        }
        let actual = resolve(&after["profiles"][selected], env)?;
        if expected != actual {
            return Err("effective_parameters_changed".into());
        }
        // Hash full resolved material (including private headers), expose no credentials.
        result.insert(task.into(),json!({"before_profile":original,"after_profile":selected,"equal":true,"effective_sha256":digest(actual.to_string().as_bytes()),"default_temperature":actual["default_temperature"],"default_max_output_tokens":actual["default_max_output_tokens"],"thinking_enabled":actual["thinking_enabled"]}));
    }
    Ok(Value::Object(result))
}
pub(super) fn convert(
    original: &Catalog,
    saved_keys: &BTreeMap<String, String>,
    rows: &[RowValue],
    environment: &[u8],
    dotenv_bytes: Option<&[u8]>,
) -> Result<Converted> {
    let mut env: BTreeMap<String, Value> = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/import/model-v4/settings-defaults.json"
    ))?;
    let defaults = env.clone();
    let mut explicit = dotenv(dotenv_bytes)?;
    let mut environment_sources: BTreeMap<String, String> = explicit
        .keys()
        .map(|k| (k.clone(), "dotenv".into()))
        .collect();
    let process: BTreeMap<String, Value> =
        serde_json::from_slice(environment).map_err(|_| "invalid_environment_capture")?;
    for (key, value) in process {
        environment_sources.insert(key.to_ascii_uppercase(), "explicit_process_capture".into());
        explicit.insert(key.to_ascii_uppercase(), value);
    }
    for (key, value) in &explicit {
        env.insert(
            key.clone(),
            typed(value, defaults.get(key).unwrap_or(&Value::Null), key)?,
        );
    }
    let mut values = BTreeMap::new();
    let mut consumed = BTreeSet::new();
    for (key, _, name) in PARAMETERS {
        if explicit.contains_key(name) && !env[name].is_null() {
            values.insert(key.into(), env[name].clone());
            consumed.insert(name.into());
        }
    }
    let mut seen = BTreeSet::new();
    for row in rows {
        if !row.value.is_null() && row.fact.scope != "global" {
            return Err("scoped_model_overrides".into());
        }
        if row.value.is_null() {
            continue;
        }
        if !seen.insert(&row.fact.key) {
            return Err("duplicate_legacy_override".into());
        }
        if retired(&row.fact.key) {
            values.insert(row.fact.key.clone(), row.value.clone());
        }
        if let Some((_, name)) = RESTART.iter().find(|r| r.0 == row.fact.key) {
            if row.fact.mode != "restart_required" && row.fact.key != "llm.model" {
                return Err("invalid_restart_override".into());
            }
            env.insert((*name).into(), row.value.clone());
        }
    }
    parameters(&values)?;
    if values
        .get("llm.model")
        .is_some_and(|v| !v.is_string() || v.as_str() == Some(""))
    {
        return Err("invalid_legacy_model".into());
    }
    let mut document = original.public.clone();
    // Public writer preserves private headers from the source, but copies must retain them too.
    let before = original.raw.clone();
    document["profiles"] = before["profiles"].clone();
    let mut keys = saved_keys.clone();
    let mut updates = serde_json::Map::new();
    let mut migrated = vec![];
    if document.get("embedding").is_none_or(Value::is_null) {
        let url = text(&env, "MEMORY_EMBEDDING_BASE_URL")?;
        let key = text(&env, "MEMORY_EMBEDDING_API_KEY")?;
        document["embedding"] = json!({"enabled":enabled(&env,"MEMORY_EMBEDDING_ENABLED")?&&!url.trim().is_empty()&&!key.trim().is_empty(),"provider":env["MEMORY_EMBEDDING_PROVIDER"],"base_url":url,"model":env["MEMORY_EMBEDDING_MODEL"],"api_key_env":alias(&mut keys,&mut updates,"embedding",key),"dimensions":env["MEMORY_EMBEDDING_DIMENSIONS"],"output_type":env["MEMORY_EMBEDDING_OUTPUT_TYPE"],"document_template_version":env["MEMORY_EMBEDDING_DOCUMENT_TEMPLATE_VERSION"],"query_instruct":env["MEMORY_EMBEDDING_QUERY_INSTRUCT"]});
        migrated.push("embedding".into());
    }
    if document.get("vision").is_none_or(Value::is_null) {
        if enabled(&env, "VISION_ENABLED")? && text(&env, "VISION_PROVIDER")? != "qwen" {
            return Err("unsupported_legacy_vision_provider".into());
        }
        document["vision"] = json!({"enabled":env["VISION_ENABLED"],"protocol":"chat_completions","base_url":env["VISION_BASE_URL"],"model":env["VISION_MODEL"],"api_key_env":alias(&mut keys,&mut updates,"vision",text(&env,"VISION_API_KEY")?)});
        migrated.push("vision".into());
    }
    if document.get("asr").is_none_or(Value::is_null) {
        let mut url = text(&env, "ASR_BASE_URL")?.trim();
        let mut key = text(&env, "ASR_API_KEY")?.trim();
        if url.is_empty() && key.is_empty() {
            if text(&env, "VISION_PROVIDER")?.eq_ignore_ascii_case("qwen")
                && !text(&env, "VISION_BASE_URL")?.trim().is_empty()
                && !text(&env, "VISION_API_KEY")?.trim().is_empty()
            {
                url = text(&env, "VISION_BASE_URL")?.trim();
                key = text(&env, "VISION_API_KEY")?.trim();
            } else if text(&env, "LLM_MODEL")?
                .to_ascii_lowercase()
                .contains("qwen")
            {
                url = text(&env, "LLM_BASE_URL")?.trim();
                key = text(&env, "LLM_API_KEY")?.trim();
            }
        }
        document["asr"] = json!({"enabled":enabled(&env,"ASR_ENABLED")?&&!url.is_empty(),"protocol":env["ASR_PROTOCOL"],"base_url":url,"model":env["ASR_MODEL"],"api_key_env":alias(&mut keys,&mut updates,"asr",key)});
        migrated.push("asr".into());
    }
    if document.get("search").is_none_or(Value::is_null) {
        let mode = if !env["WEB_MODE"].is_null() {
            env["WEB_MODE"].clone()
        } else {
            json!(if enabled(&env, "WEB_ENABLED")? {
                "native"
            } else {
                "disabled"
            })
        };
        document["search"] = json!({"mode":mode,"tavily_api_key_env":alias(&mut keys,&mut updates,"search",text(&env,"TAVILY_API_KEY")?)});
        migrated.push("search".into());
    }
    if let Some(value) = values.get("llm.model") {
        for p in document["profiles"]
            .as_object_mut()
            .ok_or("invalid_profiles")?
            .values_mut()
        {
            if p["model_env"] == "LLM_MODEL" {
                p["model"] = value.clone();
                p.as_object_mut().unwrap().remove("model_env");
            }
        }
    }
    let mut grouped: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for task in AGENTS {
        grouped
            .entry(
                document["routes"][task]
                    .as_str()
                    .ok_or("route_missing")?
                    .into(),
            )
            .or_default()
            .push(task);
    }
    for (name, tasks) in grouped {
        let profile = document["profiles"][&name].clone();
        let effective = resolve(&profile, &env)?;
        let mut changes = serde_json::Map::new();
        for (key, field, _) in PARAMETERS {
            if let Some(value) = values.get(key)
                && effective[field] != *value
            {
                changes.insert(field.into(), value.clone());
            }
        }
        if changes.is_empty() {
            continue;
        }
        let shared = document["routes"]
            .as_object()
            .unwrap()
            .iter()
            .any(|(task, p)| p == &name && !AGENTS.contains(&task.as_str()))
            || document["search_connection"] == name
            || document["profiles"]
                .as_object()
                .unwrap()
                .values()
                .any(|p| p["search_connection"] == name);
        let mut target = name.clone();
        if shared {
            target = format!("{name}.legacy-agent");
            let mut index = 2;
            while document["profiles"].get(&target).is_some() {
                target = format!("{name}.legacy-agent-{index}");
                index += 1;
            }
            document["profiles"][&target] = profile;
            for task in tasks {
                document["routes"][task] = json!(target);
            }
        }
        if changes.contains_key("thinking_enabled") {
            document["profiles"][&target]
                .as_object_mut()
                .unwrap()
                .remove("thinking_mode");
        }
        document["profiles"][&target]
            .as_object_mut()
            .unwrap()
            .extend(changes);
        migrated.push(target);
    }
    let comparison = compare(&before, &document, &env, &values)?;
    // These are the explicit legacy agent defaults consumed by the original migration.
    // Other captured aliases remain deployment inputs; never rewrite the user's .env.
    Ok(Converted {
        document,
        keys: Value::Object(updates),
        consumed: consumed.into_iter().collect(),
        migrated,
        comparison,
        environment_sources,
        environment: env,
    })
}

pub(super) fn verify_saved(document: &Value, converted: &Converted) -> Result<()> {
    for task in catalog::TASKS {
        let name = document["routes"][task]
            .as_str()
            .ok_or("saved_route_missing")?;
        let actual = resolve(&document["profiles"][name], &converted.environment)?;
        if converted.comparison[task]["effective_sha256"] != digest(actual.to_string().as_bytes()) {
            return Err("saved_effective_parameters_changed".into());
        }
    }
    Ok(())
}
