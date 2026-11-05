use super::Policy;
use crate::Result;
use serde_json::Value;
use sqlx::{Row, SqliteConnection};

pub(super) struct Target {
    pub entry: String,
    pub receiver: String,
    pub person: Option<String>,
    pub space: Option<String>,
    pub window: i64,
}

pub(super) async fn policy(db: &mut SqliteConnection, plugin: &str) -> Result<(String, Policy)> {
    let (revision, body): (String, String) = sqlx::query_as(
        "SELECT p.revision,p.body FROM host_notification_policies p
         JOIN host_installations i ON i.plugin=p.plugin AND i.revision=p.revision
         JOIN notification_publishers publisher ON publisher.plugin=i.plugin
         JOIN accounts account ON account.id=publisher.account AND account.namespace=i.namespace
         WHERE p.plugin=? AND i.enabled",
    )
    .bind(plugin)
    .fetch_one(&mut *db)
    .await?;
    let mut policy: Policy = serde_json::from_str(&body)?;
    policy.targets = sqlx::query_as::<_, (String, String)>(
        "SELECT entry,receiver FROM notification_target_scope WHERE plugin=?",
    )
    .bind(plugin)
    .fetch_all(db)
    .await?
    .into_iter()
    .collect();
    Ok((revision, policy))
}

pub(super) async fn entry(db: &mut SqliteConnection, policy: &Policy, id: &str) -> Result<Target> {
    let receiver = policy
        .targets
        .get(id)
        .ok_or("notification target not granted")?;
    let row = sqlx::query(
        "SELECT e.scope,e.space,e.bot_stage,a.original_person,a.status,
                p.enabled AS person_enabled,r.enabled AS root_enabled,
                b.platform,b.namespace
         FROM entries e JOIN assignments bot ON bot.id=? AND bot.kind='self' AND bot.status='active'
         JOIN accounts b ON b.id=bot.account_id
         LEFT JOIN assignments a ON a.id=e.actor_stage
         LEFT JOIN persons p ON p.id=a.original_person LEFT JOIN persons r ON r.id=coalesce(p.current_root,p.id)
         WHERE e.id=? AND e.closed_at IS NULL",
    )
    .bind(receiver)
    .bind(id)
    .fetch_one(&mut *db)
    .await?;
    let person: Option<String> = row.get("original_person");
    let space: Option<String> = row.get("space");
    if row.get::<&str, _>("scope") == "private" {
        if row.get::<Option<&str>, _>("bot_stage") != Some(receiver)
            || row.get::<Option<&str>, _>("status") != Some("active")
            || row.get::<Option<bool>, _>("person_enabled") != Some(true)
            || row.get::<Option<bool>, _>("root_enabled") != Some(true)
        {
            return Err("original private notification target unavailable".into());
        }
    } else {
        let address: (String, String, String) =
            serde_json::from_str(space.as_deref().ok_or("original group required")?)?;
        if address.0 != row.get::<String, _>("platform")
            || address.1 != row.get::<String, _>("namespace")
        {
            return Err("notification receiver outside original group namespace".into());
        }
        if address.0 == "qq"
            && super::super::participation::qq_route(db, &address.1, &address.2)
                .await?
                .is_none_or(|(stage, _, enabled)| !enabled || stage != *receiver)
        {
            return Err("original notification group route unavailable".into());
        }
    }
    Ok(Target {
        entry: id.into(),
        receiver: receiver.clone(),
        person,
        space,
        window: sqlx::query_scalar(
            "SELECT coalesce((SELECT generation FROM main_windows WHERE conversation=?),1)",
        )
        .bind(id)
        .fetch_one(&mut *db)
        .await?,
    })
}

pub(super) async fn select(
    db: &mut SqliteConnection,
    policy: &Policy,
    request: &Value,
) -> Result<Target> {
    let kind = request["target_type"]
        .as_str()
        .ok_or("notification target kind required")?;
    let identity = request["target_id"]
        .as_str()
        .ok_or("notification target identity required")?;
    let selected = request["conversation_id"].as_str();
    let mut found = None;
    for id in policy.targets.keys() {
        if selected.is_some_and(|selected| selected != id) {
            continue;
        }
        let matches: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM entries e LEFT JOIN assignments a ON a.id=e.actor_stage
             LEFT JOIN persons owner ON owner.id=a.original_person
             LEFT JOIN persons requested ON requested.id=?3
             WHERE e.id=?1 AND ((?2='group' AND e.scope='group' AND e.space=?3)
                 OR (?2='private' AND e.scope='private' AND requested.enabled
                     AND coalesce(owner.current_root,owner.id)=coalesce(requested.current_root,requested.id))))",
        )
        .bind(id)
        .bind(kind)
        .bind(identity)
        .fetch_one(&mut *db)
        .await?;
        if matches {
            if found.is_some() {
                return Err(
                    "notification target requires an explicit original conversation".into(),
                );
            }
            found = Some(id.clone());
        }
    }
    entry(
        db,
        policy,
        &found.ok_or("notification target outside installed grant")?,
    )
    .await
}
