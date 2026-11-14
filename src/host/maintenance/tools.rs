use super::{Change, fact_sources::Dependencies, scope::Granted};
use crate::{Result, memory, privacy::source};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::collections::BTreeSet;

pub(super) struct Receipt {
    pub value: Value,
    pub sources: Vec<i64>,
}

pub(super) async fn sources(
    db: &mut SqliteConnection,
    execution: &str,
    value_source: Option<i64>,
) -> Result<Vec<i64>> {
    Ok(sqlx::query_scalar("SELECT x.source FROM maintenance_execution_ancestry a JOIN executions x ON x.id=a.member WHERE a.execution=?1 AND x.source IS NOT NULL UNION SELECT d.event FROM maintenance_execution_ancestry a JOIN main_dependencies d ON d.execution=a.member WHERE a.execution=?1 UNION SELECT ?2 WHERE ?2 IS NOT NULL ORDER BY 1")
        .bind(execution).bind(value_source).fetch_all(db).await?)
}

pub(super) async fn retain(
    db: &mut SqliteConnection,
    plugin: &str,
    receipt: &Receipt,
) -> Result<()> {
    let item = format!("maintenance-tool:{}", receipt.value["receipt"]);
    for &event in &receipt.sources {
        source::acquire(db, plugin, &item, event).await?;
    }
    Ok(())
}

async fn available(
    db: &mut SqliteConnection,
    g: &Granted,
    id: Option<i64>,
    execution: Option<&str>,
    event: Option<i64>,
    after: i64,
) -> Result<Vec<Receipt>> {
    if !g.policy.methods.contains("maintenance.tool.page")
        || !g.policy.methods.contains("maintenance.evidence")
    {
        return Err("tool evidence requires both installed receipt and evidence grants".into());
    }
    // A local get may retain an earlier event's value. Check that separate
    // source before returning even an ID or preview from the requested entry.
    let rows = sqlx::query(
        "SELECT o.rowid AS receipt,o.namespace,o.operation_id,o.target,o.result,o.value_source,a.actor,a.entry
         FROM operations o JOIN maintenance_execution_access a ON a.execution=o.namespace
         JOIN executions x ON x.id=o.namespace
         WHERE a.entry=?1 AND o.rowid>?2 AND (?3 IS NULL OR o.namespace=?3)
           AND (?4 IS NULL OR o.rowid=?4) AND o.state='confirmed' AND o.result IS NOT NULL
           AND (?5 IS NULL OR EXISTS(
             SELECT 1 FROM maintenance_execution_ancestry ancestry
             JOIN executions original ON original.id=ancestry.member
             JOIN events trigger_event ON trigger_event.id=original.source
             WHERE ancestry.execution=o.namespace AND trigger_event.id=?5 AND trigger_event.entry_id=a.entry))
           AND (o.action!='get' OR o.value_source IS NOT NULL OR x.origin IS NOT NULL OR o.result='null')
           AND (o.value_source IS NULL OR EXISTS(
             SELECT 1 FROM events e JOIN persons p ON p.id=e.raw_actor
             JOIN persons root ON root.id=coalesce(p.current_root,p.id)
             WHERE e.id=o.value_source AND e.entry_id=a.entry AND p.enabled AND root.enabled
               AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)))
         ORDER BY o.rowid LIMIT 8",
    )
    .bind(&g.reader.base.home)
    .bind(after)
    .bind(execution)
    .bind(id)
    .bind(event)
    .fetch_all(&mut *db)
    .await?;
    let mut receipts = Vec::with_capacity(rows.len());
    for row in rows {
        let execution: String = row.get("namespace");
        let dependencies = sources(db, &execution, row.get("value_source")).await?;
        let result: Value = serde_json::from_str(row.get("result"))?;
        let value = json!({
            "receipt":row.get::<i64,_>("receipt"),"execution":execution,
            "operation":row.get::<String,_>("operation_id"),"tool":row.get::<String,_>("target"),
            "actor":row.get::<String,_>("actor"),"entry":row.get::<String,_>("entry"),
            "result":result,"dependencies_sha256":memory::scope::hash(&json!(dependencies)),
            "source_count":dependencies.len(),"source_ids":dependencies.iter().take(128).collect::<Vec<_>>(),
            "sources_complete":dependencies.len()<=128
        });
        receipts.push(Receipt {
            value,
            sources: dependencies,
        });
    }
    Ok(receipts)
}

pub(super) async fn full(db: &mut SqliteConnection, g: &Granted, id: i64) -> Result<Receipt> {
    available(db, g, Some(id), None, None, 0)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| "original tool receipt unavailable".into())
}

pub(super) async fn page(db: &mut SqliteConnection, c: &Change, g: &Granted) -> Result<Value> {
    let after = c.params["after"].as_i64().unwrap_or(0);
    let event = c
        .params
        .get("event")
        .map(|id| id.as_i64().ok_or("original event ID out of range"))
        .transpose()?;
    let receipts = available(db, g, None, c.params["execution"].as_str(), event, after).await?;
    let mut next = after;
    let mut items = vec![];
    for receipt in receipts {
        retain(db, &c.plugin, &receipt).await?;
        let value = receipt.value;
        let id = value["receipt"]
            .as_i64()
            .ok_or("original receipt required")?;
        next = id;
        let bytes = serde_json::to_vec(&value)?;
        let descriptor = super::content::descriptor("tool", &id.to_string(), &bytes);
        let preview = value["result"].to_string();
        items.push(json!({"receipt":id,"execution":value["execution"],"operation":value["operation"],"tool":value["tool"],"actor":value["actor"],"entry":value["entry"],"version":memory::scope::hash(&value),"preview":preview.chars().take(512).collect::<String>(),"complete":preview.chars().count()<=512,"content":descriptor,"source_count":value["source_count"],"source_ids":value["source_ids"],"sources_complete":value["sources_complete"]}));
    }
    Ok(json!({"items":items,"next":next}))
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
        let receipt = full(db, g, id).await?;
        if !seen.insert(id) || reference["version"] != memory::scope::hash(&receipt.value) {
            return Err("original tool receipt changed or repeated".into());
        }
        if reference["role"] == "primary" {
            primary.extend(receipt.sources.iter().copied());
        }
        all.extend(receipt.sources);
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
