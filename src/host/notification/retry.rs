//! A manual retry resumes only an original effect proved never dispatched.
use crate::{Result, runtime::effects};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

async fn untouched(db: &mut SqliteConnection, execution: &str, operation: &str) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM operations o WHERE o.namespace=?1 AND o.operation_id=?2
         AND o.state='intent' AND NOT EXISTS(SELECT 1 FROM operations c WHERE c.namespace=?1 AND c.parent=?2)
         AND NOT EXISTS(SELECT 1 FROM host_calls c WHERE c.execution=?1 AND c.operation=?2)
         AND NOT EXISTS(SELECT 1 FROM social_plans s WHERE s.namespace=?1 AND s.operation_id=?2))",
    ).bind(execution).bind(operation).fetch_one(db).await?)
}

pub(super) async fn refused(
    db: &mut SqliteConnection,
    publication: &str,
    ordinal: i64,
    category: &str,
) -> Result<Value> {
    if !matches!(category, "bot_unavailable" | "gateway_disconnected") {
        return Err("invalid predispatch refusal".into());
    }
    let row = sqlx::query("SELECT execution,operation,state FROM notification_deliveries WHERE publication=? AND ordinal=?")
        .bind(publication).bind(ordinal).fetch_one(&mut *db).await?;
    if row.get::<&str, _>("state") != "pending"
        || !untouched(db, row.get("execution"), row.get("operation")).await?
    {
        return Ok(Value::Null);
    }
    sqlx::query(
        "UPDATE notification_outbox_control SET error_category=? WHERE publication=? AND ordinal=?",
    )
    .bind(category)
    .bind(publication)
    .bind(ordinal)
    .execute(&mut *db)
    .await?;
    super::writer::delivered(db, publication, ordinal, "failed").await
}

pub(crate) async fn read(db: &mut SqliteConnection, id: i64) -> Result<Value> {
    let row = sqlx::query("SELECT c.*,d.state,d.execution,d.operation,p.plugin,p.origin FROM notification_outbox_control c
        JOIN notification_deliveries d USING(publication,ordinal) JOIN notification_publications p ON p.id=c.publication WHERE c.id=?")
        .bind(id).fetch_optional(&mut *db).await?.ok_or("not_found")?;
    authorize(db, row.get("execution"), row.get("origin")).await?;
    Ok(
        json!({"id":id,"notification_id":row.get::<&str,_>("publication"),"ordinal":row.get::<i64,_>("ordinal"),
        "plugin_id":row.get::<&str,_>("plugin"),"revision":row.get::<i64,_>("revision"),"state":row.get::<&str,_>("state"),
        "attempts":row.get::<i64,_>("attempts"),"max_attempts":row.get::<i64,_>("max_attempts"),"error_category":row.get::<Option<&str>,_>("error_category")}),
    )
}

async fn authorize(db: &mut SqliteConnection, execution: &str, origin: &str) -> Result<()> {
    effects::authorize(db, execution, origin, true)
        .await
        .map_err(|_| "state_mismatch")?;
    super::transport::authorize(db, execution)
        .await
        .map_err(|_| "state_mismatch".into())
}

pub(crate) async fn retry(db: &mut SqliteConnection, id: i64, revision: i64) -> Result<Value> {
    let row = sqlx::query("SELECT c.*,d.state,d.execution,d.operation,p.origin FROM notification_outbox_control c
        JOIN notification_deliveries d USING(publication,ordinal) JOIN notification_publications p ON p.id=c.publication WHERE c.id=?")
        .bind(id).fetch_optional(&mut *db).await?.ok_or("not_found")?;
    if row.get::<i64, _>("revision") != revision {
        return Err("version_conflict".into());
    }
    if row.get::<&str, _>("state") != "failed"
        || row.get::<i64, _>("attempts") >= row.get::<i64, _>("max_attempts")
        || !matches!(
            row.get::<Option<&str>, _>("error_category"),
            Some("bot_unavailable" | "gateway_disconnected" | "effect_gate_timeout")
        )
        || !untouched(db, row.get("execution"), row.get("operation")).await?
    {
        return Err("precondition_failed".into());
    }
    authorize(db, row.get("execution"), row.get("origin")).await?;
    sqlx::query(
        "UPDATE notification_deliveries SET state='pending' WHERE publication=? AND ordinal=?",
    )
    .bind(row.get::<&str, _>("publication"))
    .bind(row.get::<i64, _>("ordinal"))
    .execute(&mut *db)
    .await?;
    sqlx::query("UPDATE notification_outbox_control SET error_category='manual_retry' WHERE id=?")
        .bind(id)
        .execute(&mut *db)
        .await?;
    sqlx::query("UPDATE executions SET state='running' WHERE id=? AND state='held'")
        .bind(row.get::<&str, _>("execution"))
        .execute(db)
        .await?;
    Ok(
        json!({"resource_id":id.to_string(),"revision":revision+1,"effective_state":{"status":"pending"}}),
    )
}
