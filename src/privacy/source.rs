use crate::Result;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub async fn is_erased(conn: &mut SqliteConnection, source: i64) -> Result<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=?)")
            .bind(source)
            .fetch_one(conn)
            .await?,
    )
}

pub async fn execution_erased(
    conn: &mut SqliteConnection,
    exec: &str,
    source: Option<i64>,
) -> Result<bool> {
    if let Some(source) = source
        && is_erased(conn, source).await?
    {
        return Ok(true);
    }
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM main_dependencies d JOIN invalidated_sources i ON i.source=d.event WHERE d.execution=?1) OR EXISTS(SELECT 1 FROM plugin_session_redactions WHERE execution=?1)")
      .bind(exec).fetch_one(conn).await?)
}
async fn check(conn: &mut SqliteConnection, owner: &str, source: i64) -> Result<()> {
    let allowed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM source_owners o JOIN events e ON e.id=?
         LEFT JOIN persons p ON p.id=e.raw_actor LEFT JOIN persons r ON r.id=coalesce(p.current_root,p.id)
         WHERE o.id=? AND o.enabled AND ((p.enabled AND r.enabled) OR
           (e.raw_actor IS NULL AND EXISTS(SELECT 1 FROM notification_publications n
            JOIN notification_target_scope grant ON grant.plugin=n.plugin AND grant.entry=n.entry AND grant.receiver=n.receiver
            JOIN entries entry ON entry.id=n.entry AND entry.closed_at IS NULL
            JOIN assignments receiver ON receiver.id=n.receiver AND receiver.status='active'
            LEFT JOIN persons owner ON owner.id=n.target_person LEFT JOIN persons root ON root.id=coalesce(owner.current_root,owner.id)
            WHERE n.event=e.id AND n.state='published'
            AND n.window=coalesce((SELECT generation FROM main_windows WHERE conversation=n.entry),1)
            AND (n.target_person IS NULL OR (owner.enabled AND root.enabled)))))
           AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)
           AND NOT EXISTS(SELECT 1 FROM owner_erasures WHERE owner=o.id AND proof IS NULL))",
    )
    .bind(source)
    .bind(owner)
    .fetch_one(conn)
    .await?;
    if !allowed {
        return Err("owner, source or erasure watermark prevents publication".into());
    }
    Ok(())
}

pub async fn read(
    conn: &mut SqliteConnection,
    owner: &str,
    item: &str,
    source: i64,
) -> Result<Value> {
    check(conn, owner, source).await?;
    let row = sqlx::query(
        "SELECT e.envelope FROM source_holds h JOIN events e ON e.id=h.source
         WHERE h.owner=? AND h.item=? AND h.source=?",
    )
    .bind(owner)
    .bind(item)
    .bind(source)
    .fetch_one(conn)
    .await?;
    Ok(
        json!({"source":source,"ref":item,"envelope":serde_json::from_str::<Value>(row.try_get("envelope")?)?}),
    )
}

pub async fn acquire(
    conn: &mut SqliteConnection,
    owner: &str,
    item: &str,
    source: i64,
) -> Result<Value> {
    // Commit the holder association before returning any source material to the retained owner.
    check(conn, owner, source).await?;
    sqlx::query("INSERT INTO source_holds(owner,item,source) VALUES(?,?,?) ON CONFLICT DO NOTHING")
        .bind(owner)
        .bind(item)
        .bind(source)
        .execute(&mut *conn)
        .await?;
    read(conn, owner, item, source).await
}

pub fn accounting_only(raw: &str) -> String {
    let usage: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
    if !usage.is_object() {
        return "null".into();
    }
    let mut safe = serde_json::Map::new();
    for field in [
        "input",
        "cache",
        "output",
        "reasoning",
        "total",
        "billed_microusd",
        "uncached_input",
        "cache_creation",
        "cache_creation_5m",
        "cache_creation_1h",
        "server_web_search_requests",
    ] {
        if let Some(count) = usage[field].as_i64().filter(|n| *n >= 0) {
            safe.insert(field.into(), json!(count));
        }
    }
    if !safe.contains_key("input")
        && let Some(input) = usage["input_tokens"].as_i64().filter(|n| *n >= 0)
    {
        safe.insert("input".into(), json!(input));
    }
    Value::Object(safe).to_string()
}

/// Reconcile a physical attempt even after the source fence. Never persist its private body.
pub async fn late_attempt(
    conn: &mut SqliteConnection,
    attempt: &str,
    status: i64,
    usage: &str,
) -> Result<Option<Value>> {
    let row=sqlx::query("SELECT t.state,t.status_code,t.usage_json,
        EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.source OR source IN(SELECT event FROM main_dependencies WHERE execution=e.id)) OR EXISTS(SELECT 1 FROM plugin_session_redactions WHERE execution=e.id) AS revoked FROM model_attempts t
        JOIN model_requests r ON r.id=t.logical JOIN activations a ON a.id=r.activation
        JOIN executions e ON e.id=a.execution WHERE t.id=?").bind(attempt).fetch_one(&mut *conn).await?;
    if !row.try_get::<bool, _>("revoked")? {
        return Ok(None);
    }
    let prior: String = row.try_get("state")?;
    let state = if matches!(prior.as_str(), "dispatched" | "unknown") {
        if status == 200 {
            "response_held"
        } else {
            "rejected"
        }
    } else {
        if row.try_get::<Option<i64>, _>("status_code")? != Some(status) {
            return Err("conflicting terminal attempt status".into());
        }
        &prior
    };
    let existing: Option<String> = row.try_get("usage_json")?;
    let known: Value =
        serde_json::from_str(&accounting_only(existing.as_deref().unwrap_or("null")))?;
    let mut fresh: Value = serde_json::from_str(&accounting_only(usage))?;
    if let Some(known) = known.as_object() {
        if !fresh.is_object() {
            fresh = json!({});
        }
        let merged = fresh.as_object_mut().unwrap();
        for (k, v) in known {
            if merged.get(k).is_some_and(|new| new != v) {
                return Err("conflicting original usage".into());
            }
            merged.insert(k.clone(), v.clone());
        }
    }
    sqlx::query("UPDATE model_attempts SET state=?,status_code=?,usage_json=?,response=NULL,partial_bytes=NULL,intents_json=NULL,retry_proof=0,
        diagnostic='source_erased',received_at=coalesce(received_at,CURRENT_TIMESTAMP) WHERE id=?")
        .bind(state).bind(status).bind(fresh.to_string()).bind(attempt).execute(conn).await?;
    Ok(Some(json!({"attempt":attempt,"state":state,"erased":true})))
}

pub async fn scrub(conn: &mut SqliteConnection, event: i64) -> Result<()> {
    crate::host::plugin_sessions::scrub_inactive(conn).await?;
    crate::social::send::erase(conn, event).await?;
    crate::history::query::scrub(conn, event).await?;
    sqlx::query("UPDATE events SET envelope='{}',mentions='[]' WHERE id=?")
        .bind(event)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE main_turns SET result=NULL,material=NULL,diagnostic='source_erased' WHERE execution IN(SELECT execution FROM main_dependencies WHERE event=?)")
        .bind(event).execute(&mut *conn).await?;
    sqlx::query("DELETE FROM internal_values WHERE execution IN(SELECT execution FROM main_dependencies WHERE event=?)").bind(event).execute(&mut *conn).await?;
    let rows=sqlx::query("SELECT t.id,t.usage_json FROM model_attempts t JOIN model_requests r ON r.id=t.logical
        JOIN activations a ON a.id=r.activation JOIN executions e ON e.id=a.execution WHERE e.source=?1 OR e.id IN(SELECT execution FROM main_dependencies WHERE event=?1)")
        .bind(event).fetch_all(&mut *conn).await?;
    for row in rows {
        let raw: Option<String> = row.try_get("usage_json")?;
        sqlx::query("UPDATE model_attempts SET response=NULL,partial_bytes=NULL,intents_json=NULL,retry_proof=0,usage_json=?, diagnostic='source_erased' WHERE id=?")
            .bind(raw.as_deref().map(accounting_only)).bind(row.try_get::<String,_>("id")?).execute(&mut *conn).await?;
    }
    sqlx::query(
        "UPDATE model_requests SET request_json='{}' WHERE activation IN
        (SELECT a.id FROM activations a JOIN executions e ON e.id=a.execution WHERE e.source=?1 OR e.id IN(SELECT execution FROM main_dependencies WHERE event=?1))",
    )
    .bind(event)
    .execute(&mut *conn)
    .await?;
    sqlx::query("UPDATE model_tool_intents SET paired_receipt='{\"erased\":true}' WHERE paired_receipt IS NOT NULL
        AND execution IN (SELECT id FROM executions WHERE source=?1 OR id IN(SELECT execution FROM main_dependencies WHERE event=?1))").bind(event).execute(&mut *conn).await?;
    // Snapshots taken by another execution carry their value's source as well as their own.
    // For a migrated read with ambiguous provenance, fence its actor's snapshot conservatively.
    sqlx::query("UPDATE operations SET result='{}' WHERE action='get' AND result IS NOT NULL AND
        (value_source=? OR (value_source IS NULL AND json_extract(result,'$.revision') IS NOT NULL
          AND namespace IN(SELECT x.id FROM executions x JOIN events e ON e.raw_actor=x.original_actor WHERE e.id=?)))")
        .bind(event).bind(event).execute(&mut *conn).await?;
    sqlx::query("UPDATE model_tool_intents SET paired_receipt='{}' WHERE paired_receipt IS NOT NULL AND
        EXISTS(SELECT 1 FROM operations o WHERE o.namespace=model_tool_intents.execution AND o.operation_id=model_tool_intents.operation_id
          AND o.action='get' AND o.result='{}')")
        .execute(&mut *conn).await?;
    sqlx::query("UPDATE operations SET payload='{}',target='erased',result=CASE WHEN result IS NULL THEN NULL ELSE '{\"erased\":true}' END
        WHERE namespace IN (SELECT id FROM executions WHERE source=?1 OR id IN(SELECT execution FROM main_dependencies WHERE event=?1))").bind(event).execute(&mut *conn).await?;
    sqlx::query("DELETE FROM local_effect_values WHERE source=?1 OR source_execution IN(SELECT execution FROM main_dependencies WHERE event=?1)")
        .bind(event)
        .execute(conn)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn legacy_embedding_usage_survives_source_erasure() {
        let actual =
            super::accounting_only(r#"{"input_tokens":9,"provider_private_text":"erased"}"#);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&actual).unwrap(),
            serde_json::json!({"input":9})
        );
    }
}
