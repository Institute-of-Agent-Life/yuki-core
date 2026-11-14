use super::{
    Change, reads,
    scope::{self, Granted},
};
use crate::{Result, identity::new_id, memory, runtime::execution};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub(super) async fn create(
    db: &mut SqliteConnection,
    c: &Change,
    g: &Granted,
    slots: i64,
    units: i64,
) -> Result<String> {
    let operation = c.params["key"]
        .as_str()
        .or_else(|| c.params["operation"].as_str())
        .ok_or("original maintenance operation required")?;
    let digest = memory::scope::hash(&c.params);
    if let Some((exec,original))=sqlx::query_as::<_,(String,String)>("SELECT execution,digest FROM maintenance_roots WHERE plugin=? AND method=? AND operation=?")
        .bind(&c.plugin).bind(&c.method).bind(operation).fetch_optional(&mut *db).await? {
        if original!=digest{return Err("original maintenance root changed".into());}
        authorize(db,&exec).await?;
        return Ok(exec);
    }
    let id = new_id(db).await?;
    let origin = format!("maintenance:{id}");
    sqlx::query("INSERT INTO main_origins(ref,kind,conversation) VALUES(?,'plugin',?)")
        .bind(&origin)
        .bind(&g.policy.entry)
        .execute(&mut *db)
        .await?;
    sqlx::query("INSERT INTO executions(id,origin,state) VALUES(?,?,'running')")
        .bind(&id)
        .bind(origin)
        .execute(&mut *db)
        .await?;
    sqlx::query("INSERT INTO maintenance_roots VALUES(?,?,?,?,?,?,?,?,?)")
        .bind(&id)
        .bind(&c.plugin)
        .bind(&g.revision)
        .bind(c.params["scope"].as_str())
        .bind(c.params["purpose"].as_str())
        .bind(&c.method)
        .bind(operation)
        .bind(digest)
        .bind(c.params.to_string())
        .execute(&mut *db)
        .await?;
    for source in c.params["sources"].as_array().into_iter().flatten() {
        sqlx::query("INSERT OR IGNORE INTO main_dependencies VALUES(?,?)")
            .bind(&id)
            .bind(source["event"].as_i64())
            .execute(&mut *db)
            .await?;
    }
    let facts = super::fact_sources::validate(db, g, &c.params["facts"], None).await?;
    let tools = super::tools::validate(db, g, &c.params["tools"]).await?;
    for event in facts.all.into_iter().chain(tools.all) {
        sqlx::query("INSERT OR IGNORE INTO main_dependencies VALUES(?,?)")
            .bind(&id)
            .bind(event)
            .execute(&mut *db)
            .await?;
    }
    execution::root(db, &id, slots, units, Some(0)).await?;
    Ok(id)
}
pub(super) async fn open(db: &mut SqliteConnection, c: &Change, g: &Granted) -> Result<Value> {
    if (c.method == "maintenance.extract.preview" && c.params["format"] != "extraction")
        || !g.policy.methods.contains("maintenance.evidence")
        || g.policy.write_subject.is_none()
    {
        return Err(
            "direct extraction requires explicit reviewed subject and evidence grants".into(),
        );
    }
    let route_name = c.params["route"]
        .as_str()
        .ok_or("original extraction route required")?;
    if !g.policy.routes.contains(route_name) {
        return Err("extraction route outside installed scope".into());
    }
    let body: String = sqlx::query_scalar(
        "SELECT body FROM host_aux_routes WHERE plugin=? AND revision=? AND name=?",
    )
    .bind(&c.plugin)
    .bind(&g.revision)
    .bind(route_name)
    .fetch_one(&mut *db)
    .await?;
    let route: crate::host::aux_model::Route = serde_json::from_str(&body)?;
    if !route.origins.contains("maintenance")
        || !route.entries.contains(&g.policy.entry)
        || !route
            .purposes
            .contains(c.params["purpose"].as_str().ok_or("purpose required")?)
    {
        return Err("auxiliary route excludes original maintenance source".into());
    }
    let mut primary = vec![];
    let mut context = vec![];
    let mut seen = std::collections::BTreeSet::new();
    for reference in c.params["sources"]
        .as_array()
        .ok_or("original selected sources required")?
    {
        if !seen.insert(reference["event"].as_i64().ok_or("event required")?) {
            return Err("duplicate selected source".into());
        }
        let event = reads::original(
            db,
            c,
            g,
            reference["event"].as_i64().ok_or("event required")?,
        )
        .await?;
        if reference["version"] != event.version {
            return Err("original extraction source changed".into());
        }
        if reference["role"] == "primary" {
            primary.push(event.payload());
        } else {
            context.push(event.payload());
        }
    }
    let facts = super::fact_sources::validate(db, g, &c.params["facts"], None).await?;
    super::fact_sources::retain(db, &c.plugin, &c.params["facts"]).await?;
    let tools = super::tools::validate(db, g, &c.params["tools"]).await?;
    for reference in c.params["tools"].as_array().into_iter().flatten() {
        let value = super::tools::full(
            db,
            g,
            reference["receipt"].as_i64().ok_or("receipt required")?,
        )
        .await?;
        super::tools::retain(db, &c.plugin, &value).await?;
    }
    let primary_fact = c.params["facts"]
        .as_array()
        .is_some_and(|refs| refs.iter().any(|r| r["role"] == "primary"));
    if primary.is_empty() && !primary_fact && tools.primary.is_empty() {
        return Err("primary sources required".into());
    }
    if c.method == "maintenance.extract.preview" && (!facts.all.is_empty() || !tools.all.is_empty())
    {
        return Err(
            "direct extraction accepts original events; fact review uses a scoped model origin"
                .into(),
        );
    }
    let root = create(
        db,
        c,
        g,
        route.profile.root_slots,
        route.profile.max_input_units,
    )
    .await?;
    if c.method == "maintenance.model.open" {
        return Ok(
            json!({"origin":{"maintenance":root},"route":route_name,"purpose":c.params["purpose"]}),
        );
    }
    let contract: Value =
        serde_json::from_str(include_str!("../../../fixtures/extract/v4-batch.json"))?;
    Ok(
        json!({"model_params":{"origin":{"maintenance":root},"call_id":"original-extraction","route":route_name,"purpose":c.params["purpose"],"text":json!({"events":primary,"conversation_context":context}).to_string(),"context":[contract["instruction"]],"task":{"mode":c.params["mode"],"schema":contract["schema"],"validation_retries":0}}}),
    )
}
pub(crate) async fn is_background(db: &mut SqliteConnection, exec: &str) -> Result<bool> {
    Ok(sqlx::query_scalar("WITH RECURSIVE family(id) AS(SELECT ? UNION SELECT o.parent FROM family f JOIN executions x ON x.id=f.id JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL) SELECT EXISTS(SELECT 1 FROM family f JOIN maintenance_roots r ON r.execution=f.id)").bind(exec).fetch_one(db).await?)
}
pub(crate) async fn authorize(db: &mut SqliteConnection, exec: &str) -> Result<()> {
    let rows=sqlx::query("WITH RECURSIVE family(id) AS(SELECT ? UNION SELECT o.parent FROM family f JOIN executions x ON x.id=f.id JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL) SELECT r.* FROM family f JOIN maintenance_roots r ON r.execution=f.id")
        .bind(exec).fetch_all(&mut *db).await?;
    for row in rows {
        let plugin: String = row.get("plugin");
        let generation: String = sqlx::query_scalar(
            "SELECT generation FROM host_installations WHERE plugin=? AND enabled AND online",
        )
        .bind(&plugin)
        .fetch_one(&mut *db)
        .await?;
        let method: String = row.get("method");
        super::super::domain::active(db, &plugin, &generation, &method).await?;
        let c = Change {
            plugin,
            generation,
            method,
            params: serde_json::from_str(row.get("params"))?,
            paid_request: None,
            foreground: None,
        };
        let g = scope::resolve(db, &c).await?;
        if g.revision != row.get::<String, _>("revision") {
            return Err("original maintenance installation changed".into());
        }
        if c.method == "maintenance.reembed" {
            memory::scope::writable(
                db,
                &g.reader,
                c.params["fact"].as_str().ok_or("fact required")?,
            )
            .await?;
            let revision: i64 = sqlx::query_scalar("SELECT revision FROM memory_facts WHERE id=?")
                .bind(c.params["fact"].as_str())
                .fetch_one(&mut *db)
                .await?;
            if c.params["revision"] != revision {
                return Err("original maintenance fact changed".into());
            }
        } else {
            super::fact_sources::validate(db, &g, &c.params["facts"], None).await?;
            super::tools::validate(db, &g, &c.params["tools"]).await?;
            for reference in c.params["sources"]
                .as_array()
                .ok_or("source selection required")?
            {
                let source = memory::extract::source::load_reviewed(
                    db,
                    reference["event"].as_i64().ok_or("event required")?,
                    "UTC",
                )
                .await?
                .ok_or("original source erased")?;
                if !g.reader.admits_entry(&source.entry) || reference["version"] != source.version {
                    return Err("original background source changed".into());
                }
            }
        }
    }
    Ok(())
}
pub(crate) async fn resolve_origin(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    root: &str,
    route: &str,
) -> Result<String> {
    let (entry,params,method):(String,String,String)=sqlx::query_as("SELECT o.conversation,r.params,r.method FROM maintenance_roots r JOIN executions x ON x.id=r.execution JOIN main_origins o ON o.ref=x.origin WHERE r.execution=? AND r.plugin=? AND r.method IN('maintenance.extract.preview','maintenance.model.open')")
        .bind(root).bind(plugin).fetch_one(&mut *db).await?;
    super::super::domain::active(db, plugin, generation, &method).await?;
    if serde_json::from_str::<Value>(&params)?["route"] != route {
        return Err("maintenance origin belongs to another original route".into());
    }
    authorize(db, root).await?;
    Ok(entry)
}
