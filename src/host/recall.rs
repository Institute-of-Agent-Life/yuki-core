//! A recall keeps the original sent part, receiver and one physical dispatch claim.
use super::{Hub, calls, domain, grants};
use crate::{Result, social, store::Store};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

struct Selection {
    original: social::recall::Original,
    plugin: String,
    revision: String,
    generation: Option<String>,
    args: Value,
}

async fn select(
    conn: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    operation: &str,
) -> Result<Option<Selection>> {
    let original = social::recall::original(conn, exec, actor, operation).await?;
    if original.platform == "qq" {
        return Ok(None);
    }
    let row = sqlx::query("SELECT c.plugin,c.args,i.revision,i.generation FROM host_calls c LEFT JOIN host_installations i ON i.plugin=c.plugin AND i.bot_stage=?3 AND i.platform=?4 WHERE c.execution=?1 AND c.operation=?2 AND c.method='gateway.dispatch_part' AND c.state='confirmed'")
        .bind(&original.execution).bind(&original.child).bind(&original.stage).bind(&original.platform)
        .fetch_optional(&mut *conn).await?.ok_or("original_gateway_unavailable")?;
    let sent: Value = serde_json::from_str(row.try_get("args")?)?;
    if sent["route"]["entry"] != original.entry {
        return Err("original_gateway_route_changed".into());
    }
    let args = json!({"route":sent["route"],"original":{
        "event_id":original.event,"execution":original.execution,
        "operation":original.child,"native_id":original.locator}});
    Ok(Some(Selection {
        original,
        plugin: row.try_get("plugin")?,
        revision: row
            .try_get::<Option<String>, _>("revision")?
            .unwrap_or_default(),
        generation: row.try_get("generation")?,
        args,
    }))
}

pub(super) async fn check(
    conn: &mut SqliteConnection,
    plugin: &str,
    exec: &str,
    parent: &str,
    args: &Value,
) -> Result<()> {
    let actor: String =
        sqlx::query_scalar("SELECT coalesce(original_actor,origin) FROM executions WHERE id=?")
            .bind(exec)
            .fetch_one(&mut *conn)
            .await?;
    let selected = select(conn, exec, &actor, parent)
        .await?
        .ok_or("original_gateway_unavailable")?;
    if selected.plugin != plugin || selected.args != *args {
        return Err("recall_outside_original_gateway".into());
    }
    Ok(())
}

pub(super) async fn settled(
    conn: &mut SqliteConnection,
    exec: &str,
    operation: &str,
    state: &str,
) -> Result<()> {
    let (original, child): (String, String) = sqlx::query_as("SELECT r.original_execution,r.child FROM operations o JOIN model_recalls r ON r.execution=o.namespace AND r.operation=o.parent WHERE o.namespace=? AND o.operation_id=?")
        .bind(exec).bind(operation).fetch_one(&mut *conn).await?;
    social::record_recall(conn, &original, &child, state).await?;
    Ok(())
}

impl Hub {
    pub(crate) async fn recall(
        &self,
        store: &Store,
        exec: &str,
        actor: &str,
        operation: &str,
    ) -> Result<bool> {
        let prepared = async {
            let mut conn = store.readers().acquire().await?;
            let Some(selected) = select(&mut conn, exec, actor, operation).await? else {
                return Ok(None);
            };
            let prior: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM social_recalls WHERE namespace=? AND child=?)",
            )
            .bind(&selected.original.execution)
            .bind(&selected.original.child)
            .fetch_one(&mut *conn)
            .await?;
            if !prior {
                if grants::methods()["methods"].get("gateway.recall").is_none() {
                    return Err("gateway_recall_not_supported".into());
                }
                domain::active(
                    &mut conn,
                    &selected.plugin,
                    selected
                        .generation
                        .as_deref()
                        .ok_or("original_gateway_unavailable")?,
                    "gateway.recall",
                )
                .await?;
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(Some(selected))
        }
        .await;
        let selected = match prepared {
            Ok(None) => return Ok(false),
            Ok(Some(selected)) => selected,
            Err(error) => {
                crate::tools::reject(store, exec, actor, operation, error.to_string()).await?;
                return Ok(true);
            }
        };
        let granted = social::write(
            store,
            social::Change::ModelRecall(
                exec.into(),
                actor.into(),
                operation.into(),
                selected.original.stage.clone(),
                0,
            ),
        )
        .await;
        let granted = match granted {
            Ok(granted) => granted,
            Err(error) => {
                crate::tools::reject(store, exec, actor, operation, error.to_string()).await?;
                return Ok(true);
            }
        };
        if granted["dispatch"] != true {
            return Ok(true);
        }
        let child = format!("{operation}:gateway-recall");
        let result = self
            .execute(
                store,
                calls::Request {
                    vision: false,
                    plugin: selected.plugin,
                    revision: selected.revision,
                    execution: exec.into(),
                    operation: child.clone(),
                    method: "gateway.recall".into(),
                    name: "recall_own_message".into(),
                    args: selected.args,
                    parent: Some(operation.into()),
                    ordinal: Some(0),
                },
            )
            .await;
        let observed = result.as_ref().ok().and_then(|r| r["state"].as_str());
        let state = match observed {
            Some("confirmed") => "confirmed",
            Some("failed") => "failed",
            _ => {
                let crossed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM host_calls WHERE execution=? AND operation=? AND state IN('dispatched','confirmed','failed','unknown'))")
                    .bind(exec).bind(&child).fetch_one(store.readers()).await?;
                if crossed { "unknown" } else { "failed" }
            }
        };
        social::write(
            store,
            social::Change::RecallReply(
                selected.original.execution,
                selected.original.child,
                state.into(),
            ),
        )
        .await?;
        Ok(true)
    }
}
