//! Independent consumer built against the candidate .crate. It handles one reverse call during a paid tool call.
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{self, Write},
};
use yuki_plugin_sdk::{
    Binding, Id, Peer, Received,
    host::{Calls, Incoming},
    read_frame,
};
fn send(out: &mut impl Write, frame: Value) -> Result<(), Box<dyn std::error::Error>> {
    writeln!(out, "{frame}")?;
    out.flush()?;
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let binding: Binding = serde_json::from_str(&std::env::var("YUKI_PLUGIN_BINDING")?)?;
    let offer = binding.offer();
    let mut handshake = Peer::new(binding)?;
    let mut stdout = io::stdout().lock();
    let mut stdin = io::stdin().lock();
    send(&mut stdout, handshake.request(Id::Number(1), offer)?)?;
    let raw = read_frame(&mut stdin)?.ok_or("no handshake")?;
    if !matches!(
        handshake.receive(&raw)?,
        Received::Response { result: Ok(_), .. }
    ) {
        return Err("handshake rejected".into());
    }
    let reply: Value = serde_json::from_slice(&raw)?;
    let mut calls = Calls::negotiated(&reply["result"])?;
    let mut waiting = HashMap::new();
    while let Some(raw) = read_frame(&mut stdin)? {
        match calls.receive(&raw)? {
            Incoming::Call { method, id, params } if method == "tool.call" => {
                let ask = calls.request(
                    id.clone(),
                    "tool.status",
                    json!({"handle":params["handle"]}),
                )?;
                waiting.insert(id, params["arguments"].clone());
                send(&mut stdout, ask)?;
            }
            Incoming::Result {
                id,
                result: Ok(status),
                ..
            } => {
                let original = waiting.remove(&id).ok_or("unexpected original ID")?;
                send(
                    &mut stdout,
                    json!({"jsonrpc":"2.0","id":id,"result":{"ok":true,"data":{"rust_sdk":true,"status":status,"original":original}}}),
                )?;
            }
            _ => return Err("unexpected candidate method".into()),
        }
    }
    Ok(())
}
