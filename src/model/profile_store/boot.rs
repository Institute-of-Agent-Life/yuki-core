use super::{Service, catalog::Catalog, files};
use crate::{
    Result,
    config::Config,
    memory::embedding::profile::{Dialect, Mode},
};
use std::sync::Arc;

impl Service {
    /// Project restart-only connections into already enabled native consumers.
    /// Installing or enabling optional plugin owners remains a deployment action.
    pub(crate) fn configure_connections(
        &mut self,
        config: &mut Config,
        pending: bool,
    ) -> Result<()> {
        if let Some(search) = &config.search {
            self.baseline.bridge_search = search.bridge();
            self.baseline.bridge_search_limit = search.bridge().then_some(search.max_results);
            self.allow_native = Some(search.inline());
        }
        if pending {
            // A half-renamed connection must not start a fresh embedding provider.
            if config.memory.mode == Mode::Hybrid {
                config.memory.mode = Mode::LexicalOnly;
                config.memory.embedding = None;
                self.connections_suspended = true;
            }
            return Ok(());
        }
        let bundle = files::Bundle::read(&self.root, &self.filename)?;
        let snapshot = bundle
            .configuration
            .as_ref()
            .ok_or("configuration_unavailable")?;
        let catalog = Catalog::parse(std::str::from_utf8(&snapshot.data)?)?;
        if config.memory.mode == Mode::Hybrid
            && let Some(connection) = catalog.public.get("embedding").filter(|v| !v.is_null())
        {
            if connection["enabled"] == false {
                config.memory.mode = Mode::LexicalOnly;
                config.memory.embedding = None;
            } else {
                let profile = config
                    .memory
                    .embedding
                    .as_mut()
                    .ok_or("embedding_policy_unavailable")?;
                profile.base_url = connection["base_url"]
                    .as_str()
                    .ok_or("validation_error")?
                    .into();
                profile.model = connection["model"]
                    .as_str()
                    .ok_or("validation_error")?
                    .into();
                profile.provider = match connection["provider"].as_str().unwrap_or("qwen_dashscope")
                {
                    "qwen_dashscope" => Dialect::QwenDashscope,
                    "openai_compatible" => Dialect::OpenaiCompatible,
                    _ => return Err("validation_error".into()),
                };
                profile.dimensions =
                    usize::try_from(connection["dimensions"].as_u64().unwrap_or(1024))?;
                if connection["document_template_version"]
                    .as_u64()
                    .unwrap_or(1)
                    != 1
                {
                    return Err("unsupported_embedding_document_template".into());
                }
                if let Some(instruction) = connection["query_instruct"].as_str() {
                    profile.query_instruction = instruction.into();
                }
                profile.key_env = connection["api_key_env"]
                    .as_str()
                    .filter(|v| !v.is_empty())
                    .map(str::to_owned);
                profile.credential = profile
                    .key_env
                    .as_ref()
                    .map(|alias| {
                        bundle
                            .keys
                            .get(alias)
                            .cloned()
                            .or_else(|| std::env::var(alias).ok())
                            .filter(|value| !value.is_empty())
                            .map(Arc::new)
                            .ok_or("embedding_credential_unavailable")
                    })
                    .transpose()?;
                profile.credential_hash = profile
                    .credential
                    .as_ref()
                    .map(|value| super::digest(value.as_bytes()));
            }
        }
        config.memory.validate()?;
        if let Some(search) = &mut config.search {
            if let Some(mode) = catalog.public["search"]
                .get("mode")
                .filter(|v| !v.is_null())
            {
                search.mode = serde_json::from_value(mode.clone())?;
            }
            if catalog.public["search_backend"] == "tavily" {
                search.strategy = crate::model::search::Strategy::External;
            }
            search.validate()?;
            self.baseline.bridge_search = search.bridge();
            self.baseline.bridge_search_limit = search.bridge().then_some(search.max_results);
            self.allow_native = Some(search.inline());
        }
        *self
            .boot_revision
            .write()
            .map_err(|_| "configuration_unavailable")? = Some(bundle.revision);
        Ok(())
    }
}
