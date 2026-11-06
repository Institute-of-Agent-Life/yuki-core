use super::{Request, Result, Scope, scope, strip};
use crate::runtime::effects;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::collections::BTreeSet;

fn take_rows(result: &mut Value) -> Vec<Value> {
    let mut rows = vec![];
    for field in ["events", "context"] {
        for row in result[field].as_array_mut().into_iter().flatten() {
            rows.push(row.clone());
            *row = row["event_id"].clone();
        }
    }
    if let Some(anchor) = result.get_mut("receipt_anchor")
        && let Some(row) = anchor.get_mut("event")
    {
        rows.push(row.clone());
        *row = row["event_id"].clone();
    }
    rows
}
pub(crate) async fn anchor(
    db: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    key: &str,
    args: &str,
    result: &Value,
) -> Result<Value> {
    let s = scope(db, exec, actor).await?;
    if let Some(old) = sqlx::query_scalar::<_, String>(
        "SELECT args FROM history_public_anchors WHERE execution=? AND operation=?",
    )
    .bind(exec)
    .bind(key)
    .fetch_optional(&mut *db)
    .await?
    {
        if old != args {
            return Err("original history operation argument conflict".into());
        }
        return Ok(json!({"operation":key}));
    }
    if let Some(old) = sqlx::query(
        "SELECT action,target,payload,state FROM operations WHERE namespace=? AND operation_id=?",
    )
    .bind(exec)
    .bind(key)
    .fetch_optional(&mut *db)
    .await?
    {
        let parsed = Request::parse(old.get("payload"))?;
        if old.get::<&str, _>("action") != "external"
            || old.get::<&str, _>("target") != "query_chat_history"
            || old.get::<&str, _>("state") != "intent"
            || serde_json::to_string(&parsed)? != args
        {
            return Err("history receipt conflicts with original operation".into());
        }
    } else {
        effects::apply(
            db,
            &effects::Action::Intent {
                execution: exec.into(),
                id: key.into(),
                actor: actor.into(),
                action: effects::Operation::External,
                target: "query_chat_history".into(),
                payload: args.into(),
                parent: None,
                ordinal: None,
            },
        )
        .await?;
    }
    let mut blueprint = result.clone();
    let selected = take_rows(&mut blueprint);
    strip(&mut blueprint);
    sqlx::query("INSERT INTO history_public_anchors VALUES(?,?,?,?)")
        .bind(exec)
        .bind(key)
        .bind(args)
        .bind(blueprint.to_string())
        .execute(&mut *db)
        .await?;
    let allowed = serde_json::to_string(&s.entries)?;
    let mut seen = BTreeSet::new();
    for mut row in selected {
        let event = row["event_id"].as_i64().ok_or("missing canonical event")?;
        if !seen.insert(event) {
            continue;
        }
        let item = row["_item"]
            .as_i64()
            .ok_or("missing original history slot")?;
        let visible:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM history_public_events WHERE event=? AND item=? AND entry IN(SELECT value FROM json_each(?)))").bind(event).bind(item).bind(&allowed).fetch_one(&mut *db).await?;
        strip(&mut row);
        sqlx::query("INSERT INTO history_public_rows VALUES(?,?,?,?,?)")
            .bind(exec)
            .bind(key)
            .bind(event)
            .bind(item)
            .bind(visible.then(|| row.to_string()))
            .execute(&mut *db)
            .await?;
    }
    // Bodies live only in scrubbed per-source rows, never in the generic effect receipt.
    let receipt = json!({"operation":key,"events":seen});
    sqlx::query("UPDATE operations SET state='confirmed',result=? WHERE namespace=? AND operation_id=? AND state='intent'").bind(receipt.to_string()).bind(exec).bind(key).execute(&mut *db).await?;
    Ok(receipt)
}

async fn row(
    db: &mut SqliteConnection,
    exec: &str,
    key: &str,
    event: i64,
    allowed: &str,
) -> Result<Value> {
    let body:Option<String>=sqlx::query_scalar("SELECT snap.body FROM history_public_rows snap JOIN history_public_events live ON live.item=snap.item AND live.event=snap.event WHERE snap.execution=? AND snap.operation=? AND snap.event=? AND live.entry IN(SELECT value FROM json_each(?))").bind(exec).bind(key).bind(event).bind(allowed).fetch_optional(&mut *db).await?.flatten();
    Ok(match body {
        Some(body) => serde_json::from_str(&body)?,
        None => json!({"id":event,"event_id":event,"status":"missing"}),
    })
}
pub(super) async fn replay(
    db: &mut SqliteConnection,
    exec: &str,
    key: &str,
    args: &str,
    s: &Scope,
) -> Result<Option<Value>> {
    let Some(old) = sqlx::query(
        "SELECT args,blueprint FROM history_public_anchors WHERE execution=? AND operation=?",
    )
    .bind(exec)
    .bind(key)
    .fetch_optional(&mut *db)
    .await?
    else {
        return Ok(None);
    };
    if old.get::<&str, _>("args") != args {
        return Err("original history operation argument conflict".into());
    }
    let mut result: Value = serde_json::from_str(old.get("blueprint"))?;
    let allowed = serde_json::to_string(&s.entries)?;
    let mut visible = BTreeSet::new();
    let mut entries = BTreeSet::new();
    for field in ["events", "context"] {
        for item in result[field].as_array_mut().into_iter().flatten() {
            let id = item.as_i64().ok_or("invalid original event slot")?;
            *item = row(db, exec, key, id, &allowed).await?;
            if let Some(entry) = item["conversation_id"].as_str() {
                visible.insert(id);
                entries.insert(entry.to_owned());
            }
        }
    }
    if let Some(anchor) = result.get_mut("receipt_anchor")
        && let Some(item) = anchor.get_mut("event")
    {
        let id = item.as_i64().ok_or("invalid original receipt slot")?;
        *item = row(db, exec, key, id, &allowed).await?;
        if let Some(entry) = item["conversation_id"].as_str() {
            visible.insert(id);
            entries.insert(entry.to_owned());
        }
    }
    if let Some(fields) = result["matched_fields"].as_object_mut() {
        fields.retain(|id, _| id.parse::<i64>().is_ok_and(|id| visible.contains(&id)));
    }
    if let Some(sources) = result["sources"].as_array_mut() {
        sources.retain(|v| {
            v["conversation_id"]
                .as_str()
                .is_some_and(|id| entries.contains(id) && s.entries.iter().any(|e| e == id))
        });
    }
    if result["conversation_id"]
        .as_str()
        .is_some_and(|id| !s.entries.iter().any(|e| e == id))
    {
        result["conversation_id"] = Value::Null;
        for field in ["platform", "binding_id", "target_id", "generation", "kind"] {
            result.as_object_mut().unwrap().remove(field);
        }
    }
    if let Some(center) = result["center_event_id"].as_i64()
        && !visible.contains(&center)
    {
        result["center_status"] = json!("unavailable");
        result["anchor_status"] = json!("missing");
    }
    if let Some(center) = result["receipt_anchor"]["event_id"].as_i64()
        && !visible.contains(&center)
    {
        result["receipt_anchor"]["status"] = json!("missing");
        result["anchor_status"] = json!("missing");
    }
    Ok(Some(result))
}
pub(crate) async fn material(
    db: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    key: &str,
) -> Result<Value> {
    let s = scope(db, exec, actor).await?;
    let args: String = sqlx::query_scalar(
        "SELECT args FROM history_public_anchors WHERE execution=? AND operation=?",
    )
    .bind(exec)
    .bind(key)
    .fetch_one(&mut *db)
    .await?;
    let result = replay(db, exec, key, &args, &s)
        .await?
        .ok_or("missing original history receipt")?;
    let allowed = serde_json::to_string(&s.entries)?;
    sqlx::query("INSERT OR IGNORE INTO main_dependencies(execution,event) SELECT ?1,live.event FROM history_public_rows snap JOIN history_public_events live ON live.item=snap.item AND live.event=snap.event WHERE snap.execution=?1 AND snap.operation=?2 AND snap.body IS NOT NULL AND live.entry IN(SELECT value FROM json_each(?3)) UNION SELECT ?1,x.source FROM history_public_rows snap JOIN history_public_events live ON live.item=snap.item AND live.event=snap.event JOIN social_outbound o ON o.id=live.outbound JOIN executions x ON x.id=o.namespace WHERE snap.execution=?1 AND snap.operation=?2 AND snap.body IS NOT NULL AND x.source IS NOT NULL AND live.entry IN(SELECT value FROM json_each(?3)) UNION SELECT ?1,d.event FROM history_public_rows snap JOIN history_public_events live ON live.item=snap.item AND live.event=snap.event JOIN social_outbound o ON o.id=live.outbound JOIN main_dependencies d ON d.execution=o.namespace WHERE snap.execution=?1 AND snap.operation=?2 AND snap.body IS NOT NULL AND live.entry IN(SELECT value FROM json_each(?3))").bind(exec).bind(key).bind(allowed).execute(&mut *db).await?;
    Ok(json!({"namespace":exec,"id":key,"state":"confirmed","result":result}))
}
