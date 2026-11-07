use crate::{Result, tools::registry::Tool};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use yuki_plugin_sdk::{Binding, Declaration};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Install {
    #[serde(default = "initially_enabled", skip_serializing_if = "is_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<super::configuration::Definition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notifications: Option<super::notification::Policy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway_media: Option<super::gateway_media::Identity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_search: Option<super::external_search::Policy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<super::plugin_sessions::Policy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintenance: Option<super::maintenance::Policy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub participation: Option<super::participation::Policy>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub aux_routes: BTreeMap<String, super::aux_model::Route>,
    pub id: String,
    pub namespace: String,
    pub program: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub erasure_args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition: Option<super::composition::Runtime>,
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
fn initially_enabled() -> bool {
    true
}
fn is_enabled(value: &bool) -> bool {
    *value
}
static BASE_1: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
mod compiled_surface {
    include!(concat!(env!("OUT_DIR"), "/plugin_surface.rs"));
}

pub(crate) fn selected_build() -> bool {
    compiled_surface::SELECTED.is_some()
}

pub(crate) fn compiled_profile() -> &'static Value {
    static PROFILE: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    PROFILE.get_or_init(|| {
        serde_json::from_str(compiled_surface::SELECTED.unwrap_or(include_str!(
            "../../protocol/plugin/stable/1.0/profile.json"
        )))
        .expect("compiled plugin surface profile")
    })
}

pub(crate) fn profile_description() -> Value {
    let profile = compiled_profile();
    let mut methods = vec!["initialize"];
    methods.extend(
        profile["surface"]["methods"]
            .as_object()
            .expect("compiled plugin methods")
            .keys()
            .map(String::as_str),
    );
    methods.sort_unstable();
    json!({"version":profile["version"],"min_reader":profile["min_reader"],
        "candidate_source":profile["candidate_source"],"surface_sha256":profile["surface_sha256"],
        "handshake_sha256":profile["handshake_sha256"],"methods":methods,
        "activation_argument":"--plugin-base-1", "selected_by_default":selected_build()})
}

pub(crate) fn select_base(stable: bool) -> Result<()> {
    if *BASE_1.get_or_init(|| stable) != stable {
        return Err("plugin base already selected".into());
    }
    Ok(())
}

pub(crate) fn stable_base() -> bool {
    BASE_1.get().copied().unwrap_or(false)
}

pub fn methods() -> &'static Value {
    static IDL: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    IDL.get_or_init(|| {
        let mut base: Value = serde_json::from_str(include_str!(
            "../../protocol/plugin/gateway/host-preview.json"
        ))
        .expect("gateway IDL");
        for schema in [
            include_str!("../../protocol/plugin/control/methods.json"),
            include_str!("../../protocol/plugin/personal/methods.json"),
            include_str!("../../protocol/plugin/media/methods.json"),
            include_str!("../../protocol/plugin/composition/methods.json"),
            include_str!("../../protocol/plugin/aux-model/methods.json"),
            include_str!("../../protocol/plugin/participation/methods.json"),
            include_str!("../../protocol/plugin/maintenance/methods.json"),
            include_str!("../../protocol/plugin/sessions/methods.json"),
            include_str!("../../protocol/plugin/notifications/methods.json"),
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
        if stable_base() {
            base = compiled_profile()["surface"].clone();
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
        if self.grants.contains("gateway.activity") && !self.grants.contains("gateway.inbound") {
            return Err("gateway.activity requires its original inbound grant".into());
        }
        if self.grants.contains("gateway.recall")
            && !["gateway.inbound", "gateway.send"]
                .iter()
                .all(|g| self.grants.contains(*g))
        {
            return Err("gateway.recall requires the original inbound and send grants".into());
        }
        if self.grants.contains("search.external") != self.external_search.is_some() {
            return Err("search.external requires an explicit external search policy".into());
        }
        if let Some(policy) = &self.external_search {
            policy.validate(self)?;
        }
        if self.grants.contains("terminal.work")
            && !["tool.call", "forget.owner"]
                .iter()
                .all(|g| self.grants.contains(*g))
        {
            return Err("terminal.work requires tool.call and forget.owner".into());
        }
        if self.grants.contains("gateway.media") != self.gateway_media.is_some()
            || self
                .gateway_media
                .as_ref()
                .is_some_and(|i| !i.valid() || !self.grants.contains("gateway.inbound"))
        {
            return Err("gateway.media requires an inbound installation with an explicit application identity".into());
        }
        if let Some(policy) = &self.notifications {
            policy.validate(self)?;
        }
        if let Some(definition) = &self.configuration {
            definition.validate()?;
        }
        if let Some(policy) = &self.sessions {
            policy.validate(self)?;
        }
        if let Some(policy) = &self.maintenance {
            policy.validate(self)?;
        }
        if let Some(policy) = &self.participation {
            policy.validate(self)?;
        }
        for (name, route) in &self.aux_routes {
            if !identifier(name)
                || !["llm.complete", "llm.result", "forget.owner"]
                    .into_iter()
                    .all(|g| self.grants.contains(g))
            {
                return Err(
                    "auxiliary routes require explicit model and source-owner grants".into(),
                );
            }
            route.validate()?;
        }
        if !identifier(&self.id) || !identifier(&self.namespace) || !self.program.is_absolute() {
            return Err("invalid plugin identity or executable".into());
        }
        if self.erasure_args.is_some() && !self.grants.contains("forget.owner") {
            return Err("cleanup entry requires forget.owner".into());
        }
        if let Some(runtime) = &self.composition {
            runtime.validate(self)?;
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
        let mut identity = json!({"id":self.id,"namespace":self.namespace,"platform":self.platform,"bot_stage":self.bot_stage,"commands":self.commands,"tools":self.tools,"grants":self.grants});
        if let Some(policy) = &self.external_search {
            identity["external_search"] = serde_json::to_value(policy)?;
        }
        if let Some(media) = &self.gateway_media {
            identity["gateway_media"] = serde_json::to_value(media)?;
        }
        if let Some(policy) = &self.notifications {
            identity["notifications"] = serde_json::to_value(policy)?;
        }
        if let Some(definition) = &self.configuration {
            identity["configuration"] = serde_json::to_value(definition)?;
        }
        if let Some(policy) = &self.sessions {
            identity["sessions"] = serde_json::to_value(policy)?;
        }
        if let Some(policy) = &self.maintenance {
            identity["maintenance"] = serde_json::to_value(policy)?;
        }
        if let Some(policy) = &self.participation {
            identity["participation"] = serde_json::to_value(policy)?;
        }
        if !self.aux_routes.is_empty() {
            identity["aux_routes"] = serde_json::to_value(&self.aux_routes)?;
        }
        if let Some(runtime) = &self.composition {
            identity["composition"] = serde_json::to_value(runtime)?;
        }
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&identity)?)
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
            external_search: None,
            gateway_media: None,
            notifications: None,
            enabled: true,
            configuration: None,
            sessions: None,
            maintenance: None,
            participation: None,
            erasure_args: None,
            aux_routes: BTreeMap::new(),
            composition: None,
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
