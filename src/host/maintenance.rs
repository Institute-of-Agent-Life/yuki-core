mod background;
mod content;
mod fact_sources;
mod preview;
mod quality;
mod reads;
mod reembed;
mod scope;
mod tools;
use crate::{Result, store::Store};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqliteConnection;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub actor: String,
    pub entry: String,
    pub methods: BTreeSet<String>,
    pub purposes: BTreeSet<String>,
    #[serde(default)]
    pub routes: BTreeSet<String>,
    #[serde(default)]
    pub embedding_routes: BTreeSet<String>,
    pub write_subject: Option<crate::memory::facts::Subject>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub scopes: BTreeMap<String, Scope>,
    #[serde(default)]
    pub embedding_routes: BTreeMap<String, crate::memory::embedding::profile::Profile>,
}
impl Policy {
    pub(super) fn validate(&self, install: &super::Install) -> Result<()> {
        if self.scopes.is_empty() || !install.grants.contains("forget.owner") {
            return Err("maintenance requires installed scopes and erasure support".into());
        }
        for profile in self.embedding_routes.values() {
            crate::memory::embedding::Config {
                mode: crate::memory::embedding::Mode::Hybrid,
                embedding: Some(profile.clone()),
            }
            .validate()?;
        }
        for (name, scope) in &self.scopes {
            if name.is_empty()
                || name.len() > 128
                || scope.actor.is_empty()
                || scope.entry.is_empty()
                || scope.methods.is_empty()
                || scope.purposes.is_empty()
                || scope
                    .methods
                    .iter()
                    .any(|m| !m.starts_with("maintenance.") || !install.grants.contains(m))
                || scope
                    .embedding_routes
                    .iter()
                    .any(|route| !self.embedding_routes.contains_key(route))
                || scope
                    .routes
                    .iter()
                    .any(|route| !install.aux_routes.contains_key(route))
            {
                return Err("invalid installed maintenance authority".into());
            }
        }
        Ok(())
    }
}
pub struct Change {
    pub plugin: String,
    pub generation: String,
    pub method: String,
    pub params: Value,
    pub paid_request: Option<String>,
}
pub(super) async fn configure(db: &mut SqliteConnection, install: &super::Install) -> Result<()> {
    if let Some(policy) = &install.maintenance {
        sqlx::query("INSERT OR IGNORE INTO host_maintenance VALUES(?,?,?)")
            .bind(&install.id)
            .bind(install.revision()?)
            .bind(serde_json::to_string(policy)?)
            .execute(db)
            .await?;
    }
    Ok(())
}
pub(super) async fn callback(
    store: &Store,
    plugin: &str,
    generation: &str,
    method: &str,
    params: Value,
) -> Result<Value> {
    let change = || Change {
        plugin: plugin.into(),
        generation: generation.into(),
        method: method.into(),
        params: params.clone(),
        paid_request: None,
    };
    let first = super::write(store, super::Change::Maintenance(Box::new(change()))).await?;
    if method == "maintenance.extract.preview" && params.get("request").is_none() {
        let receipt = super::aux_model::callback(
            store,
            plugin,
            generation,
            "llm.complete",
            first["model_params"].clone(),
        )
        .await?;
        if !matches!(receipt["state"].as_str(), Some("response" | "paired")) {
            return Ok(serde_json::json!({"preview":null,"receipt":receipt}));
        }
        let mut ready = change();
        ready.paid_request = Some(
            receipt["request"]
                .as_str()
                .ok_or("missing paid extraction request")?
                .into(),
        );
        return super::write(store, super::Change::Maintenance(Box::new(ready))).await;
    }
    if method == "maintenance.reembed" && first["dispatch_now"] == true {
        let logical = first["logical"]
            .as_str()
            .ok_or("missing original embedding job")?;
        let profile = reembed::profile(store, plugin, logical).await?;
        let _outcome =
            crate::memory::embedding::run_scoped(store, std::sync::Arc::new(profile), logical)
                .await;
        return super::write(store, super::Change::Maintenance(Box::new(change()))).await;
    }
    Ok(first)
}
pub(super) async fn apply(db: &mut SqliteConnection, c: &Change) -> Result<Value> {
    let def = super::grants::schema(&c.method, "plugin_to_host", &c.params)?;
    super::domain::active(
        db,
        &c.plugin,
        &c.generation,
        def["grant"].as_str().ok_or("missing grant")?,
    )
    .await?;
    let granted = scope::resolve(db, c).await?;
    if c.method == "maintenance.extract.preview"
        && c.params.get("request").is_none()
        && c.paid_request.is_none()
    {
        return background::open(db, c, &granted).await;
    }
    let result = match c.method.as_str() {
        "maintenance.tool.page" => tools::page(db, c, &granted).await?,
        "maintenance.dependencies" => content::dependencies(db, c, &granted).await?,
        "maintenance.content" => content::read(db, c, &granted).await?,
        "maintenance.model.open" => background::open(db, c, &granted).await?,
        "maintenance.reembed" => reembed::prepare(db, c, &granted).await?,
        "maintenance.extract.preview" => preview::prepare(db, c, &granted).await?,
        "maintenance.extract.apply" => preview::apply(db, c, &granted).await?,
        "maintenance.history.page" => reads::history(db, c, &granted).await?,
        "maintenance.fact.page" => reads::facts(db, c, &granted, false).await?,
        "maintenance.evidence" => reads::facts(db, c, &granted, true).await?,
        "maintenance.audit" => reads::audit(db, c, &granted).await?,
        "maintenance.audit.apply" => reads::apply_audit(db, c, &granted).await?,
        _ => return Err("maintenance method not available".into()),
    };
    if !jsonschema::validator_for(&def["result"])?.is_valid(&result) {
        return Err("maintenance response violates public schema".into());
    }
    Ok(result)
}

pub(crate) use background::{is_background, resolve_origin};
pub(crate) async fn authorize(db: &mut SqliteConnection, execution: &str) -> Result<()> {
    background::authorize(db, execution).await?;
    reembed::authorize(db, execution).await
}
