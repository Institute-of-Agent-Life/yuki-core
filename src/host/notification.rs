mod grants;
mod scope;
mod transport;
mod writer;

use crate::{Result, identity::new_id, runtime::main::Config, store::Store};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqliteConnection;
use std::collections::BTreeMap;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    #[serde(default)]
    pub targets: BTreeMap<String, String>,
    #[serde(default)]
    pub agent: bool,
}

impl Policy {
    pub(super) fn validate(&self, install: &super::Install) -> Result<()> {
        if self
            .targets
            .iter()
            .any(|(entry, receiver)| entry.is_empty() || receiver.is_empty())
            || !install.grants.contains("notification.publish")
            || !install.grants.contains("forget.owner")
        {
            return Err(
                "notification publication requires valid target grants and erasure support".into(),
            );
        }
        Ok(())
    }
}

pub struct Call {
    pub plugin: String,
    pub generation: String,
    pub method: String,
    pub params: Value,
    pub config: Option<Config>,
}

pub enum Change {
    Call(Box<Call>),
    Delivered {
        publication: String,
        ordinal: i64,
        state: String,
    },
}

pub(super) async fn configure(db: &mut SqliteConnection, install: &super::Install) -> Result<()> {
    let Some(policy) = &install.notifications else {
        return Ok(());
    };
    sqlx::query("INSERT OR IGNORE INTO host_notification_policies VALUES(?,?,?)")
        .bind(&install.id)
        .bind(install.revision()?)
        .bind(serde_json::to_string(policy)?)
        .execute(&mut *db)
        .await?;
    // This account is the authenticated internal publisher. It has no human assignment.
    let id = new_id(db).await?;
    sqlx::query(
        "INSERT OR IGNORE INTO accounts(id,platform,namespace,native) VALUES(?,'plugin',?,?)",
    )
    .bind(id)
    .bind(&install.namespace)
    .bind(&install.id)
    .execute(&mut *db)
    .await?;
    sqlx::query("INSERT OR IGNORE INTO notification_publishers SELECT ?1,id FROM accounts WHERE platform='plugin' AND namespace=?2 AND native=?1")
        .bind(&install.id).bind(&install.namespace).execute(db).await?;
    Ok(())
}

pub(super) async fn callback(
    store: &Store,
    plugin: &str,
    generation: &str,
    method: &str,
    params: Value,
) -> Result<Value> {
    let result = super::write(
        store,
        super::Change::Notification(Change::Call(Box::new(Call {
            plugin: plugin.into(),
            generation: generation.into(),
            method: method.into(),
            params,
            config: store.main.as_deref().cloned(),
        }))),
    )
    .await?;
    #[cfg(test)]
    if method == "notification.publish" {
        crate::store::tests::commit_barrier("notification-published-before-reply").await;
    }
    Ok(result)
}

pub(super) async fn apply(db: &mut SqliteConnection, change: &Change) -> Result<Value> {
    match change {
        Change::Call(call) => {
            let definition = super::grants::schema(&call.method, "plugin_to_host", &call.params)?;
            super::domain::active(
                db,
                &call.plugin,
                &call.generation,
                definition["grant"]
                    .as_str()
                    .ok_or("notification grant required")?,
            )
            .await?;
            let result = match call.method.as_str() {
                "notification.publish" => writer::publish(db, call).await?,
                "notification.read" => writer::read(db, call).await?,
                "notification.sources" => writer::sources(db, call).await?,
                "notification.grant_target" | "notification.revoke_target" => {
                    grants::mutate(db, call).await?
                }
                "notification.status" => grants::status(db, call).await?,
                _ => return Err("unknown notification method".into()),
            };
            if !jsonschema::validator_for(&definition["result"])?.is_valid(&result) {
                return Err("notification receipt violates public contract".into());
            }
            Ok(result)
        }
        Change::Delivered {
            publication,
            ordinal,
            state,
        } => writer::delivered(db, publication, *ordinal, state).await,
    }
}

pub(crate) use transport::{authorize, binding, qq_route, serve};
