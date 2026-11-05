use super::{Change, Policy};
use crate::{Result, host::aux_model, identity, privacy::source};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub(super) struct Authority {
    pub revision: String,
    pub actor: Option<String>,
    pub root: Option<String>,
    pub entry: Option<String>,
    pub space: Option<String>,
    pub parent: Option<String>,
    pub event: Option<i64>,
    pub policy: Policy,
}
impl Authority {
    pub fn kind(&self) -> &'static str {
        if self.space.is_some() {
            "group"
        } else if self.actor.is_some() {
            "user"
        } else {
            "plugin"
        }
    }
}
pub(super) async fn resolve(
    db: &mut SqliteConnection,
    c: &Change,
    route_name: &str,
    purpose: &str,
    origin: &Value,
    new: bool,
) -> Result<Authority> {
    let (revision,body):(String,String)=sqlx::query_as("SELECT p.revision,p.body FROM host_session_policies p JOIN host_installations i ON i.plugin=p.plugin AND i.revision=p.revision WHERE p.plugin=? AND i.enabled AND i.online")
        .bind(&c.plugin).fetch_one(&mut *db).await?;
    let policy: Policy = serde_json::from_str(&body)?;
    if !policy.routes.contains(route_name) {
        return Err("session route outside installed policy".into());
    }
    let (_, route) = aux_model::scope::route(db, &c.plugin, route_name, purpose).await?;
    if !route.origins.contains("session") {
        return Err("route excludes isolated sessions".into());
    }
    let (actor, entry, parent, event) = if origin.is_null() {
        if !policy.background {
            return Err("actorless sessions require an installed background grant".into());
        }
        (None, None, None, None)
    } else {
        if origin.get("handle").is_none() && origin.get("event").is_none() {
            return Err("original call or observed event authority required".into());
        }
        let original = aux_model::scope::resolve_non_session(
            db,
            &c.plugin,
            &c.generation,
            origin,
            route_name,
            &route,
            new,
        )
        .await?;
        let actor: Option<String> = if let Some(event) = original.event {
            sqlx::query_scalar("SELECT raw_actor FROM events WHERE id=?")
                .bind(event)
                .fetch_one(&mut *db)
                .await?
        } else {
            sqlx::query_scalar("WITH RECURSIVE family(id) AS(SELECT ? UNION SELECT o.parent FROM family f JOIN executions x ON x.id=f.id JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL) SELECT x.original_actor FROM family f JOIN executions x ON x.id=f.id WHERE x.original_actor IS NOT NULL LIMIT 1")
                .bind(&original.parent).fetch_optional(&mut *db).await?
        };
        (actor, original.entry, original.parent, original.event)
    };
    let root: Option<String> = if let Some(actor) = &actor {
        Some(sqlx::query_scalar("SELECT r.id FROM persons p JOIN persons r ON r.id=coalesce(p.current_root,p.id) WHERE p.id=? AND p.enabled AND r.enabled").bind(actor).fetch_one(&mut *db).await?)
    } else {
        None
    };
    let space: Option<String> = if let Some(entry) = &entry {
        sqlx::query_scalar(
            "SELECT CASE WHEN scope='group' THEN space ELSE NULL END FROM entries WHERE id=?",
        )
        .bind(entry)
        .fetch_one(&mut *db)
        .await?
    } else {
        None
    };
    Ok(Authority {
        revision,
        actor,
        root,
        entry,
        space,
        parent,
        event,
        policy,
    })
}
pub(super) async fn current(
    db: &mut SqliteConnection,
    c: &Change,
    new: bool,
) -> Result<(sqlx::sqlite::SqliteRow, Authority)> {
    let id = c.params["session"]
        .as_str()
        .ok_or("original session required")?;
    let row = sqlx::query("SELECT * FROM plugin_sessions WHERE plugin=? AND id=?")
        .bind(&c.plugin)
        .bind(id)
        .fetch_one(&mut *db)
        .await?;
    let authority = resolve(
        db,
        c,
        row.get("route"),
        row.get("purpose"),
        &c.params["authority"],
        new,
    )
    .await?;
    if row.get::<&str, _>("revision") != authority.revision {
        return Err("original session installation changed".into());
    }
    let kind: &str = row.get("scope_kind");
    let owner: Option<&str> = row.get("person");
    let root:Option<String>=sqlx::query_scalar("SELECT r.id FROM persons p JOIN persons r ON r.id=coalesce(p.current_root,p.id) WHERE p.id=? AND p.enabled AND r.enabled").bind(owner).fetch_optional(&mut *db).await?;
    let owner_matches = owner.is_none() || (root.is_some() && root == authority.root);
    let allowed = match kind {
        "user" => root.is_some() && owner_matches,
        "group" => {
            owner_matches
                && authority.space.is_some()
                && authority.space.as_deref() == row.get::<Option<&str>, _>("space")
        }
        "plugin" => {
            let entry: Option<&str> = row.get("entry");
            entry.is_none() || authority.entry.as_deref() == entry
        }
        _ => false,
    };
    if !allowed {
        return Err("session outside original plugin and caller scope".into());
    }
    Ok((row, authority))
}
pub(super) async fn retain(db: &mut SqliteConnection, plugin: &str, session: &str) -> Result<()> {
    let events: Vec<i64> = sqlx::query_scalar(
        "SELECT source FROM plugin_session_sources WHERE plugin=? AND session=? ORDER BY source",
    )
    .bind(plugin)
    .bind(session)
    .fetch_all(&mut *db)
    .await?;
    for event in events {
        source::acquire(db, plugin, &format!("session:{session}"), event).await?;
    }
    Ok(())
}
pub(super) async fn bind(
    db: &mut SqliteConnection,
    c: &Change,
    session: &str,
    authority: &Authority,
) -> Result<()> {
    let events:Vec<i64>=sqlx::query_scalar("SELECT ?1 WHERE ?1 IS NOT NULL UNION SELECT source FROM executions WHERE id=?2 AND source IS NOT NULL UNION SELECT event FROM main_dependencies WHERE execution=?2")
        .bind(authority.event).bind(&authority.parent).fetch_all(&mut *db).await?;
    for event in events {
        sqlx::query("INSERT OR IGNORE INTO plugin_session_sources VALUES(?,?,?)")
            .bind(&c.plugin)
            .bind(session)
            .bind(event)
            .execute(&mut *db)
            .await?;
    }
    retain(db, &c.plugin, session).await
}
pub(super) fn context(authority: &Authority, profile: &str) -> Result<Option<Value>> {
    if !authority.policy.context_profiles.contains(profile) {
        return Err("session context outside installed grant".into());
    }
    let value = match profile {
        "none" => return Ok(None),
        "current_user" if authority.actor.is_some() => json!({"person_id":authority.actor}),
        "current_group" if authority.space.is_some() => {
            json!({"space":authority.space,"person_id":authority.actor})
        }
        _ => return Err("session context has no original caller or group".into()),
    };
    Ok(Some(json!({"role":"system","content":value.to_string()})))
}
pub(super) async fn boot(db: &mut SqliteConnection) -> Result<String> {
    let existing: Option<String> =
        sqlx::query_scalar("SELECT generation FROM plugin_session_boot WHERE singleton=1")
            .fetch_optional(&mut *db)
            .await?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = identity::new_id(db).await?;
    sqlx::query("INSERT INTO plugin_session_boot VALUES(1,?)")
        .bind(&id)
        .execute(db)
        .await?;
    Ok(id)
}
