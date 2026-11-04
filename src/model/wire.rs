use super::{
    chat::{self, Task, Turn},
    responses,
};
use crate::{Result, runtime::main::Config};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    #[default]
    Chat,
    OpenaiResponses,
    DeepseekResponsesLegacy,
    DeepseekResponses,
}
impl Protocol {
    pub fn is_chat(&self) -> bool {
        *self == Self::Chat
    }
    pub fn request(self, layout: Value, cfg: &Config, task: Option<&Task>) -> Result<Value> {
        if self.is_chat() {
            Ok(layout)
        } else {
            responses::request(layout, cfg, task)
        }
    }
    pub fn decode(self, raw: &str, task: Option<&Task>, cfg: &Config) -> Result<Turn> {
        if self.is_chat() {
            chat::decode(raw, task)
        } else {
            responses::decode(raw, task, cfg)
        }
    }
    pub fn complete(self, body: &Value) -> bool {
        if self.is_chat() {
            matches!(
                body["choices"][0]["finish_reason"].as_str(),
                Some("stop" | "tool_calls")
            )
        } else {
            responses::complete(body)
        }
    }
    pub fn usage(self, raw: &str) -> Value {
        if self.is_chat() {
            chat::usage(raw)
        } else {
            responses::usage(raw)
        }
    }
    fn items(self, body: &mut Value) -> Result<&mut Vec<Value>> {
        body[if self.is_chat() { "messages" } else { "input" }]
            .as_array_mut()
            .ok_or_else(|| "missing paid conversation items".into())
    }
    pub fn replay(self, body: &mut Value, original: &Value) -> Result<()> {
        if self.is_chat() {
            self.items(body)?
                .push(original["choices"][0]["message"].clone());
        } else {
            self.items(body)?.extend(
                original["output"]
                    .as_array()
                    .ok_or("original ordered output missing")?
                    .iter()
                    .cloned(),
            );
        }
        Ok(())
    }
    pub fn output(self, body: &mut Value, call_id: &str, receipt: &str) -> Result<()> {
        let item = if self.is_chat() {
            json!({"role":"tool","tool_call_id":call_id,"content":receipt})
        } else {
            json!({"type":"function_call_output","call_id":call_id,"output":receipt})
        };
        self.items(body)?.push(item);
        Ok(())
    }
    pub fn user(self, body: &mut Value, text: &str) -> Result<()> {
        self.items(body)?
            .push(json!({"role":"user","content":text}));
        Ok(())
    }
}
