use super::Cost;
use crate::{Result, model::wire::Protocol};
use serde_json::{Value, json};

fn sequence(p: Protocol) -> &'static str {
    if p.is_gemini() {
        "contents"
    } else if p.is_chat() || p.is_messages() {
        "messages"
    } else {
        "input"
    }
}
fn encode_public(p: Protocol, item: &Value) -> Result<Value> {
    let text = item["content"]
        .as_str()
        .ok_or("public fragment is not text")?;
    let role = item["role"]
        .as_str()
        .ok_or("public fragment role missing")?;
    Ok(if p.is_gemini() {
        json!({"role":if role=="assistant"{"model"}else{"user"},"parts":[{"text":text}]})
    } else if p.is_messages() {
        json!({"role":role,"block":{"type":"text","text":text}})
    } else {
        item.clone()
    })
}
/// Replace only a prefix verified against the receipt from which this body was cloned.
/// Native continuation blocks and paired receipts outside that prefix retain their exact values.
pub(super) fn replace(
    body: &mut Value,
    p: Protocol,
    before: &[Value],
    after: &[Value],
) -> Result<()> {
    let items = body[sequence(p)]
        .as_array_mut()
        .ok_or("native input sequence missing")?;
    let expected: Vec<_> = before
        .iter()
        .map(|v| encode_public(p, v))
        .collect::<Result<_>>()?;
    let replacement: Vec<_> = after
        .iter()
        .map(|v| encode_public(p, v))
        .collect::<Result<_>>()?;
    if p.is_messages() {
        let mut flat = Vec::new();
        for item in items.iter() {
            for block in item["content"]
                .as_array()
                .ok_or("invalid original Messages blocks")?
            {
                flat.push(json!({"role":item["role"],"block":block}));
            }
        }
        if !flat.starts_with(&expected) {
            return Err("original public Messages prefix changed".into());
        }
        flat.splice(..expected.len(), replacement);
        let mut messages = Vec::<Value>::new();
        for item in flat {
            if let Some(last) = messages
                .last_mut()
                .filter(|last| last["role"] == item["role"])
            {
                last["content"]
                    .as_array_mut()
                    .unwrap()
                    .push(item["block"].clone());
            } else {
                messages.push(json!({"role":item["role"],"content":[item["block"]]}));
            }
        }
        *items = messages;
    } else {
        let start = if p.is_chat() {
            items.iter().take_while(|m| m["role"] == "system").count()
        } else {
            0
        };
        if !items[start..].starts_with(&expected) {
            return Err("original public prefix changed".into());
        }
        items.splice(start..start + expected.len(), replacement);
    }
    Ok(())
}
pub(super) fn opaque(p: Protocol, body: &Value) -> bool {
    body[sequence(p)].as_array().into_iter().flatten().any(|m| {
        if p.is_messages() {
            return m["content"].as_array().into_iter().flatten().any(|b| {
                matches!(
                    b["type"].as_str(),
                    Some(
                        "thinking"
                            | "redacted_thinking"
                            | "server_tool_use"
                            | "web_search_tool_result"
                    )
                )
            });
        }
        if p.is_gemini() {
            return m["parts"].as_array().into_iter().flatten().any(|part| {
                part.get("thoughtSignature").is_some()
                    || part.get("thought_signature").is_some()
                    || part["thought"] == true
            });
        }
        !p.is_chat() && (m["type"] == "reasoning" || m.get("encrypted_content").is_some())
    })
}
fn normalized_tools(p: Protocol, body: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    for t in body["tools"].as_array().into_iter().flatten() {
        if p.is_gemini() {
            for f in t["functionDeclarations"].as_array().into_iter().flatten() {
                out.push(json!({"name":f["name"],"description":f["description"],"parameters":f.get("parametersJsonSchema").or(f.get("parameters")).cloned().unwrap_or_default()}));
            }
        } else {
            let f = if p.is_chat() { &t["function"] } else { t };
            if f["name"].is_string() {
                out.push(json!({"name":f["name"],"description":f["description"],"parameters":if p.is_messages(){&f["input_schema"]}else{&f["parameters"]}}));
            }
        }
    }
    out
}
pub(super) fn base(cfg: &crate::runtime::main::Config, paid: &Value) -> Value {
    let mut view = json!({"messages":[],"model":cfg.model,"max_output_tokens":cfg.max_output});
    let tools = normalized_tools(cfg.protocol, paid);
    if !tools.is_empty() {
        view["tools"] = json!(tools);
    }
    if let Some(choice) = paid["tool_choice"].as_str().or(cfg.tool_choice.as_deref()) {
        view["tool_choice"] = json!(choice);
    }
    if let Some(effort) = &cfg.reasoning_effort {
        view["reasoning_effort"] = json!(effort);
    }
    if cfg.messages_thinking {
        view["thinking_enabled"] = json!(true);
    }
    if let Some(format) = paid.get("response_format") {
        view["response_format"] = format.clone();
    }
    view
}
/// Add the JSON separators and round once for the complete request. Index both
/// prefix and retained suffix costs without serializing the conversation at each cut.
#[derive(Default, Clone, Copy)]
pub(crate) struct Count {
    value: Cost,
    items: usize,
}
impl Count {
    pub(crate) fn join(mut self, rhs: Self) -> Self {
        if self.items != 0 && rhs.items != 0 {
            self.value.ascii += 2;
        }
        self.value.add(rhs.value);
        self.items += rhs.items;
        self
    }
    pub(crate) fn tokens(self) -> u64 {
        self.value.tokens()
    }
    fn item(value: &Value) -> Self {
        Self {
            value: Cost::value(value),
            items: 1,
        }
    }
}
pub(crate) fn start(cfg: &crate::runtime::main::Config, body: &Value) -> Count {
    let p = cfg.protocol;
    let mut input = base(cfg, body);
    if cfg.native_web_search {
        input["native_tools"] = body["tools"]
            .as_array()
            .map(|v| {
                json!(
                    v.iter()
                        .filter(|t| t["type"] == "web_search")
                        .collect::<Vec<_>>()
                )
            })
            .unwrap_or_default();
    }
    let mut count = Count {
        value: Cost::value(&input),
        items: 0,
    };
    let mut system = |rules: &Value| {
        count = count.join(Count::item(&json!({"role":"system","content":rules})));
    };
    if body["instructions"].is_string() {
        system(&body["instructions"]);
    }
    if p.is_messages() {
        for block in body["system"].as_array().into_iter().flatten() {
            system(&block["text"]);
        }
    } else if p.is_gemini() {
        for part in body["systemInstruction"]["parts"]
            .as_array()
            .into_iter()
            .flatten()
        {
            system(&part["text"]);
        }
    }
    count
}
pub(crate) fn part_cost(p: Protocol, unit: &Value) -> Count {
    if p.is_messages() {
        let block = &unit["block"];
        if block["type"] == "image"
            && block["source"]["type"] == "base64"
            && block["source"]["data"].is_string()
        {
            let mut count =
                Count::item(&json!({"role":unit["role"],"content":{"image":"[prepared media]"}}));
            count.value.media = 4096;
            return count;
        }
        return Count::item(
            &json!({"role":unit["role"],"content":if block["type"]=="text"{&block["text"]}else{block}}),
        );
    }
    let mut normalized = unit.clone();
    let mut media = 0;
    if p.is_gemini()
        && let Some(parts) = normalized["parts"].as_array_mut()
    {
        for part in parts {
            for key in ["inlineData", "inline_data"] {
                if part.get(key).is_some_and(|v| v["data"].is_string()) {
                    part[key] = json!("[prepared media]");
                    media += 4096;
                }
            }
        }
    } else if normalized["role"] == "user"
        && let Some(parts) = normalized["content"].as_array_mut()
    {
        for part in parts {
            let uri = if p.is_chat() && part["type"] == "image_url" {
                part["image_url"]["url"].as_str()
            } else if !p.is_chat() && part["type"] == "input_image" {
                part["image_url"].as_str()
            } else {
                None
            };
            if uri.is_some_and(|u| u.starts_with("data:")) {
                *part = json!({"data_url":"[prepared media]"});
                media += 4096;
            }
        }
    }
    let mut c = Count::item(&normalized);
    c.value.media += media;
    c
}
pub(crate) fn estimate(cfg: &crate::runtime::main::Config, body: &Value) -> u64 {
    let mut count = start(cfg, body);
    let p = cfg.protocol;
    for msg in body[sequence(p)].as_array().into_iter().flatten() {
        if p.is_messages() {
            for block in msg["content"].as_array().into_iter().flatten() {
                count = count.join(part_cost(p, &json!({"role":msg["role"],"block":block})));
            }
        } else {
            count = count.join(part_cost(p, msg));
        }
    }
    count.tokens()
}
pub(crate) fn summary(p: Protocol, raw: &str) -> Result<String> {
    let v: Value = serde_json::from_str(raw)?;
    let text = if p.is_chat() {
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_default()
    } else if p.is_messages() {
        v["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect()
    } else if p.is_gemini() {
        v["candidates"][0]["content"]["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["thought"] != true)
            .filter_map(|b| b["text"].as_str())
            .collect()
    } else {
        v["output"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|o| o["type"] == "message")
            .flat_map(|m| m["content"].as_array().into_iter().flatten())
            .filter(|b| b["type"] == "output_text")
            .filter_map(|b| b["text"].as_str())
            .collect()
    };
    if text.trim().is_empty() {
        return Err("blank public checkpoint".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::main::Config;

    #[test]
    fn responses_and_messages_charge_binary_only_in_native_image_blocks() {
        for p in [Protocol::OpenaiResponses, Protocol::AnthropicMessages] {
            let unit = |data: &str| {
                if p.is_messages() {
                    json!({"role":"user","block":{"type":"image","source":{"type":"base64","data":data}}})
                } else {
                    json!({"role":"user","content":[{"type":"input_image","image_url":format!("data:image/png;base64,{data}")}]})
                }
            };
            let short = part_cost(p, &unit("eA=="));
            let long = part_cost(p, &unit(&"A".repeat(8000)));
            assert_eq!(short.tokens(), long.tokens());
            assert_eq!(short.value.media, 4096);
            let nested = |data: &str| {
                if p.is_messages() {
                    json!({"role":"assistant","block":{"type":"tool_use","input":unit(data)}})
                } else {
                    json!({"type":"function_call", "arguments":unit(data).to_string()})
                }
            };
            assert!(
                part_cost(p, &nested(&"A".repeat(8000))).tokens()
                    > part_cost(p, &nested("x")).tokens() + 2000
            );
        }
    }
    #[test]
    fn inline_media_charged_at_original_part_only() {
        let cfg: Config = serde_json::from_value(json!({"protocol":"gemini","model":"fixture","endpoint":"http://127.0.0.1","name":"Yuki","merge_ms":0,"max_input_units":200000,"max_output":128,"root_slots":5})).unwrap();
        let item = |part| json!({"contents":[{"role":"user","parts":[part]}]});
        let photo = |data: &str| item(json!({"inlineData":{"mimeType":"image/png","data":data}}));
        let expected = estimate(&cfg, &item(json!({"inlineData":"[prepared media]"}))) + 4096;
        assert_eq!(estimate(&cfg, &photo("c2hvcnQ=")), expected);
        assert_eq!(estimate(&cfg, &photo(&"A".repeat(8000))), expected);
        let args = |data: &str| item(json!({"functionCall":{"args":{"inlineData":{"data":data}}}}));
        assert!(estimate(&cfg, &args(&"A".repeat(8000))) > estimate(&cfg, &args("x")) + 2000);
    }
}
