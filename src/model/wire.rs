use super::tool_rejection::{self, Rejection};
use super::{
    chat::{self, Task, Turn},
    gemini, messages, responses,
};
use crate::{Result, runtime::main::Config};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    #[default]
    Chat,
    OpenaiResponses,
    DeepseekResponsesLegacy,
    DeepseekResponses,
    AnthropicMessages,
    Gemini,
}
impl Protocol {
    pub fn is_chat(&self) -> bool {
        *self == Self::Chat
    }
    pub fn is_gemini(self) -> bool {
        self == Self::Gemini
    }
    pub fn is_messages(self) -> bool {
        self == Self::AnthropicMessages
    }
    pub fn request(self, layout: Value, cfg: &Config, task: Option<&Task>) -> Result<Value> {
        let mut body = if self.is_gemini() {
            gemini::request(layout, cfg, task)?
        } else if self.is_chat() {
            layout
        } else if self.is_messages() {
            messages::request(layout, cfg, task)?
        } else {
            responses::request(layout, cfg, task)?
        };
        super::profile_store::wire::configure(cfg, &mut body)?;
        Ok(body)
    }

    pub fn decode(
        self,
        raw: &str,
        task: Option<&Task>,
        cfg: &Config,
        paid: &Value,
    ) -> Result<Turn> {
        let response: Value = serde_json::from_str(raw)?;
        if self.is_messages() {
            return messages::decode(&response, task, cfg, paid);
        }
        if self.is_gemini() {
            return gemini::decode(&response, task, paid);
        }
        let errors = self.check_declarations(&response, paid)?;
        if task.is_some() && !errors.is_empty() {
            return Err("invalid structured tool declaration or arguments".into());
        }
        let mut turn = if self.is_chat() {
            chat::decode(raw, task)?
        } else {
            responses::decode(raw, task, cfg)?
        };
        for intent in turn
            .intents
            .as_array_mut()
            .ok_or("missing decoded tool batch")?
        {
            if let Some(error) = intent["call_id"].as_str().and_then(|id| errors.get(id)) {
                tool_rejection::mark(intent, *error);
            }
        }
        Ok(turn)
    }
    fn check_declarations(
        self,
        response: &Value,
        paid: &Value,
    ) -> Result<HashMap<String, Rejection>> {
        let mut errors = HashMap::new();
        let declared = paid["tools"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let calls = if self.is_chat() {
            &response["choices"][0]["message"]["tool_calls"]
        } else {
            &response["output"]
        };
        for item in calls.as_array().into_iter().flatten() {
            if !self.is_chat() && item["type"] == "web_search_call" {
                if paid["tool_choice"] == "none"
                    || !declared.iter().any(|d| d["type"] == "web_search")
                {
                    return Err("native tool absent from original paid request".into());
                }
                continue;
            }
            if !self.is_chat() && item["type"] != "function_call" {
                continue;
            }
            let call = if self.is_chat() {
                &item["function"]
            } else {
                item
            };
            let name = call["name"].as_str().ok_or("missing function name")?;
            let original = declared
                .iter()
                .filter(|d| d["type"] == "function")
                .map(|d| if self.is_chat() { &d["function"] } else { d })
                .find(|d| d["name"] == name);
            let choice = &paid["tool_choice"];
            let selected = choice["function"]["name"]
                .as_str()
                .or(choice["name"].as_str());
            let raw = call["arguments"]
                .as_str()
                .ok_or("missing original arguments")?;
            let parsed = serde_json::from_str::<Value>(raw);
            let args = tool_rejection::arguments(raw);
            let error = tool_rejection::validate(
                original.map(|d| &d["parameters"]),
                &args,
                choice != "none" && selected.is_none_or(|required| required == name),
            )?;
            let error =
                error.or_else(|| parsed.is_err().then_some(Rejection::InvalidToolArguments));
            if let Some(error) = error {
                let id = item[if self.is_chat() { "id" } else { "call_id" }]
                    .as_str()
                    .ok_or("missing original call ID")?;
                errors.insert(id.into(), error);
            }
        }
        Ok(errors)
    }

    pub fn complete(self, body: &Value) -> bool {
        if self.is_gemini() {
            return gemini::complete(body);
        }
        if self.is_chat() {
            matches!(
                body["choices"][0]["finish_reason"].as_str(),
                Some("stop" | "tool_calls")
            )
        } else {
            if self.is_messages() {
                return messages::complete(body);
            }
            responses::complete(body)
        }
    }
    pub fn usage(self, raw: &str) -> Value {
        if self.is_gemini() {
            return gemini::usage(raw);
        }
        if self.is_chat() {
            chat::usage(raw)
        } else {
            if self.is_messages() {
                return messages::usage(raw);
            }
            responses::usage(raw)
        }
    }
    fn items(self, body: &mut Value) -> Result<&mut Vec<Value>> {
        body[if self.is_chat() { "messages" } else { "input" }]
            .as_array_mut()
            .ok_or_else(|| "missing paid conversation items".into())
    }
    pub fn replay(self, body: &mut Value, original: &Value) -> Result<()> {
        if self.is_gemini() {
            return gemini::replay(body, original);
        }
        if self.is_messages() {
            return messages::replay(body, original);
        }
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
        if self.is_gemini() {
            return gemini::output(body, call_id, receipt);
        }
        if self.is_messages() {
            return messages::output(body, call_id, receipt);
        }
        let item = if self.is_chat() {
            json!({"role":"tool","tool_call_id":call_id,"content":receipt})
        } else {
            json!({"type":"function_call_output","call_id":call_id,"output":receipt})
        };
        self.items(body)?.push(item);
        Ok(())
    }
    pub fn user(self, body: &mut Value, text: &str) -> Result<()> {
        if self.is_gemini() {
            return gemini::user(body, text);
        }
        if self.is_messages() {
            return messages::user(body, text);
        }
        self.items(body)?
            .push(json!({"role":"user","content":text}));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_arguments_and_choices_keep_their_original_call_ids() {
        let mut paid = json!({"tools":[{"type":"function","function":{"name":"get","parameters":{"type":"object","properties":{"target":{"type":"string"}},"required":["target"]}}}]});
        let answer = json!({"choices":[{"message":{"tool_calls":[{"id":"bad","function":{"name":"get","arguments":"{\"target\":7}"}},{"id":"good","function":{"name":"get","arguments":"{\"target\":\"key\"}"}}]}}]});
        let rejected = Protocol::Chat.check_declarations(&answer, &paid).unwrap();
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected["bad"], Rejection::InvalidToolArguments);
        paid["tool_choice"] = json!({"type":"function","function":{"name":"put"}});
        let rejected = Protocol::Chat.check_declarations(&answer, &paid).unwrap();
        assert_eq!(rejected["good"], Rejection::ToolChoiceMismatch);
    }
    #[test]
    fn new_history_tool_cannot_enter_an_existing_paid_activation() {
        let paid = json!({"tools":[{"type":"function","function":{"name":"get","parameters":{"type":"object"}}}]});
        let answer = json!({"choices":[{"message":{"tool_calls":[{"id":"original","function":{"name":"history_query","arguments":"{}"}}]}}]});
        assert_eq!(
            Protocol::Chat.check_declarations(&answer, &paid).unwrap()["original"],
            Rejection::ToolNotDeclared
        );
    }
}
