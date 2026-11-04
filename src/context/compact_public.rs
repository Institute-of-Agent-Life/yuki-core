pub(crate) mod history;
pub(crate) mod job;
pub(crate) mod wire;
use crate::Result;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const INSTRUCTION: &str = "[Host 会话上下文压缩要求]\n整理以上共同聊天历史，保留人物归属、决定、否定约束、最新纠正、未决问题及可回查的内部事件引用。不执行工具、不发送消息、不写长期记忆；仅返回可续接的文字检查点。";
pub fn checkpoint(summary: &str, coverage: i64) -> Value {
    json!({"role":"user","content":format!("[Conversation compact checkpoint]\n[Source events through internal event_id={coverage}]\n{summary}")})
}
pub fn hash(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}

/// The original estimator rounds the whole JSON input once, after charging spaces at
/// its list/object delimiters. Binary media is charged only at a native media slot.
#[derive(Default, Copy, Clone)]
struct Cost {
    ascii: u64,
    non_ascii: u64,
    media: u64,
}
impl Cost {
    fn text(&mut self, v: &str) {
        for ch in v.chars() {
            if ch.is_ascii() {
                self.ascii += 1;
            } else {
                self.non_ascii += 1;
            }
        }
    }
    fn add(&mut self, c: Self) {
        self.ascii += c.ascii;
        self.non_ascii += c.non_ascii;
        self.media += c.media;
    }
    fn tokens(self) -> u64 {
        self.ascii.div_ceil(3) + 2 * self.non_ascii + self.media
    }
    fn value(v: &Value) -> Self {
        let mut c = Self::default();
        match v {
            Value::Array(items) => {
                c.ascii = 2 + (items.len().saturating_sub(1) * 2) as u64;
                for v in items {
                    c.add(Self::value(v));
                }
            }
            Value::Object(obj) => {
                c.ascii = 2 + (obj.len().saturating_sub(1) * 2 + obj.len() * 2) as u64;
                for (k, v) in obj {
                    c.text(&json!(k).to_string());
                    c.add(Self::value(v));
                }
            }
            _ => c.text(&v.to_string()),
        }
        c
    }
}
pub fn input_budget(input: Option<u64>, window: Option<u64>, output: u64) -> Option<u64> {
    [input, window.map(|w| w.saturating_sub(output))]
        .into_iter()
        .flatten()
        .min()
}
pub fn scan(
    base: &Value,
    leading: &[Value],
    fragments: &[Value],
    focus: &str,
    limit: Option<u64>,
) -> Result<Option<usize>> {
    if fragments.is_empty() {
        return Ok(None);
    }
    let Some(limit) = limit else {
        return Ok(Some(fragments.len()));
    };
    if base
        .get("messages")
        .and_then(Value::as_array)
        .is_none_or(|m| !m.is_empty())
    {
        return Err("compact accounting base must have an empty transcript".into());
    }
    let instruction = if focus.is_empty() {
        INSTRUCTION.to_owned()
    } else {
        format!("{INSTRUCTION}\n整理重点：{focus}")
    };
    let extra = Cost::value(&json!({"role":"user","content":instruction}));
    let mut cost = Cost::value(base);
    let mut count = 0;
    for value in leading {
        if count > 0 {
            cost.ascii += 2;
        }
        cost.add(Cost::value(value));
        count += 1;
    }
    let mut cut = 0;
    for (i, fragment) in fragments.iter().enumerate() {
        if count > 0 {
            cost.ascii += 2;
        }
        cost.add(Cost::value(fragment));
        count += 1;
        let mut trial = cost;
        trial.ascii += 2;
        trial.add(extra);
        if trial.tokens() > limit {
            break;
        }
        cut = i + 1;
    }
    Ok((cut > 0).then_some(cut))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_public_cost_and_legal_cuts() {
        let gold: Value =
            serde_json::from_str(include_str!("../../fixtures/compact/v4-public-plan.json"))
                .unwrap();
        assert_eq!(gold["instruction"], INSTRUCTION);
        let base = json!({"messages":[],"model":"fixture","max_output_tokens":128,"tool_choice":"auto","tools":[{"name":"send","description":"send text","parameters":{"type":"object"}}]});
        let fragments = gold["fragments"].as_array().unwrap();
        let items: Vec<_> = fragments.iter().map(|f| f["message"].clone()).collect();
        for case in gold["cases"].as_array().unwrap() {
            let cut = scan(
                &base,
                std::slice::from_ref(&gold["static"]),
                &items,
                "",
                case["budget"].as_u64(),
            )
            .unwrap();
            let coverage =
                cut.and_then(|n| fragments[n - 1]["event_ids"].as_array()?.last()?.as_i64());
            assert_eq!(case["coverage"].as_i64(), coverage);
            if let Some(messages) = case["messages"].as_array() {
                let mut request = base.clone();
                request["messages"] = json!(messages);
                assert_eq!(
                    Some(Cost::value(&request).tokens()),
                    case["request_tokens"].as_u64()
                );
            }
        }
        assert_eq!(checkpoint("gold", 5), gold["checkpoint"]);
        assert_eq!(input_budget(None, None, 128), None);
        assert_eq!(input_budget(Some(50), Some(100), 80), Some(20));
    }
}
