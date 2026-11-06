use crate::{Result, runtime::main::Config};
use serde_json::{Value, json};

pub(crate) fn configure(cfg: &Config, body: &mut Value) -> Result<()> {
    let Some(binding) = &cfg.profile_binding else {
        return Ok(());
    };
    let profile = &binding.profile;
    let options = &profile["wire_options"];
    if cfg.protocol.is_gemini() {
        let generation = &mut body["generationConfig"];
        if profile["thinking_enabled"] == true && options["reasoning"] == "budget" {
            generation["thinkingConfig"] =
                json!({"thinkingBudget":options["thinking_budget_tokens"]});
        }
        if options["send_temperature"] == true {
            generation["temperature"] = profile["default_temperature"].clone();
        }
        return Ok(());
    }
    if !cfg.protocol.is_chat() {
        return Ok(());
    }
    let object = body.as_object_mut().ok_or("validation_error")?;
    if options["token_field"] == "max_completion_tokens"
        && let Some(limit) = object.remove("max_tokens")
    {
        object.insert("max_completion_tokens".into(), limit);
    }
    if options["send_temperature"] == true {
        object.insert("temperature".into(), profile["default_temperature"].clone());
    }
    if options["send_tool_choice"] == false {
        object.remove("tool_choice");
    }
    let effort = profile["reasoning_effort"].as_str();
    if profile["thinking_enabled"] == true {
        match options["reasoning"].as_str() {
            Some("effort") => {
                if let Some(effort) = effort {
                    body["reasoning_effort"] = json!(effort);
                }
            }
            Some("thinking") => {
                body["thinking"] = json!({"type":"enabled"});
                if options["send_reasoning_effort"] == true {
                    if let Some(effort) = effort {
                        body["reasoning_effort"] = json!(effort);
                    }
                } else if !matches!(effort, None | Some("none" | "minimal" | "low")) {
                    return Err("unsupported_profile_feature".into());
                }
            }
            Some("enable_thinking") => {
                body["enable_thinking"] = json!(true);
                body["thinking_budget"] = options["thinking_budget_tokens"].clone();
            }
            Some("openrouter") => {
                body["reasoning"] = json!({"enabled":true,"exclude":false});
                if let Some(effort) = effort {
                    body["reasoning"]["effort"] = json!(effort);
                }
            }
            Some("builtin") if matches!(effort, None | Some("none" | "minimal" | "low")) => {}
            _ => return Err("unsupported_profile_feature".into()),
        }
    }
    if options["reasoning_split"] == true {
        body["reasoning_split"] = json!(true);
    }
    if !options["reasoning_format"].is_null() {
        body["reasoning_format"] = options["reasoning_format"].clone();
    } else if !options["include_reasoning"].is_null() {
        body["include_reasoning"] = options["include_reasoning"].clone();
    }
    Ok(())
}

pub(crate) fn replay(cfg: &Config, response: &Value) -> Value {
    let mut response = response.clone();
    if cfg.protocol.is_chat()
        && cfg
            .profile_binding
            .as_ref()
            .is_some_and(|binding| binding.profile["wire_options"]["replay_reasoning"] == false)
        && let Some(message) = response["choices"][0]["message"].as_object_mut()
    {
        message.remove("reasoning_content");
        message.remove("reasoning_details");
    }
    response
}
