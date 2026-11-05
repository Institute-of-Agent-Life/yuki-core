use crate::Result;
use serde_json::{Map, Value, json};
use std::sync::LazyLock;
use toml_edit::{DocumentMut, Item};

pub(super) const TASKS: [&str; 6] = [
    "chat_agent",
    "memory_extraction",
    "memory_self_reflection",
    "memory_dream",
    "plugin_agent_session",
    "utility_structured",
];

static OPENAPI: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../../protocol/baseline-v4/admin-openapi.json"
    ))
    .expect("fixed v4 Admin schema")
});

pub(super) fn schema(name: &str) -> &Value {
    &OPENAPI["components"]["schemas"][name]
}

pub(super) fn validate(name: &str, value: &Value) -> Result<()> {
    let spec = json!({"components":OPENAPI["components"],
        "$ref":format!("#/components/schemas/{name}")});
    if !jsonschema::validator_for(&spec)?.is_valid(value) {
        return Err(if name.ends_with("Body") {
            "invalid_command_body"
        } else {
            "validation_error"
        }
        .into());
    }
    Ok(())
}

pub(super) struct Catalog {
    pub original: DocumentMut,
    pub public: Value,
    pub raw: Value,
}

fn shape(mut schema: &Value) -> &Value {
    loop {
        if let Some(reference) = schema["$ref"].as_str() {
            schema = &OPENAPI["components"]["schemas"][reference.rsplit('/').next().unwrap_or("")];
        } else if let Some(variants) = schema["anyOf"].as_array() {
            schema = variants
                .iter()
                .find(|item| item["type"] != "null")
                .unwrap_or(schema);
            if schema.get("anyOf").is_some() {
                return schema;
            }
        } else {
            return schema;
        }
    }
}

fn public_fields(raw: &Value, definition: &Value) -> Value {
    let definition = shape(definition);
    if let Some(object) = raw.as_object() {
        let mut result = Map::new();
        for (key, value) in object {
            if key == "headers" {
                continue;
            }
            let field = definition["properties"].get(key).or_else(|| {
                definition["additionalProperties"]
                    .as_object()
                    .map(|_| &definition["additionalProperties"])
            });
            if let Some(field) = field {
                result.insert(key.clone(), public_fields(value, field));
            }
        }
        Value::Object(result)
    } else if let Some(array) = raw.as_array() {
        Value::Array(
            array
                .iter()
                .map(|value| public_fields(value, &definition["items"]))
                .collect(),
        )
    } else {
        raw.clone()
    }
}

impl Catalog {
    pub(super) fn parse(text: &str) -> Result<Self> {
        if text.len() > 256 * 1024 {
            return Err("validation_error".into());
        }
        let original: DocumentMut = text.parse().map_err(|_| "validation_error")?;
        let raw: Value = toml::from_str(text).map_err(|_| "validation_error")?;
        let mut public = public_fields(&raw, schema("ModelDocument"));
        if let Some(routes) = public["routes"].as_object_mut() {
            routes.remove("conversation_compaction");
        }
        public["profiles"] = Value::Object(
            raw["profiles"]
                .as_object()
                .ok_or("validation_error")?
                .iter()
                .map(|(name, profile)| {
                    (
                        name.clone(),
                        public_fields(profile, schema("ModelProfileDocument")),
                    )
                })
                .collect(),
        );
        validate_document(&public)?;
        Ok(Self {
            original,
            public,
            raw,
        })
    }

    pub(super) fn patch(mut self, submitted: &Value) -> Result<String> {
        validate_document(submitted)?;
        let mut effective = submitted.clone();
        if effective["routes"].get("conversation_compaction").is_none()
            && let Some(original) = self.raw["routes"].get("conversation_compaction")
        {
            effective["routes"]["conversation_compaction"] = original.clone();
        }
        let mut root = Item::Table(self.original.as_table().clone());
        patch_schema(&mut root, &effective, schema("ModelDocument"))?;
        *self.original.as_table_mut() = root.into_table().map_err(|_| "validation_error")?;
        let text = self.original.to_string();
        Self::parse(&text)?;
        Ok(text)
    }
}

fn patch_schema(target: &mut Item, submitted: &Value, definition: &Value) -> Result<()> {
    let definition = shape(definition);
    let Some(object) = submitted.as_object() else {
        return replace(target, submitted);
    };
    if target.is_none() {
        *target = Item::Table(toml_edit::Table::new());
    }
    let table = target.as_table_like_mut().ok_or("validation_error")?;
    if let Some(properties) = definition["properties"].as_object() {
        for (key, field) in properties {
            // Header material remains exclusively in its existing server-side section.
            if key == "headers" {
                continue;
            }
            let value = &submitted[key];
            if value.is_null() {
                table.remove(key);
                continue;
            }
            patch_entry(table, key, value, field)?;
        }
    } else if definition["additionalProperties"].is_object() {
        let removed: Vec<String> = table
            .iter()
            .filter(|(key, _)| !object.contains_key(*key))
            .map(|(key, _)| key.to_owned())
            .collect();
        for key in removed {
            table.remove(&key);
        }
        for (key, value) in object {
            patch_entry(table, key, value, &definition["additionalProperties"])?;
        }
    } else {
        return Err("validation_error".into());
    }
    Ok(())
}

fn patch_entry(
    table: &mut dyn toml_edit::TableLike,
    key: &str,
    value: &Value,
    definition: &Value,
) -> Result<()> {
    if let Some(original) = table.get_mut(key) {
        patch_schema(original, value, definition)
    } else {
        let mut item = Item::None;
        patch_schema(&mut item, value, definition)?;
        table.insert(key, item);
        Ok(())
    }
}

fn replace(original: &mut Item, value: &Value) -> Result<()> {
    let mut prior_document = DocumentMut::new();
    prior_document["value"] = original.clone();
    let previous: Value = toml::from_str(&prior_document.to_string()).unwrap_or(Value::Null);
    if previous.get("value") == Some(value) {
        return Ok(());
    }
    let encoded = toml::to_string(&json!({"value":value})).map_err(|_| "validation_error")?;
    let mut document: DocumentMut = encoded.parse().map_err(|_| "validation_error")?;
    let mut replacement = document.remove("value").ok_or("validation_error")?;
    if let (Some(before), Some(after)) = (original.as_value(), replacement.as_value_mut()) {
        *after.decor_mut() = before.decor().clone();
    }
    *original = replacement;
    Ok(())
}

fn validate_document(document: &Value) -> Result<()> {
    validate("ModelDocument", document)?;
    if document["schema_version"] != 3 {
        return Err("validation_error".into());
    }
    for name in ["embedding", "vision", "asr"] {
        if let Some(base) = document[name]["base_url"]
            .as_str()
            .filter(|v| !v.is_empty())
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
    }
    let profiles = document["profiles"].as_object().ok_or("validation_error")?;
    for target in std::iter::once(&document["search_connection"])
        .chain(profiles.values().map(|p| &p["search_connection"]))
    {
        if target.as_str().is_some_and(|id| !profiles.contains_key(id)) {
            return Err("validation_error".into());
        }
    }
    for (name, profile) in profiles {
        if name.is_empty()
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
        {
            return Err("validation_error".into());
        }
        for field in ["base_url_env", "model_env", "reasoning_effort_env"] {
            if let Some(alias) = profile[field].as_str()
                && alias.is_empty()
            {
                return Err("validation_error".into());
            }
        }
        if (profile.get("model_env").is_none()
            && profile["model"].as_str().is_none_or(str::is_empty))
            || (profile["provider"]
                .as_str()
                .is_some_and(|p| !p.eq_ignore_ascii_case("fake"))
                && profile.get("base_url_env").is_none()
                && profile["base_url"].as_str().is_none_or(str::is_empty))
            || profile["thinking_mode"]
                .as_str()
                .is_some_and(|mode| !matches!(mode, "enabled" | "disabled" | "configurable"))
        {
            return Err("validation_error".into());
        }
        if let Some(endpoint) = profile["base_url"]
            .as_str()
            .filter(|value| !value.is_empty())
        {
            let endpoint = reqwest::Url::parse(endpoint).map_err(|_| "validation_error")?;
            if !matches!(endpoint.scheme(), "http" | "https")
                || !endpoint.username().is_empty()
                || endpoint.password().is_some()
                || endpoint.query().is_some()
                || endpoint.fragment().is_some()
            {
                return Err("validation_error".into());
            }
        }
        for field in ["timeout_seconds", "default_max_output_tokens"] {
            if profile[field]
                .as_f64()
                .is_none_or(|n| !n.is_finite() || n <= 0.0)
            {
                return Err("validation_error".into());
            }
        }
        if profile["provider"].as_str().is_none_or(str::is_empty)
            || profile["max_retries"].as_u64().is_none()
            || profile["default_temperature"]
                .as_f64()
                .is_none_or(|n| !(0.0..=2.0).contains(&n))
        {
            return Err("validation_error".into());
        }
    }
    let routes = document["routes"].as_object().ok_or("validation_error")?;
    if TASKS.iter().any(|task| {
        routes
            .get(*task)
            .and_then(Value::as_str)
            .is_none_or(|name| !profiles.contains_key(name))
    }) || routes
        .keys()
        .any(|task| task != "conversation_compaction" && !TASKS.contains(&task.as_str()))
    {
        return Err("validation_error".into());
    }
    for task in ["chat_agent", "plugin_agent_session"] {
        let name = routes[task].as_str().ok_or("validation_error")?;
        if !profiles[name]["capabilities"]
            .as_array()
            .is_some_and(|caps| caps.contains(&json!("tools")))
        {
            return Err("validation_error".into());
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) const ORIGINAL: &str = r#"schema_version = 3
# deployment annotation remains owned by the operator
deployment_note = 'keep this exact quoting'

[profiles.primary]
provider = 'fixture'
protocol = 'chat_completions'
base_url = 'http://127.0.0.1:12001/v1'
api_key_env = 'MODEL_TEST_KEY'
model = 'before' # model annotation
timeout_seconds = 30.0
max_retries = 0
default_temperature = 0.1
default_max_output_tokens = 128
capabilities = ['tools']
operator_extension = 'retain this unknown field'
[profiles.primary.headers]
x-private-provider = 'header-canary'

[routes]
# routes annotation
chat_agent = 'primary'
memory_extraction = 'primary'
memory_self_reflection = 'primary'
memory_dream = 'primary'
plugin_agent_session = 'primary'
utility_structured = 'primary'
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_patch_keeps_unknown_fields_comments_and_hidden_headers() {
        let original = Catalog::parse(ORIGINAL).unwrap();
        let mut public = original.public.clone();
        assert!(public.get("deployment_note").is_none());
        assert!(public["profiles"]["primary"].get("headers").is_none());
        assert!(!public.to_string().contains("header-canary"));
        assert!(
            public["profiles"]["primary"]
                .get("operator_extension")
                .is_none()
        );
        public["profiles"]["primary"]["model"] = json!("after");
        let saved = original.patch(&public).unwrap();
        for fragment in [
            "# deployment annotation remains owned by the operator",
            "deployment_note = 'keep this exact quoting'",
            "# model annotation",
            "operator_extension = 'retain this unknown field'",
            "x-private-provider = 'header-canary'",
            "# routes annotation",
            "capabilities = ['tools']",
        ] {
            assert!(saved.contains(fragment), "lost {fragment}");
        }
        assert_eq!(Catalog::parse(&saved).unwrap().public, public);
    }

    #[test]
    fn profile_noop_round_trips_original_toml_bytes() {
        let original = Catalog::parse(ORIGINAL).unwrap();
        let public = original.public.clone();
        assert_eq!(original.patch(&public).unwrap(), ORIGINAL);
    }

    #[test]
    fn retired_compaction_route_remains_readable_without_becoming_an_active_task() {
        let old = ORIGINAL.replace(
            "[routes]",
            "[routes]\nconversation_compaction = 'retired_missing_profile' # original metadata",
        );
        let original = Catalog::parse(&old).unwrap();
        assert!(
            original.public["routes"]
                .get("conversation_compaction")
                .is_none()
        );
        let public = original.public.clone();
        assert_eq!(original.patch(&public).unwrap(), old);
    }

    #[test]
    fn profile_writer_rejects_hidden_headers_bad_routes_and_invalid_bounds() {
        let original = Catalog::parse(ORIGINAL).unwrap().public;
        for (pointer, value) in [
            ("/schema_version", json!(2)),
            ("/profiles/primary/timeout_seconds", json!(0)),
            ("/profiles/primary/max_retries", json!(-1)),
            ("/profiles/primary/default_temperature", json!(3)),
            ("/profiles/primary/model", json!("")),
            (
                "/profiles/primary/base_url",
                json!("https://user:private-key@model.invalid/v1"),
            ),
            ("/profiles/primary/capabilities", json!([])),
            ("/routes/chat_agent", json!("missing")),
        ] {
            let mut changed = original.clone();
            *changed.pointer_mut(pointer).unwrap() = value;
            assert!(
                Catalog::parse(ORIGINAL).unwrap().patch(&changed).is_err(),
                "{pointer}"
            );
        }
        let mut alias = original.clone();
        alias["profiles"]["primary"]["base_url_env"] = json!("");
        assert!(Catalog::parse(ORIGINAL).unwrap().patch(&alias).is_err());
        alias["profiles"]["primary"]["base_url_env"] = json!("custom_endpoint");
        assert!(Catalog::parse(ORIGINAL).unwrap().patch(&alias).is_ok());
        alias["profiles"]["secondary"] = original["profiles"]["primary"].clone();
        let added = Catalog::parse(ORIGINAL).unwrap().patch(&alias).unwrap();
        assert_eq!(Catalog::parse(&added).unwrap().public, alias);
        let mut hidden = original;
        hidden["profiles"]["primary"]["headers"] = json!({"x-private-provider":"forged"});
        assert!(Catalog::parse(ORIGINAL).unwrap().patch(&hidden).is_err());
    }
}
