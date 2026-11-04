//! Line-framed JSON-RPC for the Yuki handshake preview. Each Peer belongs to one connection.
pub mod host;
#[rustfmt::skip]
mod generated;
pub use generated::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap},
    io::{self, BufRead},
};

pub trait Validate {
    fn validate(&self) -> bool;
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Id {
    Number(i64),
    Text(String),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub namespace: String,
    pub grants: Vec<String>,
    pub manifest_revision: String,
    pub declarations: Vec<Declaration>,
}

fn unique(declarations: &[Declaration]) -> Result<(), i64> {
    let names: BTreeSet<_> = declarations.iter().map(|d| &d.name).collect();
    if names.len() == declarations.len() {
        Ok(())
    } else {
        Err(DUPLICATE_DECLARATION)
    }
}
fn ordered(mut declarations: Vec<Declaration>) -> Vec<Declaration> {
    declarations.sort_by(|a, b| a.name.cmp(&b.name));
    declarations
}
fn version(v: &Version) -> (u16, u16) {
    (v.major, v.minor)
}

impl Binding {
    pub fn offer(&self) -> Initialize {
        let low = VERSIONS.iter().min().unwrap();
        let high = VERSIONS.iter().max().unwrap();
        Initialize {
            versions: VersionRange {
                min: Version {
                    major: low.0,
                    minor: low.1,
                },
                max: Version {
                    major: high.0,
                    minor: high.1,
                },
            },
            required_features: FEATURES.iter().map(|s| s.to_string()).collect(),
            optional_features: vec![],
            manifest_revision: self.manifest_revision.clone(),
            declarations: self.declarations.clone(),
        }
    }
    fn negotiate(&self, params: &Initialize) -> Result<Initialized, i64> {
        if !params.validate() {
            return Err(INVALID_PARAMS);
        }
        unique(&params.declarations)?;
        if self.manifest_revision != params.manifest_revision
            || ordered(params.declarations.clone()) != ordered(self.declarations.clone())
        {
            return Err(MANIFEST_MISMATCH);
        }
        let min = version(&params.versions.min);
        let max = version(&params.versions.max);
        let &(major, minor) = VERSIONS
            .iter()
            .filter(|v| min <= **v && **v <= max)
            .max()
            .ok_or(UNSUPPORTED_VERSION)?;
        if params
            .required_features
            .iter()
            .any(|f| !FEATURES.contains(&f.as_str()))
        {
            return Err(MISSING_FEATURE);
        }
        let selected: BTreeSet<_> = params
            .required_features
            .iter()
            .chain(&params.optional_features)
            .filter(|f| FEATURES.contains(&f.as_str()))
            .cloned()
            .collect();
        Ok(Initialized {
            version: Version { major, minor },
            features: selected.into_iter().collect(),
            extension_points: EXTENSIONS.iter().map(|s| s.to_string()).collect(),
            method_revisions: METHODS
                .iter()
                .map(|(s, r)| MethodRevision {
                    name: s.to_string(),
                    revision: *r,
                })
                .collect(),
            schema_revision: SCHEMA_REVISION,
            namespace: self.namespace.clone(),
            grants: self.grants.clone(),
            manifest_revision: self.manifest_revision.clone(),
        })
    }
}

pub enum Received {
    Reply(Value),
    Response {
        id: Id,
        result: Result<Initialized, Value>,
    },
    Notification,
}

pub struct Peer {
    binding: Binding,
    negotiated: Option<Initialized>,
    pending: HashMap<Id, Initialize>,
}

fn error(id: Value, code: i64, message: &str) -> Received {
    Received::Reply(json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}))
}
fn problem(code: i64) -> &'static str {
    match code {
        UNSUPPORTED_VERSION => "unsupported_version",
        MISSING_FEATURE => "missing_feature",
        MANIFEST_MISMATCH => "manifest_mismatch",
        DUPLICATE_DECLARATION => "duplicate_declaration",
        _ => "invalid_params",
    }
}

impl Peer {
    pub fn new(binding: Binding) -> Result<Self, &'static str> {
        if binding.namespace.trim().is_empty()
            || binding.namespace.chars().count() > 256
            || binding.grants.len() > 128
            || binding
                .grants
                .iter()
                .any(|s| !(1..=256).contains(&s.chars().count()))
            || binding.manifest_revision.trim().is_empty()
            || unique(&binding.declarations).is_err()
            || !binding.offer().validate()
        {
            return Err("invalid trusted binding");
        }
        Ok(Self {
            binding,
            negotiated: None,
            pending: HashMap::new(),
        })
    }

    pub fn request(&mut self, id: Id, params: Initialize) -> Result<Value, &'static str> {
        if !params.validate() || self.pending.contains_key(&id) || self.pending.len() >= 128 {
            return Err("invalid or duplicate pending request");
        }
        let value = json!({"jsonrpc":"2.0","id":id,"method":METHODS[0].0,"params":params});
        if serde_json::to_vec(&value)
            .map_err(|_| "invalid frame")?
            .len()
            > MAX_FRAME_BYTES
        {
            return Err("frame too large");
        }
        self.pending.insert(id, params);
        Ok(value)
    }

    pub fn finish(&self) -> Result<(), &'static str> {
        if self.pending.is_empty() {
            Ok(())
        } else {
            Err("EOF with pending requests")
        }
    }

    pub fn receive(&mut self, raw: &[u8]) -> Result<Received, &'static str> {
        if raw.len() > MAX_FRAME_BYTES {
            return Err("frame too large");
        }
        let Ok(message) = serde_json::from_slice::<Value>(raw) else {
            return Ok(error(Value::Null, PARSE_ERROR, "parse_error"));
        };
        if message["jsonrpc"] != "2.0" || !message.is_object() {
            return Ok(error(Value::Null, INVALID_REQUEST, "invalid_request"));
        }
        if let Some(method) = message.get("method") {
            let Some(method) = method.as_str() else {
                return Ok(error(Value::Null, INVALID_REQUEST, "invalid_request"));
            };
            if !message.as_object().unwrap().contains_key("id") {
                return Ok(Received::Notification);
            }
            let Ok(id) = serde_json::from_value::<Id>(message["id"].clone()) else {
                return Ok(error(Value::Null, INVALID_REQUEST, "invalid_id"));
            };
            let id = json!(id);
            if message.get("result").is_some() || message.get("error").is_some() {
                return Ok(error(id, INVALID_REQUEST, "mixed request and response"));
            }
            if method != METHODS[0].0 {
                return Ok(error(id, METHOD_NOT_FOUND, "method_not_found"));
            }
            let Ok(params) = serde_json::from_value::<Initialize>(message["params"].clone()) else {
                return Ok(error(id, INVALID_PARAMS, "invalid_params"));
            };
            let result = match self.binding.negotiate(&params) {
                Ok(result) => result,
                Err(code) => return Ok(error(id, code, problem(code))),
            };
            if self.negotiated.as_ref().is_some_and(|old| old != &result) {
                return Ok(error(id, MANIFEST_MISMATCH, "handshake_changed"));
            }
            self.negotiated = Some(result.clone());
            return Ok(Received::Reply(
                json!({"jsonrpc":"2.0","id":id,"result":result}),
            ));
        }
        let id: Id =
            serde_json::from_value(message["id"].clone()).map_err(|_| "invalid response ID")?;
        let offer = self.pending.get(&id).ok_or("unknown response ID")?;
        let result = match (message.get("result"), message.get("error")) {
            (Some(body), None) => {
                let response: Initialized = serde_json::from_value(body.clone())
                    .map_err(|_| "invalid initialize result")?;
                if !response.validate()
                    || version(&response.version) < version(&offer.versions.min)
                    || version(&response.version) > version(&offer.versions.max)
                    || !offer
                        .required_features
                        .iter()
                        .all(|feature| response.features.contains(feature))
                    || !response.features.iter().all(|f| {
                        offer.required_features.contains(f) || offer.optional_features.contains(f)
                    })
                    || !METHODS.iter().all(|(name, rev)| {
                        response
                            .method_revisions
                            .iter()
                            .any(|m| m.name == *name && m.revision == *rev)
                    })
                    || response.schema_revision != SCHEMA_REVISION
                    || response.manifest_revision != offer.manifest_revision
                {
                    return Err("result outside offered contract");
                }
                Ok(response)
            }
            (None, Some(err)) if err["code"].is_i64() && err["message"].is_string() => {
                Err(err.clone())
            }
            _ => return Err("malformed response"),
        };
        self.pending.remove(&id);
        Ok(Received::Response { id, result })
    }
}

pub fn read_frame(input: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut buffer = Vec::new();
    loop {
        let bytes = input.fill_buf()?;
        if bytes.is_empty() {
            return if buffer.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::other("unterminated frame"))
            };
        }
        let newline = bytes.iter().position(|b| *b == b'\n');
        let size = newline.unwrap_or(bytes.len());
        if buffer.len() + size > MAX_FRAME_BYTES {
            return Err(io::Error::other("frame too large"));
        }
        buffer.extend_from_slice(&bytes[..size]);
        input.consume(size + usize::from(newline.is_some()));
        if newline.is_some() {
            return Ok(Some(buffer));
        }
    }
}
