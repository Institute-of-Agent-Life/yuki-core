pub(in crate::host) mod scope;
mod transport;
mod writer;

use crate::{Result, runtime::main::Config, store::Store};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub profile: Config,
    pub purposes: BTreeSet<String>,
    pub origins: BTreeSet<String>,
    pub entries: BTreeSet<String>,
    #[serde(default)]
    pub media: BTreeSet<String>,
}

impl Route {
    pub(super) fn validate(&self) -> Result<()> {
        self.profile.validate()?;
        if self.purposes.is_empty()
            || self.purposes.iter().any(|p| p.is_empty() || p.len() > 128)
            || self.origins.is_empty()
            || self
                .origins
                .iter()
                .any(|o| !matches!(o.as_str(), "event" | "call" | "maintenance" | "session"))
            || (self.entries.is_empty() && self.origins.iter().any(|origin| origin != "session"))
            || self.entries.iter().any(String::is_empty)
            || self
                .media
                .iter()
                .any(|m| !matches!(m.as_str(), "image" | "audio"))
            || self.media.contains("image") && !self.profile.vision_input
            || self.media.contains("audio")
                && !(self.profile.protocol.is_chat() || self.profile.protocol.is_gemini())
            || self.profile.native_web_search
            || self.profile.binding_endpoint.is_some()
            || self.profile.tool_choice.is_some()
        {
            return Err("auxiliary route requires explicit purpose, entry scope and bounded model capabilities".into());
        }
        Ok(())
    }
}

pub enum Change {
    Request {
        plugin: String,
        generation: String,
        params: Value,
    },
    Dispatch {
        plugin: String,
        generation: String,
        request: String,
    },
    Result {
        plugin: String,
        generation: String,
        params: Value,
        page: bool,
    },
}

pub(super) use writer::{apply, configure};
pub(super) use writer::{structured_receipt, visible_receipt};

pub(super) async fn callback(
    store: &Store,
    plugin: &str,
    generation: &str,
    method: &str,
    params: Value,
) -> Result<Value> {
    super::grants::schema(method, "plugin_to_host", &params)?;
    if method == "llm.result" {
        return write(
            store,
            Change::Result {
                plugin: plugin.into(),
                generation: generation.into(),
                params,
                page: true,
            },
        )
        .await;
    }
    let admitted = write(
        store,
        Change::Request {
            plugin: plugin.into(),
            generation: generation.into(),
            params,
        },
    )
    .await?;
    let request = admitted["request"]
        .as_str()
        .ok_or("missing admitted request")?;
    #[cfg(test)]
    crate::store::tests::commit_barrier("aux-reserved-before-dispatch").await;
    if admitted["observe_only"] != true {
        transport::run(store, plugin, generation, request).await?;
    }
    write(
        store,
        Change::Result {
            plugin: plugin.into(),
            generation: generation.into(),
            params: serde_json::json!({"request":request}),
            page: false,
        },
    )
    .await
}

async fn write(store: &Store, change: Change) -> Result<Value> {
    super::write(store, super::Change::AuxModel(Box::new(change))).await
}
