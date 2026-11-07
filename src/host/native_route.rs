//! Original native routing metadata is separate from the canonical admission key.
use super::gateway_media::Identity;
use crate::{Result, privacy::source};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub(super) async fn record(
    db: &mut SqliteConnection,
    plugin: &str,
    params: &Value,
    identity: Option<&Identity>,
    event: i64,
    created: bool,
) -> Result<()> {
    let Some(root) = params.get("topic_root") else {
        return Ok(());
    };
    let root = root.as_str().ok_or("invalid original topic root")?;
    if params["topic"].as_str().is_none_or(str::is_empty)
        || params["native_id"].as_str().is_none_or(str::is_empty)
    {
        return Err("original topic root requires its native topic and message".into());
    }
    let identity =
        serde_json::to_string(identity.ok_or("original topic requires application identity")?)?;
    if source::is_erased(db, event).await? {
        return Ok(());
    }
    let current = sqlx::query("SELECT i.namespace,i.platform,i.bot_stage,json_extract(a.value,'$.deployment') AS deployment
        FROM host_installations i JOIN core_meta a ON a.key='plugin-approval:'||i.plugin WHERE i.plugin=?")
        .bind(plugin).fetch_one(&mut *db).await?;
    let previous = sqlx::query("SELECT * FROM gateway_topic_origins WHERE event=?")
        .bind(event)
        .fetch_optional(&mut *db)
        .await?;
    if let Some(previous) = previous {
        if previous.get::<&str, _>("plugin") != plugin
            || previous.get::<&str, _>("topic_root") != root
            || previous.get::<&str, _>("identity") != identity
            || previous.get::<&str, _>("namespace") != current.get::<&str, _>("namespace")
            || previous.get::<&str, _>("platform") != current.try_get::<&str, _>("platform")?
            || previous.get::<&str, _>("stage") != current.try_get::<&str, _>("bot_stage")?
            || previous.get::<&str, _>("deployment") != current.try_get::<&str, _>("deployment")?
        {
            return Err("original topic receipt differs".into());
        }
    } else if created {
        sqlx::query("INSERT INTO gateway_topic_origins VALUES(?,?,?,?,?,?,?,?)")
            .bind(event)
            .bind(plugin)
            .bind(current.get::<&str, _>("namespace"))
            .bind(current.try_get::<&str, _>("platform")?)
            .bind(current.try_get::<&str, _>("bot_stage")?)
            .bind(current.try_get::<&str, _>("deployment")?)
            .bind(identity)
            .bind(root)
            .execute(db)
            .await?;
    }
    Ok(())
}

pub(super) async fn original(
    db: &mut SqliteConnection,
    plugin: &str,
    execution: &str,
    entry: &str,
) -> Result<Option<Value>> {
    let row = sqlx::query("SELECT o.*,e.entry_id,d.topic,d.closed_at,i.namespace AS current_namespace,i.platform AS current_platform,
        i.bot_stage AS current_stage,json_extract(a.value,'$.deployment') AS current_deployment,
        json_extract(c.value,'$.gateway_media') AS current_identity
        FROM executions x JOIN events e ON e.id=x.source JOIN gateway_topic_origins o ON o.event=e.id
        JOIN entries d ON d.id=e.entry_id
        JOIN host_installations i ON i.plugin=o.plugin
        JOIN core_meta a ON a.key='plugin-approval:'||i.plugin
        JOIN core_meta c ON c.key='plugin-install:'||i.plugin
        WHERE x.id=? AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)")
        .bind(execution).fetch_optional(db).await?;
    let Some(row) = row else { return Ok(None) };
    let identity: Value = serde_json::from_str(row.get("identity"))?;
    let current_identity: Option<&str> = row.try_get("current_identity")?;
    let current_identity: Value =
        serde_json::from_str(current_identity.ok_or("original topic application unavailable")?)?;
    if row.get::<&str, _>("plugin") != plugin
        || row.get::<&str, _>("entry_id") != entry
        || row.get::<Option<&str>, _>("topic").is_none()
        || row.get::<Option<i64>, _>("closed_at").is_some()
        || row.get::<&str, _>("namespace") != row.get::<&str, _>("current_namespace")
        || row.get::<&str, _>("platform") != row.try_get::<&str, _>("current_platform")?
        || row.get::<&str, _>("stage") != row.try_get::<&str, _>("current_stage")?
        || row.get::<&str, _>("deployment") != row.try_get::<&str, _>("current_deployment")?
        || identity != current_identity
    {
        return Err("original topic deployment or route changed".into());
    }
    Ok(Some(json!(row.get::<&str, _>("topic_root"))))
}

pub(super) async fn check(
    db: &mut SqliteConnection,
    plugin: &str,
    execution: &str,
    args: &Value,
) -> Result<()> {
    let entry = args["route"]["entry"].as_str().unwrap_or_default();
    if args["route"].get("topic_root") != original(db, plugin, execution, entry).await?.as_ref() {
        return Err("original topic route differs".into());
    }
    Ok(())
}
