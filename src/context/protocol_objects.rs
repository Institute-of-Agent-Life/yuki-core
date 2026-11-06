//! Original provider messages stay on their paid chain. A new chain retains portable
//! facts only from the normal response and its immutable, paired effect receipts.
use crate::{Result, model::wire::Protocol};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub fn key(p: Protocol) -> &'static str {
    if p.is_gemini() {
        "contents"
    } else if p.is_chat() || p.is_messages() {
        "messages"
    } else {
        "input"
    }
}
pub fn parts(p: Protocol, body: &Value) -> Result<Vec<Value>> {
    let entries = body[key(p)]
        .as_array()
        .ok_or("missing original native messages")?;
    if !p.is_messages() {
        return Ok(entries.clone());
    }
    let mut parts = Vec::new();
    for message in entries {
        for block in message["content"]
            .as_array()
            .ok_or("missing original Messages blocks")?
        {
            let mut block = block.clone();
            if message["role"] == "user"
                && let Some(map) = block.as_object_mut()
            {
                map.remove("cache_control");
            }
            parts.push(json!({"role":message["role"],"block":block}));
        }
    }
    Ok(parts)
}
pub fn body_with(p: Protocol, template: &Value, items: &[Value]) -> Result<Value> {
    let mut body = template.clone();
    if !p.is_messages() {
        body[key(p)] = json!(items);
        return Ok(body);
    }
    let mut turns = Vec::<Value>::new();
    for item in items {
        if let Some(last) = turns.last_mut().filter(|m| m["role"] == item["role"]) {
            last["content"]
                .as_array_mut()
                .ok_or("invalid Messages group")?
                .push(item["block"].clone());
        } else {
            turns.push(json!({"role":item["role"],"content":[item["block"]]}));
        }
    }
    body["messages"] = json!(turns);
    crate::model::messages::cache_last(&mut body)?;
    Ok(body)
}
pub fn text(p: Protocol, role: &str, content: &str) -> Value {
    if p.is_messages() {
        json!({"role":role,"block":{"type":"text","text":content}})
    } else if p.is_gemini() {
        json!({"role":if role=="assistant"{"model"}else{role},"parts":[{"text":content}]})
    } else {
        json!({"role":role,"content":content})
    }
}
pub fn user_text(p: Protocol, unit: &Value) -> Option<&str> {
    if unit["role"] != "user" {
        return None;
    }
    if p.is_messages() {
        unit["block"]["text"].as_str()
    } else if p.is_gemini() {
        unit["parts"].as_array().filter(|p| p.len() == 1)?.first()?["text"].as_str()
    } else {
        unit["content"].as_str()
    }
}
pub struct Round {
    pub logical: String,
    pub parts: Vec<Value>,
    pub portable: Vec<Value>,
    pub calls: bool,
}
pub async fn history(
    conn: &mut SqliteConnection,
    exec: &str,
    cfg: &crate::runtime::main::Config,
) -> Result<Vec<Round>> {
    let p = cfg.protocol;
    let records=sqlx::query("SELECT r.id,r.request_json,a.response FROM model_requests r JOIN model_attempts a ON a.logical=r.id WHERE r.activation=? AND a.state IN('response','paired') AND a.response IS NOT NULL ORDER BY r.ordinal")
        .bind(format!("main:{exec}")).fetch_all(&mut *conn).await?;
    let mut result = Vec::new();
    for record in records {
        let logical: String = record.try_get("id")?;
        let mut body: Value = serde_json::from_str(record.try_get("request_json")?)?;
        let start = parts(p, &body)?.len();
        let raw: Value = serde_json::from_str(record.try_get("response")?)?;
        p.replay(
            &mut body,
            &crate::model::profile_store::wire::replay(cfg, &raw),
        )?;
        let intents=sqlx::query("SELECT i.ordinal,i.call_id,i.operation_id,i.paired_receipt,o.payload,o.action,o.target FROM model_tool_intents i JOIN operations o ON o.namespace=i.execution AND o.operation_id=i.operation_id WHERE i.logical=? ORDER BY i.ordinal")
          .bind(&logical).fetch_all(&mut *conn).await?;
        let mut calls = Vec::new();
        for row in &intents {
            let id: &str = row.try_get("call_id")?;
            let Some(receipt) = row.try_get::<Option<String>, _>("paired_receipt")? else {
                return Err("unpaired original tool result".into());
            };
            let name = if row.get::<String, _>("action") == "external" {
                row.get::<String, _>("target")
            } else {
                row.get("action")
            };
            let native_args = original_arguments(p, &raw, id);
            let args = native_args.unwrap_or_else(|| row.get::<String, _>("payload"));
            let receipt = crate::host::selected_processor::receipt(
                conn,
                exec,
                row.try_get("operation_id")?,
                &receipt,
            )
            .await?;
            p.output(&mut body, id, receipt)?;
            calls.push(json!({"call_id":id,"effect_key":row.get::<String,_>("operation_id"),"tool":name,"arguments":args,"result":receipt}));
        }
        for row in &intents {
            crate::media::append_image(conn, cfg, exec, row.try_get("operation_id")?, &mut body)
                .await?;
        }
        let paid_parts = parts(p, &body)?;
        let pieces = paid_parts
            .get(start..)
            .ok_or("original response shortened native sequence")?
            .to_vec();
        let content = visible(p, &raw);
        let portable = if !calls.is_empty() {
            let evidence = json!({"kind":"work_unobserved_tool_round","content":content,"calls":calls,
                "instruction":"These are original recorded tool results awaiting observation. Continue from these results; do not replay the original calls."});
            vec![text(p, "user", &evidence.to_string())]
        } else if let Some(s) = content.filter(|s| !s.is_empty()) {
            vec![text(p, "assistant", &s)]
        } else {
            vec![]
        };
        result.push(Round {
            logical,
            parts: pieces,
            portable,
            calls: !calls.is_empty(),
        });
    }
    Ok(result)
}
fn original_arguments(p: Protocol, raw: &Value, id: &str) -> Option<String> {
    let calls = if p.is_chat() {
        &raw["choices"][0]["message"]["tool_calls"]
    } else if !p.is_messages() && !p.is_gemini() {
        &raw["output"]
    } else {
        return None;
    };
    calls.as_array()?.iter().find_map(|c| {
        if p.is_chat() && c["id"] == id {
            c["function"]["arguments"].as_str().map(str::to_owned)
        } else if !p.is_chat() && c["type"] == "function_call" && c["call_id"] == id {
            c["arguments"].as_str().map(str::to_owned)
        } else {
            None
        }
    })
}
fn visible(p: Protocol, raw: &Value) -> Option<String> {
    if p.is_chat() {
        return raw["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_owned);
    }
    let value = if p.is_messages() {
        raw["content"]
            .as_array()?
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<String>()
    } else if p.is_gemini() {
        raw["candidates"][0]["content"]["parts"]
            .as_array()?
            .iter()
            .filter(|b| b["thought"] != true)
            .filter_map(|b| b["text"].as_str())
            .collect()
    } else {
        raw["output"]
            .as_array()?
            .iter()
            .filter(|o| o["type"] == "message")
            .flat_map(|m| m["content"].as_array().into_iter().flatten())
            .filter(|b| b["type"] == "output_text")
            .filter_map(|b| b["text"].as_str())
            .collect()
    };
    (!value.is_empty()).then_some(value)
}
