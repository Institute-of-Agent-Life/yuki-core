use super::{Change, fact_sources::Dependencies, scope::Granted};
use crate::{Result, memory, privacy::source};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::collections::BTreeSet;

pub(super) async fn sources(db: &mut SqliteConnection, execution: &str) -> Result<Vec<i64>> {
    Ok(sqlx::query_scalar("SELECT x.source FROM maintenance_execution_ancestry a JOIN executions x ON x.id=a.member WHERE a.execution=?1 AND x.source IS NOT NULL UNION SELECT d.event FROM maintenance_execution_ancestry a JOIN main_dependencies d ON d.execution=a.member WHERE a.execution=?1 ORDER BY 1")
        .bind(execution).fetch_all(db).await?)
}
pub(super) async fn full(db: &mut SqliteConnection, g: &Granted, id: i64) -> Result<Value> {
    if !g.policy.methods.contains("maintenance.tool.page")
        || !g.policy.methods.contains("maintenance.evidence")
    {
        return Err("tool evidence requires both installed receipt and evidence grants".into());
    }
    let row=sqlx::query("SELECT o.rowid,o.namespace,o.operation_id,o.target,o.result,a.actor,a.entry FROM operations o JOIN maintenance_execution_access a ON a.execution=o.namespace WHERE o.rowid=? AND a.entry=? AND o.state='confirmed' AND o.result IS NOT NULL")
        .bind(id).bind(&g.reader.base.home).fetch_one(&mut *db).await?;
    let result: Value = serde_json::from_str(row.get("result"))?;
    let execution: String = row.get("namespace");
    let dependencies = sources(db, &execution).await?;
    Ok(
        json!({"receipt":id,"execution":execution,"operation":row.get::<String,_>("operation_id"),"tool":row.get::<String,_>("target"),"actor":row.get::<String,_>("actor"),"entry":row.get::<String,_>("entry"),"result":result,"dependencies_sha256":memory::scope::hash(&json!(dependencies)),"source_count":dependencies.len(),"source_ids":dependencies.iter().take(128).collect::<Vec<_>>(),"sources_complete":dependencies.len()<=128}),
    )
}
pub(super) async fn retain(db: &mut SqliteConnection, plugin: &str, value: &Value) -> Result<()> {
    for event in sources(
        db,
        value["execution"]
            .as_str()
            .ok_or("original execution required")?,
    )
    .await?
    {
        source::acquire(
            db,
            plugin,
            &format!("maintenance-tool:{}", value["receipt"]),
            event,
        )
        .await?;
    }
    Ok(())
}
pub(super) async fn page(db: &mut SqliteConnection, c: &Change, g: &Granted) -> Result<Value> {
    if !g.policy.methods.contains("maintenance.evidence") {
        return Err("tool receipts require explicit evidence permission".into());
    }
    let after = c.params["after"].as_i64().unwrap_or(0);
    let ids:Vec<i64>=sqlx::query_scalar("SELECT o.rowid FROM operations o JOIN maintenance_execution_access a ON a.execution=o.namespace WHERE a.entry=? AND o.rowid>? AND (? IS NULL OR o.namespace=?) AND o.state='confirmed' AND o.result IS NOT NULL ORDER BY o.rowid LIMIT 8")
        .bind(&g.reader.base.home).bind(after).bind(c.params["execution"].as_str()).bind(c.params["execution"].as_str()).fetch_all(&mut *db).await?;
    let mut items = vec![];
    for id in &ids {
        let value = full(db, g, *id).await?;
        retain(db, &c.plugin, &value).await?;
        let bytes = serde_json::to_vec(&value)?;
        let descriptor = super::content::descriptor("tool", &id.to_string(), &bytes);
        let preview = value["result"].to_string();
        items.push(json!({"receipt":id,"execution":value["execution"],"operation":value["operation"],"tool":value["tool"],"actor":value["actor"],"entry":value["entry"],"version":memory::scope::hash(&value),"preview":preview.chars().take(512).collect::<String>(),"complete":preview.chars().count()<=512,"content":descriptor,"source_count":value["source_count"],"source_ids":value["source_ids"],"sources_complete":value["sources_complete"]}));
    }
    Ok(json!({"items":items,"next":ids.last().copied().unwrap_or(after)}))
}
pub(super) async fn validate(
    db: &mut SqliteConnection,
    g: &Granted,
    refs: &Value,
) -> Result<Dependencies> {
    let mut all = BTreeSet::new();
    let mut primary = BTreeSet::new();
    let mut seen = BTreeSet::new();
    for reference in refs.as_array().into_iter().flatten() {
        let id = reference["receipt"]
            .as_i64()
            .ok_or("original receipt required")?;
        let value = full(db, g, id).await?;
        if !seen.insert(id) || reference["version"] != memory::scope::hash(&value) {
            return Err("original tool receipt changed or repeated".into());
        }
        let dependencies =
            sources(db, value["execution"].as_str().ok_or("execution required")?).await?;
        if reference["role"] == "primary" {
            primary.extend(dependencies.iter().copied());
        }
        all.extend(dependencies);
    }
    Ok(Dependencies { all, primary })
}
pub(super) async fn original(db: &mut SqliteConnection, g: &Granted, preview: &str) -> Result<()> {
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT receipt,version,role FROM maintenance_tool_inputs WHERE preview=? ORDER BY receipt",
    )
    .bind(preview)
    .fetch_all(&mut *db)
    .await?;
    let refs = rows
        .into_iter()
        .map(|(receipt, version, role)| json!({"receipt":receipt,"version":version,"role":role}))
        .collect::<Vec<_>>();
    validate(db, g, &json!(refs)).await?;
    Ok(())
}
