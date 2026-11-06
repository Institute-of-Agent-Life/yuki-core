use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Copy, Default, clap::ValueEnum, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    #[default]
    Hybrid,
    LexicalOnly,
}
impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hybrid => "hybrid",
            Self::LexicalOnly => "lexical-only",
        }
    }
}
#[derive(Default, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub mode: Mode,
    pub embedding: Option<Profile>,
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        match (&self.mode,&self.embedding) {
            (Mode::Hybrid,Some(p))=>p.validate(),
            (Mode::Hybrid,None)=>Err("hybrid memory requires [memory.embedding]; configure it or explicitly select lexical-only".into()),
            (Mode::LexicalOnly, None)=>Ok(()),
            (Mode::LexicalOnly,Some(_))=>Err("lexical-only memory cannot enable an embedding route".into()),
        }
    }
}
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Dialect {
    OpenaiCompatible,
    QwenDashscope,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub provider: Dialect,
    pub model: String,
    pub base_url: String,
    pub dimensions: usize,
    pub key_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_hash: Option<String>,
    #[serde(skip)]
    pub credential: Option<std::sync::Arc<String>>,
    #[serde(default = "batch")]
    pub batch_size: usize,
    #[serde(default = "limit")]
    pub max_characters: usize,
    #[serde(default = "instruct")]
    pub query_instruction: String,
}
fn batch() -> usize {
    16
}
fn limit() -> usize {
    2048
}
fn instruct() -> String {
    "Retrieve personal memory facts relevant to the conversational query.".into()
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
impl Profile {
    fn validate(&self) -> Result<()> {
        let url = reqwest::Url::parse(&self.base_url)?;
        let loopback = url
            .host_str()
            .and_then(|s| s.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || self.model.trim().is_empty()
            || !(1..=8192).contains(&self.dimensions)
            || !(1..=100).contains(&self.batch_size)
            || !(16..=8192).contains(&self.max_characters)
            || (self.provider == Dialect::QwenDashscope
                && (self.key_env.is_none()
                    || self.dimensions != 1024
                    || self.batch_size > 20
                    || self.query_instruction.trim().is_empty()))
        {
            return Err("invalid embedding route, model, dimensions or batch limits".into());
        }
        if let Some(key) = &self.key_env
            && (key.is_empty()
                || self
                    .credential
                    .as_ref()
                    .map(|v| v.as_str())
                    .filter(|v| !v.is_empty())
                    .is_none()
                    && std::env::var(key).ok().is_none_or(|value| value.is_empty()))
        {
            return Err("configured embedding credential unavailable".into());
        }
        Ok(())
    }
    pub fn identity(&self) -> String {
        let mut values = json!([
            self.provider,
            self.model,
            self.base_url.trim_end_matches('/'),
            self.dimensions,
            "dense",
            1,
            self.max_characters,
            self.query_instruction,
            self.key_env
        ]);
        if let Some(hash) = &self.credential_hash {
            values
                .as_array_mut()
                .expect("identity array")
                .push(json!(hash));
        }
        digest(values.to_string().as_bytes())
    }
    pub fn endpoint(&self) -> String {
        format!(
            "{}{}",
            self.base_url.trim_end_matches('/'),
            match self.provider {
                Dialect::OpenaiCompatible => "/embeddings",
                Dialect::QwenDashscope => "/services/embeddings/text-embedding/text-embedding",
            }
        )
    }
    pub fn wire(&self, texts: &[String], query: bool) -> Value {
        match self.provider {
            Dialect::OpenaiCompatible => {
                json!({"model":self.model,"input":texts,"dimensions":self.dimensions,"encoding_format":"float"})
            }
            Dialect::QwenDashscope => {
                let mut parameters = json!({"text_type":if query{"query"}else{"document"},"dimension":self.dimensions,"output_type":"dense"});
                if query {
                    parameters["instruct"] = json!(self.query_instruction);
                }
                json!({"model":self.model,"input":{"texts":texts},"parameters":parameters})
            }
        }
    }
    pub fn rows(&self, raw: &Value, n: usize) -> Vec<Option<Vec<f32>>> {
        let (rows, index) = match self.provider {
            Dialect::OpenaiCompatible => (&raw["data"], "index"),
            Dialect::QwenDashscope => (&raw["output"]["embeddings"], "text_index"),
        };
        let mut out = vec![None; n];
        let mut seen = vec![false; n];
        for row in rows.as_array().into_iter().flatten() {
            let Some(i) = row[index]
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .filter(|i| *i < n)
            else {
                return vec![None; n];
            };
            if seen[i] {
                out[i] = None;
                continue;
            }
            seen[i] = true;
            if let Ok(values) = serde_json::from_value::<Vec<f32>>(row["embedding"].clone())
                && values.len() == self.dimensions
                && values.iter().all(|v| v.is_finite())
                && values.iter().any(|v| *v != 0.0)
            {
                out[i] = Some(values);
            }
        }
        out
    }
}
static DIGITS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\d+").unwrap());
pub fn bounded(text: &str, max: usize) -> String {
    let clean: String = text
        .nfkc()
        .map(|c| {
            if matches!(c,'\0'..='\u{8}'|'\u{b}'|'\u{c}'|'\u{e}'..='\u{1f}'|'\u{7f}') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let anon = DIGITS.replace_all(&clean, |c: &regex::Captures<'_>| {
        if (5..=20).contains(&c[0].chars().count()) {
            "[id]".to_string()
        } else {
            c[0].to_string()
        }
    });
    anon.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max)
        .collect()
}
pub fn document(kind: &str, category: &str, key: &str, body: &str, max: usize) -> String {
    format!(
        "Kind: {}\nCategory: {}\nKey: {}\nFact: {}",
        bounded(kind, max),
        bounded(category, max),
        bounded(key, max),
        bounded(body, max)
    )
    .chars()
    .take(max)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_the_original_v4_document_and_four_physical_request_bodies() {
        let v: Value =
            serde_json::from_str(include_str!("../../../fixtures/embedding/v4-wire.json")).unwrap();
        let f = &v["fields"];
        let text = document(
            f["kind"].as_str().unwrap(),
            f["category"].as_str().unwrap(),
            f["memory_key"].as_str().unwrap(),
            f["content"].as_str().unwrap(),
            2048,
        );
        assert_eq!(text, v["projection"]);
        assert_eq!(digest(text.as_bytes()), v["sha256"]);
        for (i, wire) in v["wires"].as_array().unwrap().iter().enumerate() {
            let p:Profile=serde_json::from_value(json!({"base_url":"http://127.0.0.1/v1","provider":if i<2 {"openai_compatible"}else{"qwen_dashscope"},"model":if i<2 {"fixture"}else{"fixture-q"},"dimensions":if i<2{2}else{1024}})).unwrap();
            assert!(p.endpoint().ends_with(wire["path"].as_str().unwrap()));
            assert_eq!(
                p.wire(
                    &[if i % 2 == 1 {
                        "茶".into()
                    } else {
                        text.clone()
                    }],
                    i % 2 == 1
                ),
                wire["body"]
            );
        }
    }
}
