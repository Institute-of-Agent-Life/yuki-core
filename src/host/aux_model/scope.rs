use super::Route;
use crate::{
    Result,
    identity::new_id,
    runtime::{effects, execution},
};
use serde_json::Value;
use sqlx::{Row, SqliteConnection};

pub(super) struct Origin {
    pub original: String,
    pub entry: String,
    pub parent: Option<String>,
    pub event: Option<i64>,
}

pub(super) async fn route(
    db: &mut SqliteConnection,
    plugin: &str,
    name: &str,
    purpose: &str,
) -> Result<(String, Route)> {
    let row = sqlx::query(
        "SELECT r.revision,r.body FROM host_aux_routes r
         JOIN host_installations i ON i.plugin=r.plugin AND i.revision=r.revision
         WHERE r.plugin=? AND r.name=? AND i.enabled AND i.online",
    )
    .bind(plugin)
    .bind(name)
    .fetch_one(db)
    .await?;
    let route: Route = serde_json::from_str(row.get("body"))?;
    if !route.purposes.contains(purpose) {
        return Err("purpose outside installed auxiliary route".into());
    }
    Ok((row.get("revision"), route))
}

pub(super) async fn resolve(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    input: &Value,
    route: &Route,
    new_request: bool,
) -> Result<Origin> {
    let (entry, parent, event) = if let Some(handle) = input["handle"].as_str() {
        if !route.origins.contains("call") {
            return Err("route excludes host call origins".into());
        }
        let row = sqlx::query(
            "SELECT c.execution,c.generation,c.state,c.revision,i.revision AS installed,
                    coalesce(x.original_actor,x.origin) AS actor
             FROM host_calls c JOIN executions x ON x.id=c.execution
             JOIN host_installations i ON i.plugin=c.plugin
             WHERE c.handle=? AND c.plugin=?",
        )
        .bind(handle)
        .bind(plugin)
        .fetch_one(&mut *db)
        .await?;
        if row.get::<&str, _>("revision") != row.get::<&str, _>("installed")
            || new_request
                && (row.get::<&str, _>("generation") != generation
                    || row.get::<&str, _>("state") != "dispatched")
        {
            return Err("original host call is unavailable".into());
        }
        let exec: String = row.get("execution");
        effects::authorize(db, &exec, row.get("actor"), new_request).await?;
        let entry: String = sqlx::query_scalar(
            "SELECT coalesce(e.entry_id,o.conversation) FROM executions x
             LEFT JOIN events e ON e.id=x.source LEFT JOIN main_origins o ON o.ref=x.origin
             WHERE x.id=?",
        )
        .bind(&exec)
        .fetch_one(&mut *db)
        .await?;
        (entry, Some(exec), None)
    } else {
        if !route.origins.contains("event") {
            return Err("route excludes observed event origins".into());
        }
        let event = input["event"].as_i64().ok_or("original event required")?;
        let entry: String = sqlx::query_scalar(
            "SELECT e.entry_id FROM events e JOIN entries d ON d.id=e.entry_id
             JOIN persons p ON p.id=e.raw_actor JOIN persons root ON root.id=coalesce(p.current_root,p.id)
             JOIN assignments a ON a.id=e.actor_stage JOIN assignments b ON b.id=e.bot_stage
             WHERE e.id=?1 AND p.enabled AND root.enabled AND d.closed_at IS NULL
             AND a.status='active' AND b.status='active'
             AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)
             AND (NOT ?2 OR EXISTS(SELECT 1 FROM host_observations
                 WHERE plugin=?3 AND generation=?4 AND event=e.id))",
        ).bind(event).bind(new_request).bind(plugin).bind(generation)
            .fetch_one(&mut *db).await?;
        (entry, None, Some(event))
    };
    if !route.entries.contains(&entry) {
        return Err("entry outside installed auxiliary route".into());
    }
    Ok(Origin {
        original: input.to_string(),
        entry,
        parent,
        event,
    })
}

pub(super) async fn execution(
    db: &mut SqliteConnection,
    plugin: &str,
    origin: &Origin,
    route: &Route,
) -> Result<String> {
    if let Some(exec) = sqlx::query_scalar::<_, String>(
        "SELECT execution FROM host_aux_origins WHERE plugin=? AND original=?",
    )
    .bind(plugin)
    .bind(&origin.original)
    .fetch_optional(&mut *db)
    .await?
    {
        execution::eligible(db, &exec).await?;
        return Ok(exec);
    }
    let exec = new_id(db).await?;
    let reference = format!("plugin-aux:{exec}");
    sqlx::query("INSERT INTO main_origins(ref,kind,conversation,parent) VALUES(?,'plugin',?,?)")
        .bind(&reference)
        .bind(&origin.entry)
        .bind(&origin.parent)
        .execute(&mut *db)
        .await?;
    sqlx::query(
        "INSERT INTO executions(id,origin,state,work_id)
         VALUES(?,?,'running',(SELECT work_id FROM executions WHERE id=?))",
    )
    .bind(&exec)
    .bind(&reference)
    .bind(&origin.parent)
    .execute(&mut *db)
    .await?;
    sqlx::query("INSERT INTO host_aux_origins VALUES(?,?,?,?,?)")
        .bind(plugin)
        .bind(&origin.original)
        .bind(&exec)
        .bind(&origin.parent)
        .bind(origin.event)
        .execute(&mut *db)
        .await?;
    if let Some(parent) = &origin.parent {
        if !sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM budget_members WHERE execution=?)",
        )
        .bind(parent)
        .fetch_one(&mut *db)
        .await?
        {
            execution::root(
                db,
                parent,
                route.profile.root_slots,
                route.profile.max_input_units,
            )
            .await?;
        }
        sqlx::query(
            "INSERT INTO budget_members SELECT ?,root FROM budget_members WHERE execution=?",
        )
        .bind(&exec)
        .bind(parent)
        .execute(&mut *db)
        .await?;
        sqlx::query(
            "INSERT OR IGNORE INTO main_dependencies SELECT ?1,id FROM events
             WHERE id IN(SELECT source FROM executions WHERE id=?2
                 UNION SELECT event FROM main_dependencies WHERE execution=?2)",
        )
        .bind(&exec)
        .bind(parent)
        .execute(&mut *db)
        .await?;
    } else {
        execution::root(
            db,
            &exec,
            route.profile.root_slots,
            route.profile.max_input_units,
        )
        .await?;
    }
    if let Some(event) = origin.event {
        sqlx::query("INSERT OR IGNORE INTO main_dependencies VALUES(?,?)")
            .bind(&exec)
            .bind(event)
            .execute(db)
            .await?;
    }
    Ok(exec)
}
