use super::chat::{self, Mode, Task, Turn};
use crate::{Result, runtime::main::Config};
use serde_json::{Value, json};
use std::collections::HashSet;

fn ephemeral() -> Value {
    json!({"type":"ephemeral"})
}

fn append(body: &mut Value, role: &str, blocks: &[Value]) -> Result<()> {
    let messages = body["messages"]
        .as_array_mut()
        .ok_or("missing Messages history")?;
    if let Some(last) = messages.last_mut().filter(|m| m["role"] == role) {
        last["content"]
            .as_array_mut()
            .ok_or("invalid Messages content")?
            .extend_from_slice(blocks);
    } else {
        messages.push(json!({"role":role,"content":blocks}));
    }
    Ok(())
}

pub fn request(layout: Value, cfg: &Config, task: Option<&Task>) -> Result<Value> {
    let mut body =
        json!({"model":cfg.model,"max_tokens":cfg.max_output,"stream":false,"messages":[]});
    let mut system = Vec::new();
    for msg in layout["messages"]
        .as_array()
        .ok_or("missing program layout")?
    {
        let block =
            json!({"type":"text","text":msg["content"].as_str().ok_or("invalid program text")?});
        match msg["role"].as_str() {
            Some("system") => system.push(block),
            Some(role @ ("assistant" | "user")) => append(&mut body, role, &[block])?,
            _ => return Err("unsupported initial Messages role".into()),
        }
    }
    if let Some(last) = system.last_mut() {
        last["cache_control"] = ephemeral();
        body["system"] = json!(system);
    }
    let thinking =
        cfg.messages_thinking || cfg.reasoning_effort.is_some() || cfg.thinking_budget.is_some();
    if let Some(budget) = cfg.thinking_budget {
        body["thinking"] = json!({"type":"enabled","budget_tokens":budget});
    } else if thinking {
        body["thinking"] = json!({"type":"adaptive"});
        if let Some(effort) = &cfg.reasoning_effort {
            body["output_config"] = json!({"effort":effort});
        }
    }
    let mut tools = Vec::new();
    for decl in layout["tools"].as_array().into_iter().flatten() {
        let function = &decl["function"];
        tools.push(json!({"name":function["name"],"description":function["description"].as_str().unwrap_or(""),"input_schema":function["parameters"]}));
    }
    if !tools.is_empty() {
        let choice = if task.is_some() {
            "emit_result"
        } else {
            cfg.tool_choice.as_deref().unwrap_or("auto")
        };
        body["tool_choice"] = match choice {
            "none" => json!({"type":"none"}),
            "auto" => json!({"type":"auto"}),
            _ if thinking => json!({"type":"auto"}),
            "required" => json!({"type":"any"}),
            name => json!({"type":"tool","name":name}),
        };
    }
    if cfg.native_web_search && task.is_none() {
        tools.push(json!({"type":"web_search_20250305","name":"web_search"}));
    }
    if let Some(last) = tools.last_mut() {
        last["cache_control"] = ephemeral();
        body["tools"] = json!(tools);
    }
    if let Some(task) = task {
        match task.mode {
            Mode::JsonSchema => {
                body["output_config"]["format"] = json!({"type":"json_schema","schema":task.schema})
            }
            Mode::TextJson => {
                return Err("Messages native response format requires JSON Schema".into());
            }
            Mode::FunctionTool => {}
        }
    }
    cache_last(&mut body)?;
    Ok(body)
}

// The conversation breakpoint moves. Signed provider blocks are never edited.
pub fn cache_last(body: &mut Value) -> Result<()> {
    let mut newest = true;
    for msg in body["messages"]
        .as_array_mut()
        .ok_or("missing Messages history")?
        .iter_mut()
        .rev()
        .filter(|m| m["role"] == "user")
    {
        for block in msg["content"]
            .as_array_mut()
            .ok_or("invalid Messages history")?
            .iter_mut()
            .rev()
        {
            // Only user blocks are owned by us. Assistant text can carry opaque provider fields.
            if matches!(block["type"].as_str(), Some("text" | "tool_result")) {
                if newest {
                    block["cache_control"] = ephemeral();
                    newest = false;
                } else if let Some(object) = block.as_object_mut() {
                    object.remove("cache_control");
                }
            }
        }
    }
    Ok(())
}
pub fn replay(body: &mut Value, response: &Value) -> Result<()> {
    append(
        body,
        "assistant",
        response["content"]
            .as_array()
            .ok_or("missing original assistant blocks")?,
    )
}
pub fn user(body: &mut Value, text: &str) -> Result<()> {
    append(body, "user", &[json!({"type":"text","text":text})])
}
pub fn output(body: &mut Value, id: &str, raw: &str) -> Result<()> {
    let receipt: Value = serde_json::from_str(raw)?;
    let mut block = json!({"type":"tool_result","tool_use_id":id,"content":raw});
    if matches!(receipt["state"].as_str(), Some("failed" | "unknown")) || receipt["ok"] == false {
        block["is_error"] = json!(true);
    }
    append(body, "user", &[block])
}
pub fn complete(v: &Value) -> bool {
    v["type"] == "message"
        && v["role"] == "assistant"
        && v["error"].is_null()
        && v["content"].is_array()
        && matches!(
            v["stop_reason"].as_str(),
            Some("tool_use" | "end_turn" | "stop_sequence" | "pause_turn")
        )
}

pub fn decode(v: &Value, task: Option<&Task>, cfg: &Config, paid: &Value) -> Result<Turn> {
    if !complete(v) {
        return Err("incomplete Messages response".into());
    }
    let mut text = String::new();
    let mut intents = Vec::new();
    let mut ids = HashSet::new();
    let mut emitted = None;
    let mut native = Vec::new();
    let mut citations = Vec::new();
    let mut server_ids = HashSet::new();
    for msg in paid["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["role"] == "assistant")
    {
        for b in msg["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["type"] == "server_tool_use")
        {
            if let Some(id) = b["id"].as_str() {
                server_ids.insert(id);
            }
        }
    }
    let declared = paid["tools"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let server_allowed = paid["tool_choice"]["type"] != "none"
        && declared.iter().any(|d| d["type"] == "web_search_20250305");
    for block in v["content"].as_array().unwrap() {
        match block["type"].as_str() {
            Some("text") => {
                text.push_str(block["text"].as_str().ok_or("missing Messages text")?);
                citations.extend(block["citations"].as_array().into_iter().flatten().cloned());
            }
            Some("thinking")
                if block["thinking"].is_string()
                    && block["signature"].as_str().is_some_and(|s| !s.is_empty()) => {}
            Some("redacted_thinking") if block["data"].is_string() => {}
            Some("server_tool_use") if server_allowed && block["name"] == "web_search" => {
                let id = block["id"]
                    .as_str()
                    .filter(|s| !s.is_empty() && server_ids.insert(*s) && ids.insert(*s))
                    .ok_or("duplicate server tool ID")?;
                native.push(json!({"id":id,"type":"search","input":block["input"]}));
            }
            Some("web_search_tool_result") if server_allowed => {
                let id = block["tool_use_id"]
                    .as_str()
                    .filter(|id| server_ids.contains(*id))
                    .ok_or("unpaired server search result")?;
                if !block["content"].is_array() && !block["content"].is_object() {
                    return Err("invalid server result".into());
                }
                native.push(json!({"id":id,"status":if block["content"].is_array(){"completed"}else{"failed"},"content":block["content"]}));
            }
            Some("tool_use") => {
                let id = block["id"]
                    .as_str()
                    .filter(|id| !id.is_empty() && !server_ids.contains(*id) && ids.insert(*id))
                    .ok_or("duplicate or missing tool_use id")?;
                let name = block["name"].as_str().ok_or("missing tool_use name")?;
                let declaration = declared
                    .iter()
                    .find(|d| d["name"] == name && d["input_schema"].is_object())
                    .ok_or("tool outside paid declaration")?;
                if paid["tool_choice"]["type"] == "none"
                    || !jsonschema::validator_for(&declaration["input_schema"])?
                        .is_valid(&block["input"])
                {
                    return Err("invalid paid tool arguments".into());
                }
                if let Some(task) = task {
                    if task.mode != Mode::FunctionTool || name != "emit_result" || emitted.is_some()
                    {
                        return Err("invalid structured Messages tool".into());
                    }
                    emitted = Some(block["input"].clone());
                } else {
                    let choice = cfg.tool_choice.as_deref().unwrap_or("auto");
                    if !matches!(choice, "auto" | "required") && choice != name {
                        return Err("wrong logical Messages choice".into());
                    }
                    intents.push(chat::intent(id, name, block["input"].clone())?);
                }
            }
            _ => return Err("unsupported or incomplete Messages block".into()),
        }
    }
    let calls = !intents.is_empty() || emitted.is_some();
    if (v["stop_reason"] == "tool_use") != calls {
        return Err("Messages stop and tool_use disagree".into());
    }
    let result = if v["stop_reason"] == "pause_turn" {
        if v["content"].as_array().unwrap().is_empty() {
            return Err("paused Messages response lacks continuation blocks".into());
        }
        Value::Null
    } else if let Some(task) = task {
        let value = if task.mode == Mode::FunctionTool {
            if !text.trim().is_empty() {
                return Err("extra text with structured Messages tool".into());
            }
            emitted.ok_or("missing emit_result")?
        } else {
            serde_json::from_str(&text)?
        };
        chat::validate_result(task, value)?
    } else {
        let required = cfg
            .tool_choice
            .as_deref()
            .is_some_and(|s| !matches!(s, "auto" | "none"));
        if !calls && (required || text.is_empty() && native.is_empty()) {
            return Err("expected Messages tool or visible result".into());
        }
        json!({"text":text,"native_events":native,"citations":citations})
    };
    Ok(Turn {
        intents: json!(intents),
        result,
    })
}

pub fn usage(raw: &str) -> Value {
    let body = serde_json::from_str::<Value>(raw).unwrap_or(Value::Null);
    let u = &body["usage"];
    let mut known = serde_json::Map::new();
    for (field, value) in [
        ("uncached_input", &u["input_tokens"]),
        ("cache", &u["cache_read_input_tokens"]),
        ("cache_creation", &u["cache_creation_input_tokens"]),
        (
            "cache_creation_5m",
            &u["cache_creation"]["ephemeral_5m_input_tokens"],
        ),
        (
            "cache_creation_1h",
            &u["cache_creation"]["ephemeral_1h_input_tokens"],
        ),
        ("reasoning", &u["output_tokens_details"]["thinking_tokens"]),
        ("output", &u["output_tokens"]),
        (
            "server_web_search_requests",
            &u["server_tool_use"]["web_search_requests"],
        ),
    ] {
        if let Some(n) = value.as_i64().filter(|v| *v >= 0) {
            known.insert(field.into(), json!(n));
        }
    }
    let sum = |keys: &[&str]| {
        keys.iter()
            .try_fold(0i64, |acc, k| acc.checked_add(known.get(*k)?.as_i64()?))
    };
    if let Some(n) = sum(&["uncached_input", "cache", "cache_creation"]) {
        if let Some(t) = known
            .get("output")
            .and_then(Value::as_i64)
            .and_then(|output| n.checked_add(output))
        {
            known.insert("total".into(), json!(t));
        }
        known.insert("input".into(), json!(n));
    }
    if known.is_empty() {
        Value::Null
    } else {
        Value::Object(known)
    }
}
