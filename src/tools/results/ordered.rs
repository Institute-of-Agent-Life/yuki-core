//! Artifact-local JSON preserves casefold ties without changing global contract hashing.
use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, SeqAccess, Visitor},
};
use serde_json::{Value, json};
use std::fmt;

#[derive(Clone)]
pub(super) enum Node {
    Scalar(Value),
    Array(Vec<Node>),
    Object(Vec<(String, Node)>),
}
impl Node {
    pub fn value(&self) -> Value {
        match self {
            Self::Scalar(v) => v.clone(),
            Self::Array(a) => Value::Array(a.iter().map(Self::value).collect()),
            Self::Object(o) => {
                Value::Object(o.iter().map(|(k, v)| (k.clone(), v.value())).collect())
            }
        }
    }
    // Count without copying a large subtree, stopping as soon as the page cannot fit.
    pub fn size(&self, limit: usize) -> Option<usize> {
        fn string_size(s: &str, limit: usize) -> Option<usize> {
            let mut used = 2;
            for c in s.chars() {
                used += match c {
                    '\"' | '\\' | '\x08' | '\t' | '\n' | '\x0c' | '\r' => 2,
                    '\0'..='\x1f' => 6,
                    _ => 1,
                };
                if used > limit {
                    return None;
                }
            }
            (used <= limit).then_some(used)
        }
        let mut used = 2;
        match self {
            Self::Scalar(Value::String(s)) => return string_size(s, limit),
            Self::Scalar(v) => {
                let n = v.to_string().chars().count();
                return (n <= limit).then_some(n);
            }
            Self::Array(a) => {
                for (index, v) in a.iter().enumerate() {
                    used += if index > 0 { 2 } else { 0 };
                    used += v.size(limit.saturating_sub(used))?;
                    if used > limit {
                        return None;
                    }
                }
            }
            Self::Object(o) => {
                for (index, (key, v)) in o.iter().enumerate() {
                    used += if index > 0 { 4 } else { 2 };
                    used += string_size(key, limit.saturating_sub(used))?;
                    used += v.size(limit.saturating_sub(used))?;
                    if used > limit {
                        return None;
                    }
                }
            }
        }
        (used <= limit).then_some(used)
    }
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(o) => o.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Array(_) => "array",
            Self::Object(_) => "object",
            Self::Scalar(Value::Null) => "null",
            Self::Scalar(Value::Bool(_)) => "boolean",
            Self::Scalar(Value::String(_)) => "string",
            Self::Scalar(_) => "number",
        }
    }
    pub fn shape(&self) -> Value {
        let mut out = json!({"type":self.kind()});
        match self {
            Self::Array(a) => out["length"] = json!(a.len()),
            Self::Object(o) => out["child_count"] = json!(o.len()),
            Self::Scalar(Value::String(s)) => out["characters"] = json!(s.chars().count()),
            _ => {}
        }
        out
    }
    pub fn scalar_text(&self) -> String {
        match self {
            Self::Scalar(Value::String(s)) => s.clone(),
            _ => self.value().to_string(),
        }
    }
    pub fn keys(&self) -> Vec<(&str, &Self)> {
        let Self::Object(o) = self else {
            return vec![];
        };
        let mut out: Vec<_> = o.iter().map(|(k, v)| (k.as_str(), v)).collect();
        out.sort_by_cached_key(|(k, _)| super::legacy::fold(k));
        out
    }
}
impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Read;
        impl<'de> Visitor<'de> for Read {
            type Value = Node;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON artifact")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Node, E> {
                Ok(Node::Scalar(Value::Null))
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Node, E> {
                Ok(Node::Scalar(json!(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Node, E> {
                Ok(Node::Scalar(json!(v)))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Node, E> {
                Ok(Node::Scalar(json!(v)))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Node, E> {
                Ok(Node::Scalar(json!(v)))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Node, E> {
                Ok(Node::Scalar(json!(v)))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Node, A::Error> {
                let mut out = vec![];
                while let Some(v) = a.next_element()? {
                    out.push(v);
                }
                Ok(Node::Array(out))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Node, A::Error> {
                let mut out: Vec<(String, Node)> = vec![];
                let mut positions = std::collections::HashMap::<String, usize>::new();
                while let Some((k, v)) = a.next_entry::<String, Node>()? {
                    if let Some(index) = positions.get(&k) {
                        out[*index].1 = v;
                    } else {
                        positions.insert(k.clone(), out.len());
                        out.push((k, v));
                    }
                }
                Ok(Node::Object(out))
            }
        }
        d.deserialize_any(Read)
    }
}
