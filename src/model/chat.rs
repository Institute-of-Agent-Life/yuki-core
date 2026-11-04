use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    FunctionTool,
    TextJson,
    JsonSchema,
}
#[derive(Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub mode: Mode,
    pub schema: Value,
    #[serde(default)]
    pub validation_retries: u8,
}
#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub text: String,
    #[serde(default)]
    pub context: Vec<Value>,
    pub structured: Option<Task>,
}

fn function(name: &str, schema: Value) -> Value {
    json!({"type":"function","function":{"name":name,"parameters":schema}})
}
fn binding(name: &str) -> Value {
    if name == "send" {
        return function(name, crate::social::content::schema());
    }
    let (properties, required) = match name {
        "get" => (
            json!({"target":{"type":"string","minLength":1}}),
            json!(["target"]),
        ),
        "put" => (
            json!({"target":{"type":"string","minLength":1},"value":{}}),
            json!(["target", "value"]),
        ),
        "complete" => (json!({"summary":{"type":"string"}}), json!(["summary"])),
        _ => unreachable!(),
    };
    function(
        name,
        json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    )
}
const TOOLS: [&str; 4] = ["get", "put", "complete", "send"];
pub fn configure(body: &mut Value, task: Option<&Task>) {
    match task {
        None => {
            body["tools"] = TOOLS.iter().map(|name| binding(name)).collect();
            body["tool_choice"] = json!("auto");
        }
        Some(task) => match task.mode {
            Mode::FunctionTool => {
                body["tools"] = json!([function("emit_result", task.schema.clone())]);
                body["tool_choice"] = json!({"type":"function","function":{"name":"emit_result"}});
            }
            Mode::JsonSchema => {
                body["response_format"] = json!({"type":"json_schema","json_schema":{"name":"result","strict":true,"schema":task.schema}})
            }
            Mode::TextJson => {
                body["response_format"] = json!({"type":"json_object"});
                body["messages"].as_array_mut().unwrap().insert(1,json!({"role":"system","content":format!("Return one JSON object satisfying {}",task.schema)}));
            }
        },
    }
}
pub struct Turn {
    pub intents: Value,
    pub result: Value,
}
pub fn decode(raw: &str, task: Option<&Task>) -> Result<Turn> {
    let body: Value = serde_json::from_str(raw)?;
    let choices = body["choices"]
        .as_array()
        .filter(|c| c.len() == 1)
        .ok_or("expected one complete choice")?;
    let choice = &choices[0];
    let message = &choice["message"];
    let finish = choice["finish_reason"]
        .as_str()
        .ok_or("missing finish reason")?;
    if !body["error"].is_null()
        || !message["refusal"].is_null()
        || message["role"] != "assistant"
        || !matches!(finish, "stop" | "tool_calls")
    {
        return Err("incomplete choice".into());
    }
    let calls = match &message["tool_calls"] {
        Value::Null => &[][..],
        Value::Array(items) => items.as_slice(),
        _ => return Err("invalid tool_calls".into()),
    };
    if (finish == "tool_calls") == calls.is_empty() {
        return Err("tool calls disagree with finish reason".into());
    }
    let content = match &message["content"] {
        Value::Null => "",
        Value::String(s) => s,
        _ => return Err("unsupported assistant content".into()),
    };
    let mut seen = HashSet::new();
    let mut intents = Vec::new();
    let mut emitted = None;
    for call in calls {
        let id = call["id"]
            .as_str()
            .filter(|id| !id.is_empty() && seen.insert(*id))
            .ok_or("missing or duplicate call id")?;
        if call["type"] != "function" {
            return Err("unknown call type".into());
        }
        let name = call["function"]["name"]
            .as_str()
            .ok_or("missing function name")?;
        let arguments: Value = serde_json::from_str(
            call["function"]["arguments"]
                .as_str()
                .ok_or("invalid function arguments")?,
        )?;
        if let Some(task) = task {
            if task.mode != Mode::FunctionTool
                || name != "emit_result"
                || calls.len() != 1
                || !content.trim().is_empty()
            {
                return Err("unexpected structured tool or accompanying text".into());
            }
            emitted = Some(arguments);
            continue;
        }
        if !TOOLS.contains(&name)
            || !jsonschema::validator_for(&binding(name)["function"]["parameters"])?
                .is_valid(&arguments)
        {
            return Err("tool outside pinned contract".into());
        }
        let (action, target) = if matches!(name, "get" | "put") {
            (name, arguments["target"].as_str().unwrap())
        } else {
            ("external", name)
        };
        intents.push(json!({"call_id":id,"action":action,"target":target,"payload":arguments}));
    }
    let result = if let Some(task) = task {
        let value = if task.mode == Mode::FunctionTool {
            emitted.ok_or("missing emit_result")?
        } else {
            serde_json::from_str(content)?
        };
        if !value.is_object() || !jsonschema::validator_for(&task.schema)?.is_valid(&value) {
            return Err("invalid structured result".into());
        }
        value
    } else {
        message.clone()
    };
    Ok(Turn {
        intents: Value::Array(intents),
        result,
    })
}

pub fn usage(raw: &str) -> Value {
    let v = serde_json::from_str::<Value>(raw).unwrap_or(Value::Null);
    let mut usage = serde_json::Map::new();
    for (name, val) in [
        ("input", &v["usage"]["prompt_tokens"]),
        ("output", &v["usage"]["completion_tokens"]),
        ("total", &v["usage"]["total_tokens"]),
        ("cache", &v["usage"]["prompt_cache_hit_tokens"]),
        (
            "reasoning",
            &v["usage"]["completion_tokens_details"]["reasoning_tokens"],
        ),
    ] {
        if let Some(n) = val.as_i64().filter(|n| *n >= 0) {
            usage.insert(name.into(), json!(n));
        }
    }
    if usage.is_empty() {
        Value::Null
    } else {
        Value::Object(usage)
    }
}
