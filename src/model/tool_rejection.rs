//! Trusted decoder outcomes, distinct from untrusted tool arguments.
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Rejection {
    ToolNotDeclared,
    InvalidToolArguments,
    ToolChoiceMismatch,
}
pub(crate) fn arguments(raw: &str) -> Value {
    // The immutable provider response retains the exact malformed argument string.
    serde_json::from_str(raw).unwrap_or_else(|_| json!(raw))
}
pub(crate) fn validate(
    schema: Option<&Value>,
    args: &Value,
    allowed: bool,
) -> Result<Option<Rejection>> {
    let Some(schema) = schema else {
        return Ok(Some(Rejection::ToolNotDeclared));
    };
    if !allowed {
        return Ok(Some(Rejection::ToolChoiceMismatch));
    }
    Ok((!jsonschema::validator_for(schema)?.is_valid(args))
        .then_some(Rejection::InvalidToolArguments))
}
pub(crate) fn intent(id: &str, name: &str, payload: Value, error: Rejection) -> Value {
    json!({"call_id":id,"action":"external","target":name,"payload":payload,"rejected":error})
}
pub(crate) fn mark(intent: &mut Value, error: Rejection) {
    if intent["action"] != "external" {
        intent["target"] = intent["action"].clone();
        intent["action"] = json!("external");
    }
    intent["rejected"] = json!(error);
}
