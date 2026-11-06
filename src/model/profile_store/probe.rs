//! Explicit one-shot Admin probes. Drafts and replies never enter Work or its ledger.
use super::{Request, Service, catalog, files, invoke, runtime};
use crate::{Result, operator::config::Authority, runtime::main::Config, store::Store};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn connection(service: &Service, input: &Value, discover: bool) -> Result<Config> {
    let saved = input.get("profile_id").is_some();
    catalog::validate(
        if saved {
            "SavedModelConnection"
        } else if discover {
            "DraftModelDiscovery"
        } else {
            "DraftModelTest"
        },
        input,
    )?;
    let bundle = files::Bundle::read(&service.root, &service.filename)?;
    let (id, mut profile, headers) = if saved {
        let id = input["profile_id"].as_str().ok_or("validation_error")?;
        let original = bundle
            .configuration
            .as_ref()
            .ok_or("configuration_unavailable")?;
        let document = catalog::Catalog::parse(std::str::from_utf8(&original.data)?)?;
        (
            id.to_owned(),
            document.public["profiles"]
                .get(id)
                .cloned()
                .ok_or("not_found")?,
            document.raw["profiles"][id].get("headers").cloned(),
        )
    } else if discover {
        let mut profile = input["draft_connection"].clone();
        profile["model"] = json!("discovery");
        profile["default_temperature"] = json!(0);
        profile["default_max_output_tokens"] = json!(1);
        ("discovery".into(), profile, None)
    } else {
        (
            input["draft_profile"]["id"]
                .as_str()
                .ok_or("validation_error")?
                .into(),
            input["draft_profile"].clone(),
            None,
        )
    };
    // Probe policy applies only to this ephemeral copy, including a saved profile.
    let fields = profile.as_object_mut().ok_or("validation_error")?;
    fields.remove("thinking_mode");
    fields.remove("reasoning_effort_env");
    fields.insert("thinking_enabled".into(), json!(false));
    fields.insert("search_mode".into(), Value::Null);
    fields.insert("max_retries".into(), json!(0));
    fields
        .entry("protocol")
        .or_insert(json!("chat_completions"));
    let limit = profile["default_max_output_tokens"]
        .as_i64()
        .ok_or("validation_error")?
        .min(16);
    profile["default_max_output_tokens"] = json!(limit);
    let mut keys = bundle.keys;
    if let Some(secret) = input["api_key"].as_str() {
        if secret.trim().is_empty() || secret.len() > 8192 || secret.contains(['\r', '\n']) {
            return Err("validation_error".into());
        }
        let alias = profile["api_key_env"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("ADMIN_PROBE_KEY")
            .to_owned();
        keys.insert(alias.clone(), secret.into());
        profile["api_key_env"] = json!(alias);
    }
    let public = json!({"profiles":{&id:profile}});
    let mut raw = public.clone();
    if let Some(headers) = headers {
        raw["profiles"][&id]["headers"] = headers;
    }
    let catalog = catalog::Catalog {
        original: toml_edit::DocumentMut::new(),
        public,
        raw,
    };
    runtime::validate_profiles(&catalog, &keys)?;
    runtime::compile_connection(service, &catalog, &keys, &id, false)
}

fn status_error(status: u16, raw: &[u8], discover: bool) -> Option<&'static str> {
    if (200..300).contains(&status) {
        return None;
    }
    Some(match status {
        401 | 403 => "authentication",
        429 if serde_json::from_slice::<Value>(raw)
            .ok()
            .is_some_and(|v| v["error"]["code"] == "insufficient_quota") =>
        {
            "quota"
        }
        429 => "rate_limit",
        408 | 504 => "timeout",
        404 | 405 | 501 if discover => "unsupported",
        500..=599 => "unavailable",
        _ => "protocol",
    })
}
fn transport_error(error: &(dyn std::error::Error + 'static)) -> &'static str {
    if error
        .downcast_ref::<reqwest::Error>()
        .is_some_and(reqwest::Error::is_timeout)
    {
        "timeout"
    } else {
        "unavailable"
    }
}
async fn discover(
    client: &reqwest::Client,
    cfg: &Config,
) -> std::result::Result<Value, &'static str> {
    let profile = &cfg.profile_binding.as_ref().ok_or("configuration")?.profile;
    let base = profile["base_url"]
        .as_str()
        .ok_or("configuration")?
        .trim_end_matches('/');
    let seconds = profile["timeout_seconds"].as_f64().ok_or("configuration")?;
    let mut request = client
        .get(format!("{base}/models"))
        .timeout(Duration::try_from_secs_f64(seconds).map_err(|_| "configuration")?);
    let credentials = cfg.credentials.as_ref().ok_or("configuration")?;
    for (key, value) in &credentials.headers {
        request = request.header(key, value);
    }
    if cfg.protocol.is_messages() {
        request = request.header("anthropic-version", "2023-06-01");
    }
    if let Some(key) = &credentials.key {
        request = if cfg.protocol.is_messages() {
            request.header("x-api-key", key)
        } else if cfg.protocol.is_gemini() {
            request.header("x-goog-api-key", key)
        } else {
            request.bearer_auth(key)
        };
    }
    let mut response = request.send().await.map_err(|e| transport_error(&e))?;
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| transport_error(&e))? {
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err("protocol");
        }
        bytes.extend(chunk);
    }
    if let Some(error) = status_error(status, &bytes, true) {
        return Err(error);
    }
    let data: Value = serde_json::from_slice(&bytes).map_err(|_| "protocol")?;
    let (field, id, title) = if cfg.protocol.is_gemini() {
        ("models", "name", "displayName")
    } else {
        ("data", "id", "display_name")
    };
    let mut models = Vec::new();
    for item in data[field].as_array().ok_or("protocol")? {
        let mut model = json!({"id":item[id].as_str().ok_or("protocol")?});
        if let Some(name) = item[title].as_str() {
            model["display_name"] = json!(name);
        }
        models.push(model);
    }
    Ok(json!({"models":models,"error_category":null}))
}
async fn test(client: &reqwest::Client, cfg: &Config) -> std::result::Result<Value, &'static str> {
    let body = cfg
        .protocol
        .request(
            json!({"model":cfg.model,"max_tokens":cfg.max_output,
        "messages":[{"role":"user","content":"Reply OK."}],"stream":false}),
            cfg,
            None,
        )
        .map_err(|_| "unsupported")?;
    let (status, bytes, complete) =
        crate::runtime::main::physical_request(client, cfg, &body.to_string())
            .await
            .map_err(|e| transport_error(e.as_ref()))?;
    if let Some(error) = status_error(status, &bytes, false) {
        return Err(error);
    }
    if !complete {
        return Err("unavailable");
    }
    let raw = std::str::from_utf8(&bytes).map_err(|_| "protocol")?;
    let response: Value = serde_json::from_str(raw).map_err(|_| "protocol")?;
    if !cfg.protocol.complete(&response) {
        return Err("protocol");
    }
    cfg.protocol
        .decode(raw, None, cfg, &body)
        .map_err(|_| "protocol")?;
    Ok(json!({"ok":true,"error_category":null}))
}

pub(crate) async fn run(
    store: &Store,
    authority: Authority,
    method: &str,
    input: &Value,
) -> Result<Value> {
    let discover_models = method == "model.discover";
    invoke(store, Request::ProbeAuth(authority.clone())).await?;
    let service = store
        .profile_files
        .as_ref()
        .ok_or("configuration_unavailable")?;
    let started = Instant::now();
    let cfg = connection(service, input, discover_models)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    // Revoke wins immediately before dispatch and again before publishing the result.
    invoke(store, Request::ProbeAuth(authority.clone())).await?;
    let result = if discover_models {
        discover(&client, &cfg).await
    } else {
        test(&client, &cfg).await
    };
    invoke(store, Request::ProbeAuth(authority)).await?;
    let mut result = result.unwrap_or_else(|category| {
        if discover_models {
            json!({"models":[],"error_category":category})
        } else {
            json!({"ok":false,"error_category":category})
        }
    });
    if !discover_models {
        result["latency_seconds"] = json!(started.elapsed().as_secs_f64());
    }
    Ok(result)
}
