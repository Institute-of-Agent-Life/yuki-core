use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Jev {
    pub response_model: String,
    pub timeout_seconds: f64,
}

impl Jev {
    pub(super) fn timeout(&self) -> Result<Duration> {
        let duration = Duration::try_from_secs_f64(self.timeout_seconds)?;
        if duration.is_zero() {
            return Err("Jev timeout must be positive".into());
        }
        Ok(duration)
    }

    pub(super) fn result(&self, raw: &str) -> Result<Value> {
        let body: Value = serde_json::from_str(raw)?;
        if body["model"] != self.response_model
            || body["answers"]
                .as_object()
                .is_none_or(|answers| answers.is_empty())
            || body.get("usage").is_some_and(|usage| !usage.is_object())
        {
            return Err("invalid Jev response envelope".into());
        }
        Ok(body)
    }
}

pub(super) fn usage(raw: &str) -> Value {
    let body: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
    let mut known = serde_json::Map::new();
    for (ledger, wire) in [
        ("input", "input_tokens"),
        ("output", "output_tokens"),
        ("total", "total_tokens"),
    ] {
        if let Some(tokens) = body["usage"][wire].as_i64().filter(|&n| n >= 0) {
            known.insert(ledger.into(), json!(tokens));
        }
    }
    if known.is_empty() {
        Value::Null
    } else {
        Value::Object(known)
    }
}
