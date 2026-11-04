use crate::{Result, tools::registry::Tool};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::PathBuf};
use yuki_plugin_sdk::{Binding, Declaration};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Install {
    pub id: String,
    pub namespace: String,
    pub program: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub bot_stage: Option<String>,
    #[serde(default)]
    pub grants: BTreeSet<String>,
    #[serde(default)]
    pub commands: BTreeSet<String>,
    #[serde(default)]
    pub tools: Vec<Tool>,
}
pub fn methods() -> &'static Value {
    static IDL: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    IDL.get_or_init(|| {
        let mut base: Value = serde_json::from_str(include_str!(
            "../../protocol/plugin/gateway/host-preview.json"
        ))
        .expect("gateway IDL");
        for schema in [
            include_str!("../../protocol/plugin/personal/methods.json"),
            include_str!("../../protocol/plugin/media/methods.json"),
        ] {
            let group: Value = serde_json::from_str(schema).expect("host extension IDL");
            for (name, method) in group["methods"].as_object().expect("extension methods") {
                assert!(
                    base["methods"]
                        .as_object_mut()
                        .unwrap()
                        .insert(name.clone(), method.clone())
                        .is_none()
                );
            }
        }
        base
    })
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && !matches!(s, "." | "..")
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}
impl Install {
    pub fn validate(&self) -> Result<()> {
        if !identifier(&self.id) || !identifier(&self.namespace) || !self.program.is_absolute() {
            return Err("invalid plugin identity or executable".into());
        }
        let known = methods()["methods"].as_object().unwrap();
        if self
            .grants
            .iter()
            .any(|g| !known.values().any(|m| m["grant"] == *g))
        {
            return Err("unknown host grant".into());
        }
        if self.grants.iter().any(|g| g.starts_with("media."))
            && !self.grants.contains("forget.owner")
        {
            return Err("media readers and publishers require source erasure support".into());
        }
        if (!self.commands.is_empty() && !self.grants.contains("command.invoke"))
            || (!self.tools.is_empty() && !self.grants.contains("tool.call"))
        {
            return Err("installed declarations require an explicit grant".into());
        }
        let gateway =
            self.grants.contains("gateway.inbound") || self.grants.contains("gateway.send");
        if gateway
            && (self
                .platform
                .as_deref()
                .is_none_or(|p| !identifier(p) || p.eq_ignore_ascii_case("qq"))
                || self.bot_stage.as_deref().is_none_or(str::is_empty))
        {
            return Err("external gateway requires its own non-QQ platform and SELF stage".into());
        }
        if !gateway && (self.platform.is_some() || self.bot_stage.is_some()) {
            return Err("gateway identity requires a gateway grant".into());
        }
        if self.commands.iter().any(|p| {
            !p.starts_with('/')
                || p.len() < 2
                || p.contains(char::is_whitespace)
                || p.split('/').skip(1).any(|x| !identifier(x))
        }) {
            return Err("invalid bound command path".into());
        }
        if self.env.iter().any(|key| {
            key.is_empty()
                || !key
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        }) {
            return Err("invalid environment allowlist".into());
        }
        for t in &self.tools {
            if !identifier(&t.name)
                || crate::tools::registry::builtin(&t.name).is_some()
                || t.provider.is_some()
                || t.cacheable
                || t.parallel_read
            {
                return Err(
                    "plugin tool name collides or has an unverified execution policy".into(),
                );
            }
            jsonschema::validator_for(&t.schema)?;
        }
        Ok(())
    }
    pub fn revision(&self) -> Result<String> {
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(
                &json!({"id":self.id,"namespace":self.namespace,"platform":self.platform,"bot_stage":self.bot_stage,"commands":self.commands,"tools":self.tools,"grants":self.grants})
            )?)
        ))
    }
    pub fn binding(&self) -> Result<Binding> {
        let declarations = self
            .commands
            .iter()
            .map(|p| Declaration {
                kind: "command".into(),
                name: p.clone(),
                revision: 1,
            })
            .chain(self.tools.iter().map(|t| Declaration {
                kind: "tool".into(),
                name: t.name.clone(),
                revision: 1,
            }))
            .collect();
        Ok(Binding {
            namespace: self.namespace.clone(),
            grants: self.grants.iter().cloned().collect(),
            manifest_revision: self.revision()?,
            declarations,
        })
    }
}

pub fn validate(installs: &[Install]) -> Result<()> {
    let (mut ids, mut namespaces, mut paths, mut names) = (
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
        BTreeSet::new(),
    );
    for i in installs {
        i.validate()?;
        if !ids.insert(&i.id)
            || !namespaces.insert(&i.namespace)
            || i.commands.iter().any(|p| !paths.insert(p))
            || i.tools.iter().any(|t| !names.insert(&t.name))
        {
            return Err("ambiguous installed plugin, namespace, command or tool".into());
        }
    }
    Ok(())
}
pub fn schema(method: &str, direction: &str, params: &Value) -> Result<&'static Value> {
    let def = methods()["methods"]
        .get(method)
        .filter(|d| d["direction"] == direction)
        .ok_or("method unavailable in this direction")?;
    if !jsonschema::validator_for(&def["params"])?.is_valid(params) {
        return Err("invalid host method parameters".into());
    }
    Ok(def)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn neither_sender_nor_observer_can_claim_the_native_qq_namespace() {
        let mut i = Install {
            id: "p".into(),
            namespace: "qq.native".into(),
            program: std::env::current_exe().unwrap(),
            args: vec![],
            env: vec![],
            platform: Some("qq".into()),
            bot_stage: Some("native-self".into()),
            grants: ["gateway.send".into()].into(),
            commands: Default::default(),
            tools: vec![],
        };
        assert!(i.validate().is_err());
        i.grants = ["observe.metadata".into()].into();
        assert!(i.validate().is_err());
    }
}
