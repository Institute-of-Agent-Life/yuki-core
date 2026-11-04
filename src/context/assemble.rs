use crate::{
    model::chat::{self, Input},
    runtime::main::Config,
};
use serde_json::{Value, json};

/// Assemble once, before reservation. Runtime data has its own message and never impersonates the trigger.
pub fn body(
    profile: &Config,
    soul: &str,
    input: &Input,
    history: Vec<Value>,
    declarations: Vec<Value>,
) -> Value {
    let name = &profile.name;
    // The original compiler emits the character contribution before the direct-mode program.
    let mut prefix = format!("【角色身份】\n当前角色的名称是 {name}。");
    if !soul.trim().is_empty() {
        prefix.push_str("\n\n【SOUL】\n");
        prefix.push_str(&soul.replace("{{name}}", name));
    }
    prefix.push_str("\n\n");
    prefix.push_str(include_str!("main-program.txt"));
    let mut messages = vec![json!({"role":"system","content":prefix})];
    messages.extend(history);
    if !input.context.is_empty() {
        let state = match input.context.as_slice() {
            [item] if item.is_object() => item.clone(),
            items => json!({"materials":items}),
        };
        messages.push(json!({"role":"user","content":format!("\n\n<runtime_context>\n{state}\n</runtime_context>\n\n")}));
    }
    messages.push(json!({"role":"user","content":input.text}));
    let mut body = json!({"model":profile.model,"messages":messages,"max_tokens":profile.max_output,"stream":false});
    chat::configure(&mut body, input.structured.as_ref(), declarations);
    body
}
