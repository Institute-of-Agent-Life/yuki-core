//! Opt-in business methods from the unpublished host candidate. Initialization still uses Peer.
use crate::{Id, MAX_FRAME_BYTES};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
pub fn schema() -> &'static Value {
    static IDL: OnceLock<Value> = OnceLock::new();
    IDL.get_or_init(|| {
        serde_json::from_str(include_str!("host-preview.json")).expect("generated host projection")
    })
}
pub struct Calls {
    allowed: HashSet<String>,
    outbound: HashMap<Id, String>,
}
pub enum Incoming {
    Call {
        method: String,
        id: Id,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    Result {
        id: Id,
        method: String,
        result: Result<Value, Value>,
    },
}
impl Calls {
    pub fn negotiated(init: &Value) -> Result<Self, &'static str> {
        if init["host_schema_revision"] != 2 {
            return Err("host preview not negotiated");
        }
        let mut allowed = HashSet::new();
        for m in init["method_revisions"]
            .as_array()
            .ok_or("missing methods")?
        {
            let name = m["name"].as_str().ok_or("invalid method")?;
            if schema()["methods"]
                .get(name)
                .is_some_and(|d| d["revision"] == m["revision"])
            {
                allowed.insert(name.into());
            }
        }
        Ok(Self {
            allowed,
            outbound: HashMap::new(),
        })
    }
    pub fn request(&mut self, id: Id, method: &str, params: Value) -> Result<Value, &'static str> {
        if !self.allowed.contains(method)
            || schema()["methods"][method]["direction"] != "plugin_to_host"
            || self.outbound.len() >= 128
            || self.outbound.contains_key(&id)
        {
            return Err("unavailable method or request ID");
        }
        let frame = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        if frame.to_string().len() > MAX_FRAME_BYTES {
            return Err("frame too large");
        }
        self.outbound.insert(id, method.into());
        Ok(frame)
    }
    pub fn receive(&mut self, raw: &[u8]) -> Result<Incoming, &'static str> {
        if raw.len() > MAX_FRAME_BYTES {
            return Err("frame too large");
        }
        let msg: Value = serde_json::from_slice(raw).map_err(|_| "invalid frame")?;
        if msg["jsonrpc"] != "2.0" {
            return Err("invalid JSON-RPC");
        }
        if let Some(method) = msg["method"].as_str() {
            let direction = &schema()["methods"][method]["direction"];
            if !self.allowed.contains(method)
                || msg.get("result").is_some()
                || msg.get("error").is_some()
            {
                return Err("undeclared/mixed request");
            }
            if direction == "host_notification" && msg.get("id").is_none() {
                return Ok(Incoming::Notification {
                    method: method.into(),
                    params: msg["params"].clone(),
                });
            }
            if direction != "host_to_plugin" {
                return Err("wrong call direction");
            }
            return Ok(Incoming::Call {
                method: method.into(),
                id: serde_json::from_value(msg["id"].clone()).map_err(|_| "invalid id")?,
                params: msg["params"].clone(),
            });
        }
        let id: Id = serde_json::from_value(msg["id"].clone()).map_err(|_| "invalid id")?;
        let result = match (msg.get("result"), msg.get("error")) {
            (Some(v), None) => Ok(v.clone()),
            (None, Some(e)) => Err(e.clone()),
            _ => return Err("invalid response"),
        };
        let method = self.outbound.remove(&id).ok_or("unmatched response")?;
        Ok(Incoming::Result { id, method, result })
    }
}
