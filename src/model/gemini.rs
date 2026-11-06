use super::chat::{self, Mode, Task, Turn};
use crate::{Result, runtime::main::Config};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub fn request(layout: Value, cfg: &Config, task: Option<&Task>) -> Result<Value> {
    let mut contents = Vec::new();
    let mut system = Vec::new();
    for m in layout["messages"]
        .as_array()
        .ok_or("missing program layout")?
    {
        let part = json!({"text":m["content"].as_str().ok_or("invalid program text")?});
        match m["role"].as_str() {
            Some("system") => system.push(part),
            Some("assistant") => contents.push(json!({"role":"model","parts":[part]})),
            Some("user") => contents.push(json!({"role":"user","parts":[part]})),
            _ => return Err("unsupported initial GenerateContent role".into()),
        }
    }
    let mut body =
        json!({"contents":contents,"generationConfig":{"maxOutputTokens":cfg.max_output}});
    if !system.is_empty() {
        body["systemInstruction"] = json!({"parts":system});
    }
    let generation = &mut body["generationConfig"];
    if let Some(effort) = &cfg.reasoning_effort {
        generation["thinkingConfig"] = json!({"thinkingLevel":effort});
    }
    if let Some(t) = task
        && t.mode != Mode::FunctionTool
    {
        generation["responseMimeType"] = json!("application/json");
        if t.mode == Mode::JsonSchema {
            generation["responseJsonSchema"] = t.schema.clone();
        }
    }
    let mut tools = Vec::new();
    let native = cfg.native_web_search && task.is_none();
    if let Some(decls) = layout["tools"].as_array().filter(|x| !x.is_empty()) {
        if native && !cfg.gemini_mixed_tools {
            return Err("profile disallows Google Search mixed with local functions".into());
        }
        tools.push(json!({"functionDeclarations":decls.iter().map(|d|{let f=&d["function"];json!({"name":f["name"],"description":f["description"].as_str().unwrap_or(""),"parametersJsonSchema":f["parameters"]})}).collect::<Vec<_>>()}));
        let choice = if task.is_some() {
            "emit_result"
        } else {
            cfg.tool_choice.as_deref().unwrap_or("auto")
        };
        let mut function = json!({"mode":match choice{"auto" if native=>"VALIDATED","auto"=>"AUTO","none"=>"NONE",_=>"ANY"}});
        if !matches!(choice, "auto" | "none" | "required") {
            function["allowedFunctionNames"] = json!([choice]);
        }
        body["toolConfig"] = json!({"functionCallingConfig":function});
    }
    if native {
        tools.push(json!({"googleSearch":{}}));
        body["toolConfig"]["includeServerSideToolInvocations"] = json!(true);
    }
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    Ok(body)
}
pub fn complete(body: &Value) -> bool {
    body["error"].is_null()
        && body["promptFeedback"]["blockReason"].is_null()
        && body["candidates"].as_array().is_some_and(|c| {
            c.len() == 1
                && c[0]["finishReason"] == "STOP"
                && c[0]["content"]["parts"].is_array()
                && (c[0]["content"]["role"].is_null() || c[0]["content"]["role"] == "model")
        })
}
fn internal_id(parts: &Value, index: usize) -> String {
    let hash = Sha256::digest(parts.to_string().as_bytes());
    format!("gemini_{hash:x}_{index}")
}
fn call_id(call: &Value, parts: &Value, index: usize) -> Result<String> {
    match &call["id"] {
        Value::Null => Ok(internal_id(parts, index)),
        Value::String(id) if !id.is_empty() => Ok(id.clone()),
        _ => Err("invalid native function ID".into()),
    }
}
fn native_enabled(body: &Value) -> bool {
    body["tools"]
        .as_array()
        .is_some_and(|v| v.iter().any(|t| t["googleSearch"].is_object()))
}
pub fn replay(body: &mut Value, v: &Value) -> Result<()> {
    let mut content = v["candidates"][0]["content"].clone();
    if content["role"].is_null() {
        content["role"] = json!("model");
    }
    body["contents"]
        .as_array_mut()
        .ok_or("missing paid contents")?
        .push(content);
    Ok(())
}
pub fn user(body: &mut Value, text: &str) -> Result<()> {
    body["contents"]
        .as_array_mut()
        .ok_or("missing paid contents")?
        .push(json!({"role":"user","parts":[{"text":text}]}));
    Ok(())
}
pub fn output(body: &mut Value, id: &str, receipt: &str) -> Result<()> {
    let messages = body["contents"]
        .as_array_mut()
        .ok_or("missing paid contents")?;
    let origin = messages
        .iter()
        .rev()
        .find(|m| m["role"] == "model")
        .ok_or("missing original model Content")?;
    let parts = &origin["parts"];
    let mut original = None;
    for (index, part) in parts
        .as_array()
        .ok_or("missing original model parts")?
        .iter()
        .enumerate()
    {
        let call = &part["functionCall"];
        if call.is_object() && call_id(call, parts, index)? == id {
            original = Some(call);
            break;
        }
    }
    let call = original.ok_or("no original Gemini call for receipt")?;
    let r: Value = serde_json::from_str(receipt)?;
    let failed = matches!(r["state"].as_str(), Some("unknown" | "failed")) || r["ok"] == false;
    let mut function =
        json!({"name":call["name"],"response":{if failed{"error"}else{"output"}:receipt}});
    if let Some(native) = call["id"].as_str() {
        function["id"] = json!(native);
    }
    let response = json!({"functionResponse":function});
    if let Some(m) = messages.last_mut().filter(|m| {
        m["role"] == "user"
            && m["parts"]
                .as_array()
                .is_some_and(|p| p.iter().all(|i| i["functionResponse"].is_object()))
    }) {
        m["parts"].as_array_mut().unwrap().push(response);
    } else {
        messages.push(json!({"role":"user","parts":[response]}));
    }
    Ok(())
}

pub fn decode(v: &Value, task: Option<&Task>, paid: &Value) -> Result<Turn> {
    if !complete(v) {
        return Err("incomplete GenerateContent candidate".into());
    }
    let candidate = &v["candidates"][0];
    let parts = &candidate["content"]["parts"];
    let mut ids = HashSet::new();
    let mut intents = Vec::new();
    let mut emitted = None;
    let mut text = String::new();
    let mut media = Vec::new();
    let mut server = HashSet::new();
    let mut events = Vec::new();
    let native = native_enabled(paid);
    let config = &paid["toolConfig"]["functionCallingConfig"];
    let decls = paid["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|tool| {
            tool["functionDeclarations"]
                .as_array()
                .into_iter()
                .flatten()
        })
        .collect::<Vec<_>>();
    for (index, p) in parts.as_array().unwrap().iter().enumerate() {
        if [
            "text",
            "functionCall",
            "toolCall",
            "toolResponse",
            "inlineData",
            "fileData",
        ]
        .iter()
        .filter(|k| p.get(**k).is_some())
        .count()
            != 1
        {
            return Err("GenerateContent Part must have exactly one payload".into());
        }
        if let Some(t) = p["text"].as_str() {
            if p["thought"] != true {
                text.push_str(t);
            }
        } else if let Some(call) = p.get("functionCall") {
            let id = call_id(call, parts, index)?;
            if !ids.insert(id.clone()) {
                return Err("duplicate Gemini function ID".into());
            }
            let name = call["name"].as_str().ok_or("missing function name")?;
            let args = call.get("args").cloned().unwrap_or_else(|| json!({}));
            let decl = decls.iter().find(|d| d["name"] == name);
            let allowed = config["mode"] != "NONE"
                && config["allowedFunctionNames"]
                    .as_array()
                    .is_none_or(|v| v.iter().any(|n| n == name));
            let error = super::tool_rejection::validate(
                decl.map(|d| &d["parametersJsonSchema"]),
                &args,
                allowed,
            )?;
            if let Some(t) = task {
                if error.is_some()
                    || t.mode != Mode::FunctionTool
                    || name != "emit_result"
                    || emitted.is_some()
                {
                    return Err("invalid Gemini structured function".into());
                }
                emitted = Some(args);
            } else {
                let mut intent = chat::intent(&id, name, args)?;
                if let Some(error) = error {
                    super::tool_rejection::mark(&mut intent, error);
                }
                intents.push(intent);
            }
        } else if let Some(call) = p.get("toolCall") {
            let id = call["id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("missing Gemini server ID")?;
            if !native
                || !matches!(
                    call["toolType"].as_str(),
                    Some("GOOGLE_SEARCH" | "GOOGLE_SEARCH_WEB")
                )
                || !server.insert(id)
                || !ids.insert(id.to_owned())
            {
                return Err("unknown or duplicate server tool call".into());
            }
            events.push(json!({"id":id,"status":"searching","args":call["args"]}));
        } else if let Some(result) = p.get("toolResponse") {
            let id = result["id"]
                .as_str()
                .ok_or("missing Gemini server result ID")?;
            if !native || !server.remove(id) {
                return Err("unpaired Gemini server tool result".into());
            }
            let failed = !result["response"]["error"].is_null() || !result["error"].is_null();
            events.push(
                json!({"id":id,"status":if failed{"failed"}else{"completed"},"result":result}),
            );
        } else if let Some(data) = p.get("inlineData").or(p.get("fileData")) {
            if !data["mimeType"].is_string()
                || !(data["data"].is_string() || data["fileUri"].is_string())
            {
                return Err("invalid generated media part".into());
            }
            media.push(json!({"index":index,"media":data}));
        } else {
            return Err("unsupported Gemini part".into());
        }
    }
    let grounding = &candidate["groundingMetadata"];
    if !grounding.is_null() && !native {
        return Err("grounding from undeclared search".into());
    }
    let citations: Vec<_> = grounding["groundingChunks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let w = &c["web"];
            w["uri"]
                .as_str()
                .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
                .map(|_| w.clone())
        })
        .collect();
    let result = if let Some(t) = task {
        let value = if t.mode == Mode::FunctionTool {
            if !text.trim().is_empty() {
                return Err("unexpected text with structured Gemini function".into());
            }
            emitted.ok_or("missing Gemini emit_result")?
        } else {
            serde_json::from_str(&text)?
        };
        chat::validate_result(t, value)?
    } else {
        if config["mode"] == "ANY" && intents.is_empty() {
            return Err("Gemini required function missing".into());
        }
        if text.is_empty()
            && intents.is_empty()
            && events.is_empty()
            && media.is_empty()
            && grounding.is_null()
        {
            return Err("no complete Gemini result".into());
        }
        json!({"text":text,"native_events":events,"grounding":grounding,"citations":citations,"media":media})
    };
    Ok(Turn {
        intents: json!(intents),
        result,
    })
}
pub fn usage(raw: &str) -> Value {
    let v = serde_json::from_str::<Value>(raw).unwrap_or(Value::Null);
    let u = &v["usageMetadata"];
    let mut known = serde_json::Map::new();
    for (name, field) in [
        ("input", "promptTokenCount"),
        ("cache", "cachedContentTokenCount"),
        ("reasoning", "thoughtsTokenCount"),
        ("total", "totalTokenCount"),
        ("candidate_output", "candidatesTokenCount"),
    ] {
        if let Some(n) = u[field].as_i64().filter(|n| *n >= 0) {
            known.insert(name.into(), json!(n));
        }
    }
    // The fixed adapter counts thought tokens alongside visible candidates. Preserve raw known fields.
    if let Some(n) = known
        .get("candidate_output")
        .and_then(Value::as_i64)
        .and_then(|n| n.checked_add(known.get("reasoning").and_then(Value::as_i64).unwrap_or(0)))
    {
        known.insert("output".into(), json!(n));
    }
    if known.is_empty() {
        Value::Null
    } else {
        Value::Object(known)
    }
}
