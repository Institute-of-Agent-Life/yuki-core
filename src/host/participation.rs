mod feedback;
mod mentions;
mod proposals;
mod self_source;
mod sources;
use crate::{Result, runtime::main::Config, store::Store};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqliteConnection;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub entries: BTreeSet<String>,
    #[serde(default)]
    pub self_scenes: BTreeMap<String, String>,
}
impl Policy {
    pub(super) fn validate(&self, install: &super::Install) -> Result<()> {
        if self.entries.is_empty()
            || self.entries.iter().any(String::is_empty)
            || self
                .self_scenes
                .iter()
                .any(|(entry, stage)| !self.entries.contains(entry) || stage.is_empty())
            || !install.grants.contains("forget.owner")
        {
            return Err(
                "participation requires explicit entries, receivers and erasure support".into(),
            );
        }
        Ok(())
    }
}
pub struct Change {
    pub plugin: String,
    pub generation: String,
    pub method: String,
    pub params: Value,
    pub config: Option<Config>,
}
pub(super) async fn configure(db: &mut SqliteConnection, install: &super::Install) -> Result<()> {
    if let Some(policy) = &install.participation {
        sqlx::query("INSERT OR IGNORE INTO host_participation VALUES(?,?,?)")
            .bind(&install.id)
            .bind(install.revision()?)
            .bind(serde_json::to_string(policy)?)
            .execute(db)
            .await?;
    }
    Ok(())
}
async fn policy(db: &mut SqliteConnection, plugin: &str) -> Result<(String, Policy)> {
    let (revision, body): (String,String) = sqlx::query_as("SELECT p.revision,p.body FROM host_participation p JOIN host_installations i ON i.plugin=p.plugin AND i.revision=p.revision WHERE p.plugin=? AND i.enabled")
        .bind(plugin).fetch_one(db).await?;
    Ok((revision, serde_json::from_str(&body)?))
}
pub(super) async fn callback(
    store: &Store,
    plugin: &str,
    generation: &str,
    method: &str,
    params: Value,
) -> Result<Value> {
    super::write(
        store,
        super::Change::Participation(Box::new(Change {
            plugin: plugin.into(),
            generation: generation.into(),
            method: method.into(),
            params,
            config: crate::model::profile_store::runtime::current(store)
                .await?
                .as_deref()
                .cloned(),
        })),
    )
    .await
}
pub(super) async fn apply(db: &mut SqliteConnection, change: &Change) -> Result<Value> {
    let definition = super::grants::schema(&change.method, "plugin_to_host", &change.params)?;
    super::domain::active(
        db,
        &change.plugin,
        &change.generation,
        definition["grant"].as_str().ok_or("missing grant")?,
    )
    .await?;
    let (revision, policy) = policy(db, &change.plugin).await?;
    let result = match change.method.as_str() {
        "participation.sources" => sources::list(db, change, &policy).await?,
        "participation.source" => sources::read(db, change, &policy).await?,
        "participation.promote" => sources::promote(db, change, &policy).await?,
        "participation.self_propose" => proposals::submit(db, change, &policy, &revision).await?,
        "participation.feedback" => feedback::read(db, change, &policy).await?,
        _ => return Err("unknown participation method".into()),
    };
    if !jsonschema::validator_for(&definition["result"])?.is_valid(&result) {
        return Err("participation response violates schema".into());
    }
    Ok(result)
}
pub(crate) use proposals::{authorize, self_binding, self_group_route};
pub(in crate::host) use sources::authorize_review;

pub(crate) async fn qq_route(
    db: &mut SqliteConnection,
    namespace: &str,
    group: &str,
) -> Result<Option<(String, i64, bool)>> {
    Ok(sqlx::query_as("SELECT r.stage,r.revision,r.enabled FROM recovered_groups r
      JOIN participation_qq_receivers b ON b.stage=r.stage WHERE r.namespace=?1 AND r.native_group=?2
      UNION ALL SELECT g.stage,0,g.enabled FROM participation_qq_groups g
      JOIN participation_qq_receivers b ON b.stage=g.stage WHERE g.namespace=?1 AND g.native_group=?2
      AND NOT EXISTS(SELECT 1 FROM recovered_groups WHERE namespace=?1 AND native_group=?2) LIMIT 1")
      .bind(namespace).bind(group).fetch_optional(db).await?)
}
