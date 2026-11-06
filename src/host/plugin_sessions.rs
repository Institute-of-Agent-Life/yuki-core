mod content;
mod model;
mod scope;
mod writer;
use crate::{Result, store::Store};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqliteConnection;
use std::collections::BTreeSet;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub routes: BTreeSet<String>,
    #[serde(default)]
    pub background: bool,
    pub context_profiles: BTreeSet<String>,
}
impl Policy {
    pub(super) fn validate(&self, install: &super::Install) -> Result<()> {
        if self.routes.is_empty()
            || self
                .routes
                .iter()
                .any(|r| !install.aux_routes.contains_key(r))
            || self.context_profiles.is_empty()
            || self
                .context_profiles
                .iter()
                .any(|p| !matches!(p.as_str(), "none" | "current_user" | "current_group"))
            || !install.grants.contains("forget.owner")
        {
            return Err(
                "isolated sessions require explicit routes, context grants and erasure support"
                    .into(),
            );
        }
        Ok(())
    }
}
#[derive(Clone)]
pub struct Change {
    pub plugin: String,
    pub generation: String,
    pub method: String,
    pub params: Value,
    pub finish: bool,
}
pub(super) async fn configure(db: &mut SqliteConnection, install: &super::Install) -> Result<()> {
    if let Some(policy) = &install.sessions {
        sqlx::query("INSERT OR IGNORE INTO host_session_policies VALUES(?,?,?)")
            .bind(&install.id)
            .bind(install.revision()?)
            .bind(serde_json::to_string(policy)?)
            .execute(db)
            .await?;
    }
    Ok(())
}
pub(super) async fn start(db: &mut SqliteConnection) -> Result<()> {
    sqlx::query("UPDATE plugin_sessions SET state='expired' WHERE persistence='ephemeral' AND state='active'").execute(&mut *db).await?;
    model::scrub_inactive(db).await?;
    sqlx::query("DELETE FROM plugin_session_boot")
        .execute(&mut *db)
        .await?;
    scope::boot(db).await?;
    Ok(())
}
pub(super) async fn callback(
    store: &Store,
    plugin: &str,
    generation: &str,
    method: &str,
    params: Value,
) -> Result<Value> {
    let mut call = Change {
        plugin: plugin.into(),
        generation: generation.into(),
        method: method.into(),
        params,
        finish: false,
    };
    let first = super::write(store, super::Change::PluginSession(Box::new(call.clone()))).await?;
    if let Some(params) = first.get("model_params") {
        super::aux_model::callback(store, plugin, generation, "llm.complete", params.clone())
            .await?;
        call.finish = true;
        return super::write(store, super::Change::PluginSession(Box::new(call))).await;
    }
    Ok(first)
}
pub(super) async fn apply(db: &mut SqliteConnection, c: &Change) -> Result<Value> {
    let schema = super::grants::schema(&c.method, "plugin_to_host", &c.params)?;
    let mut normalized = c.clone();
    if let Some(id) = c.params["session"].as_str() {
        normalized.params["session"] = serde_json::json!(id.to_ascii_lowercase());
    }
    let c = &normalized;
    super::domain::active(
        db,
        &c.plugin,
        &c.generation,
        schema["grant"].as_str().ok_or("session grant required")?,
    )
    .await?;
    let value = if c.method == "session.create" {
        writer::create(db, c).await?
    } else {
        let (session, _) = scope::current(db, c, false).await?;
        match c.method.as_str() {
            "session.sources" => content::sources(db, c, &session).await?,
            "session.page" => content::page(db, c, &session).await?,
            "session.content" => content::read(db, c, &session).await?,
            "session.append" => writer::append(db, c, &session).await?,
            "session.reset" | "session.close" => writer::reset(db, c, &session).await?,
            "session.run" if c.finish => model::finish(db, c, &session).await?,
            "session.run" => model::prepare(db, c, &session).await?,
            _ => return Err("unknown isolated session method".into()),
        }
    };
    if value.get("model_params").is_none()
        && !jsonschema::validator_for(&schema["result"])?.is_valid(&value)
    {
        return Err("isolated session result violates public contract".into());
    }
    Ok(value)
}

pub(crate) use model::{authorize, scrub_inactive};
pub(super) use model::{bind_execution, model_input, resolve_origin};

pub(super) fn visible_text(result: &Value) -> Option<&str> {
    if result["role"] == "assistant" {
        result["content"]
            .as_str()
            .or_else(|| result["content"].is_null().then_some(""))
    } else {
        result["text"].as_str()
    }
}
