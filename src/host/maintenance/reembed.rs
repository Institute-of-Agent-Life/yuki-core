use super::{
    Change, Policy,
    scope::{self, Granted},
};
use crate::{Result, memory, store::Store};
use memory::embedding::profile::Profile;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub(super) async fn prepare(db: &mut SqliteConnection, c: &Change, g: &Granted) -> Result<Value> {
    let route = c.params["route"]
        .as_str()
        .ok_or("embedding route required")?;
    if !g.policy.embedding_routes.contains(route) || g.policy.write_subject.is_none() {
        return Err("scoped reembedding not granted".into());
    }
    let profile = g
        .embedding_routes
        .get(route)
        .ok_or("installed embedding route unavailable")?;
    if c.params["profile"] != profile.identity() {
        return Err("original semantic embedding profile changed".into());
    }
    let fact = c.params["fact"].as_str().ok_or("original fact required")?;
    memory::scope::writable(db, &g.reader, fact).await?;
    let version: i64 = sqlx::query_scalar("SELECT revision FROM memory_facts WHERE id=?")
        .bind(fact)
        .fetch_one(&mut *db)
        .await?;
    if c.params["revision"] != version {
        return Err("original fact version changed".into());
    }
    let operation = c.params["operation"]
        .as_str()
        .ok_or("original reembed operation required")?;
    let digest = memory::scope::hash(&c.params);
    if let Some((old, revision)) = sqlx::query_as::<_, (String, String)>(
        "SELECT digest,revision FROM maintenance_embeddings WHERE plugin=? AND operation=?",
    )
    .bind(&c.plugin)
    .bind(operation)
    .fetch_optional(&mut *db)
    .await?
    {
        if old != digest || revision != g.revision {
            return Err("original reembed operation changed".into());
        }
        return status(db, c, profile).await;
    }
    let parent = super::background::create(db, c, g, 1, 2_000_000).await?;
    let admitted = memory::embedding::prepare_scoped(db, profile, fact, version, &parent).await?;
    sqlx::query("INSERT INTO maintenance_embeddings VALUES(?,?,?,?,?,?,?,?,?,?,?)")
        .bind(&c.plugin)
        .bind(operation)
        .bind(&g.revision)
        .bind(c.params["scope"].as_str())
        .bind(c.params["purpose"].as_str())
        .bind(route)
        .bind(fact)
        .bind(version)
        .bind(digest)
        .bind(admitted["logical"].as_str())
        .bind(admitted.to_string())
        .execute(&mut *db)
        .await?;
    status(db, c, profile).await
}
async fn status(db: &mut SqliteConnection, c: &Change, p: &Profile) -> Result<Value> {
    let row=sqlx::query("SELECT fact,fact_revision,logical,observed FROM maintenance_embeddings WHERE plugin=? AND operation=?")
        .bind(&c.plugin).bind(c.params["operation"].as_str()).fetch_one(&mut *db).await?;
    let logical: Option<String> = row.get("logical");
    let (state, usage, dispatch) = if let Some(logical) = &logical {
        let r=sqlx::query("SELECT j.state,a.state AS attempt,a.usage_json FROM embedding_requests j LEFT JOIN model_attempts a ON a.logical=j.logical WHERE j.logical=? ORDER BY a.ordinal DESC LIMIT 1")
            .bind(logical).fetch_one(&mut *db).await?;
        let attempt: Option<&str> = r.get("attempt");
        let state = if r.get::<&str, _>("state") == "prepared" {
            attempt.unwrap_or("prepared")
        } else {
            r.get("state")
        };
        (
            state.to_owned(),
            r.get::<Option<&str>, _>("usage_json")
                .map(serde_json::from_str::<Value>)
                .transpose()?
                .unwrap_or(Value::Null),
            attempt.is_none() && state == "prepared",
        )
    } else {
        let current: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM fact_vectors WHERE fact=? AND revision=? AND profile=?)",
        )
        .bind(row.get::<&str, _>("fact"))
        .bind(row.get::<i64, _>("fact_revision"))
        .bind(p.identity())
        .fetch_one(&mut *db)
        .await?;
        let observed: Value = serde_json::from_str(row.get("observed"))?;
        (
            if current {
                "current".into()
            } else {
                observed["state"].as_str().unwrap_or("unavailable").into()
            },
            Value::Null,
            false,
        )
    };
    let fact: &str = row.get("fact");
    let mut result = json!({"operation":c.params["operation"],"fact":fact,"revision":row.get::<i64,_>("fact_revision"),"profile":p.identity(),"logical":logical,"state":state,"usage":usage,"dispatch_now":dispatch});
    if c.params["retain"] == true {
        result["erasure_refs"] =
            json!(super::fact_sources::retain_quality(db, &c.plugin, fact).await?);
    }
    Ok(result)
}
pub(super) async fn profile(store: &Store, plugin: &str, logical: &str) -> Result<Profile> {
    let (body,route):(String,String)=sqlx::query_as("SELECT p.body,e.route FROM maintenance_embeddings e JOIN host_maintenance p ON p.plugin=e.plugin AND p.revision=e.revision WHERE e.plugin=? AND e.logical=?")
        .bind(plugin).bind(logical).fetch_one(store.readers()).await?;
    let p: Policy = serde_json::from_str(&body)?;
    p.embedding_routes
        .get(&route)
        .cloned()
        .ok_or_else(|| "original embedding route missing".into())
}
pub(crate) async fn authorize(db: &mut SqliteConnection, execution: &str) -> Result<()> {
    let rows=sqlx::query("SELECT e.* FROM maintenance_embeddings e JOIN embedding_requests j ON j.logical=e.logical WHERE j.execution=?")
        .bind(execution).fetch_all(&mut *db).await?;
    for row in rows {
        let plugin: String = row.get("plugin");
        let generation: String = sqlx::query_scalar(
            "SELECT generation FROM host_installations WHERE plugin=? AND online AND enabled",
        )
        .bind(&plugin)
        .fetch_one(&mut *db)
        .await?;
        super::super::domain::active(db, &plugin, &generation, "maintenance.reembed").await?;
        let c = Change {
            plugin,
            generation,
            method: "maintenance.reembed".into(),
            paid_request: None,
            params: json!({"scope":row.get::<String,_>("scope"),"purpose":row.get::<String,_>("purpose")}),
        };
        let g = scope::resolve(db, &c).await?;
        if g.revision != row.get::<String, _>("revision")
            || !g
                .policy
                .embedding_routes
                .contains(row.get::<&str, _>("route"))
        {
            return Err("original embedding grant changed".into());
        }
        memory::scope::writable(db, &g.reader, row.get("fact")).await?;
        let version: i64 = sqlx::query_scalar("SELECT revision FROM memory_facts WHERE id=?")
            .bind(row.get::<&str, _>("fact"))
            .fetch_one(&mut *db)
            .await?;
        if version != row.get::<i64, _>("fact_revision") {
            return Err("original embedding fact changed".into());
        }
    }
    Ok(())
}
