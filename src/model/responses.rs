use super::chat::{self, Mode, Task, Turn};
use crate::{Result, runtime::main::Config};
use serde_json::{Value, json};
use std::collections::HashSet;

pub fn request(layout: Value, cfg: &Config, task: Option<&Task>) -> Result<Value> {
    let legacy = cfg.protocol == super::wire::Protocol::DeepseekResponsesLegacy;
    let native = cfg.native_web_search && task.is_none();
    let choice = if let Some(task) = task {
        if task.mode == Mode::FunctionTool {
            "emit_result"
        } else {
            "none"
        }
    } else {
        cfg.tool_choice.as_deref().unwrap_or("auto")
    };
    if legacy && choice == "none" && native {
        return Err("legacy Responses cannot suppress declared native tools".into());
    }
    let messages = layout["messages"].as_array().ok_or("missing layout")?;
    let n = messages
        .iter()
        .take_while(|m| m["role"] == "system")
        .count();
    let rules = messages[..n]
        .iter()
        .map(|m| m["content"].as_str().ok_or("invalid program prefix"))
        .collect::<std::result::Result<Vec<_>, _>>()?
        .join("\n\n");
    let mut body = json!({"model":cfg.model,"input":messages[n..],"stream":false,"max_output_tokens":cfg.max_output,"instructions":rules});
    let mut tools = Vec::new();
    if let Some(local) = layout["tools"].as_array() {
        for tool in local {
            if legacy && choice == "none" {
                continue;
            }
            let mut function = tool["function"].clone();
            function["type"] = json!("function");
            tools.push(function);
        }
    }
    if native {
        tools.push(json!({"type":"web_search"}));
    }
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    if !legacy {
        body["tool_choice"] = if matches!(choice, "auto" | "none" | "required") {
            json!(choice)
        } else {
            json!({"type":"function","name":choice})
        };
    }
    if let Some(effort) = &cfg.reasoning_effort {
        body["reasoning"] = json!({"effort":effort});
    }
    if cfg.protocol == super::wire::Protocol::OpenaiResponses {
        body["store"] = json!(false);
        let mut include = vec!["reasoning.encrypted_content"];
        if native {
            include.push("web_search_call.action.sources");
        }
        body["include"] = json!(include);
    }
    if let Some(task) = task {
        match task.mode {
            Mode::TextJson => body["text"] = json!({"format":{"type":"json_object"}}),
            Mode::JsonSchema => {
                body["text"] = json!({"format":{"type":"json_schema","name":"result","strict":true,"schema":task.schema}})
            }
            Mode::FunctionTool => {}
        }
    }
    Ok(body)
}
pub fn complete(body: &Value) -> bool {
    body["status"] == "completed"
        && body["error"].is_null()
        && body["incomplete_details"].is_null()
        && body["output"].as_array().is_some_and(|items| {
            items.iter().all(|i| {
                i["status"].is_null()
                    || i["status"] == "completed"
                    || (i["type"] == "web_search_call" && i["status"] == "failed")
            })
        })
}
pub fn decode(raw: &str, task: Option<&Task>, cfg: &Config) -> Result<Turn> {
    let response: Value = serde_json::from_str(raw)?;
    if !complete(&response) {
        return Err("Responses body or output item is incomplete".into());
    }
    let output = response["output"]
        .as_array()
        .ok_or("missing ordered output")?;
    let mut text = String::new();
    let mut intents = Vec::new();
    let mut seen = HashSet::new();
    let mut item_ids = HashSet::new();
    let mut structured = None;
    let mut events = Vec::new();
    let mut citations = Vec::new();
    let mut has_message = false;
    for item in output {
        if let Some(id) = item["id"].as_str()
            && !item_ids.insert(id)
        {
            return Err("duplicate provider item id".into());
        }
        match item["type"].as_str() {
            Some("reasoning") => {} // Private provider state stays in the ordered original item.
            Some("message") => {
                if item["role"] != "assistant" {
                    return Err("unexpected Responses role".into());
                }
                has_message = true;
                for part in item["content"]
                    .as_array()
                    .ok_or("missing message content")?
                {
                    if part["type"] != "output_text" {
                        return Err("refusal or unsupported output message".into());
                    }
                    text.push_str(part["text"].as_str().ok_or("missing output text")?);
                    citations.extend(
                        part["annotations"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|a| a["type"] == "url_citation")
                            .cloned(),
                    );
                }
            }
            Some("web_search_call") => {
                if !cfg.native_web_search
                    || task.is_some()
                    || cfg.tool_choice.as_deref() == Some("none")
                {
                    return Err("undeclared native event".into());
                }
                events.push(item.clone());
            }
            Some("function_call") => {
                let id = item["call_id"]
                    .as_str()
                    .filter(|id| !id.is_empty() && seen.insert(*id))
                    .ok_or("missing or duplicate Responses call_id")?;
                let name = item["name"].as_str().ok_or("missing function name")?;
                let args: Value =
                    serde_json::from_str(item["arguments"].as_str().ok_or("missing arguments")?)?;
                if let Some(task) = task {
                    if task.mode != Mode::FunctionTool
                        || name != "emit_result"
                        || structured.is_some()
                    {
                        return Err("unexpected structured function".into());
                    }
                    structured = Some(args);
                } else {
                    let choice = cfg.tool_choice.as_deref().unwrap_or("auto");
                    if !matches!(choice, "auto" | "required") && choice != name {
                        return Err("function outside selected choice".into());
                    }
                    intents.push(chat::intent(id, name, args)?);
                }
            }
            _ => return Err("unsupported output item".into()),
        }
    }
    let result = if let Some(task) = task {
        let value = if task.mode == Mode::FunctionTool {
            if !text.trim().is_empty() {
                return Err("extra text with structured function".into());
            }
            structured.ok_or("missing emit_result")?
        } else {
            serde_json::from_str(&text)?
        };
        chat::validate_result(task, value)?
    } else {
        if cfg
            .tool_choice
            .as_deref()
            .is_some_and(|choice| !matches!(choice, "auto" | "none"))
            && intents.is_empty()
            && events.is_empty()
        {
            return Err("requested Responses tool not observed".into());
        }
        if !has_message && intents.is_empty() && events.is_empty() {
            return Err("no complete message or declared tool".into());
        }
        json!({"text":text,"output":output,"native_events":events,"citations":citations})
    };
    Ok(Turn {
        intents: Value::Array(intents),
        result,
    })
}

pub fn usage(raw: &str) -> Value {
    let body = serde_json::from_str::<Value>(raw).unwrap_or(Value::Null);
    let u = &body["usage"];
    let mut known = serde_json::Map::new();
    for (k, n) in [
        ("input", &u["input_tokens"]),
        ("output", &u["output_tokens"]),
        ("total", &u["total_tokens"]),
        ("cache", &u["input_tokens_details"]["cached_tokens"]),
        ("reasoning", &u["output_tokens_details"]["reasoning_tokens"]),
    ] {
        if let Some(n) = n.as_i64().filter(|n| *n >= 0) {
            known.insert(k.into(), json!(n));
        }
    }
    if known.is_empty() {
        Value::Null
    } else {
        Value::Object(known)
    }
}
