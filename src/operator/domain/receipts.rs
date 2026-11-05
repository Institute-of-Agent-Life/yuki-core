use super::*;

pub(super) async fn principal(db: &mut SqliteConnection, actor: &Actor) -> Result<String> {
    if let Some(id) = actor
        .namespace
        .strip_prefix("operator:")
        .or_else(|| actor.namespace.strip_prefix("person:"))
    {
        return Ok(id.into());
    }
    let key = format!("control-principal:{}", actor.namespace);
    if let Some(value) = get(db, &key).await? {
        return Ok(value.as_str().ok_or("invalid_control_principal")?.into());
    }
    let id = crate::identity::new_id(db).await?;
    put(db, &key, &json!(id)).await?;
    Ok(id)
}
pub(super) async fn read(db: &mut SqliteConnection, actor: &Actor, args: &Value) -> Result<Value> {
    let identity = args["request_id"].as_str().ok_or("validation_error")?;
    let parts: Vec<_> = identity.split(':').collect();
    let saved = if let ["control", principal, id] = parts.as_slice() {
        crate::host::personal::credentials::uuid(principal)?;
        crate::host::personal::credentials::uuid(id)?;
        let raw: String = sqlx::query_scalar("SELECT value FROM core_meta WHERE key LIKE 'control-receipt:%' AND json_extract(value,'$.principal_id')=? AND json_extract(value,'$.request_id')=?")
            .bind(principal.to_ascii_lowercase()).bind(id.to_ascii_lowercase()).fetch_optional(&mut *db).await?.ok_or("not_found")?;
        serde_json::from_str(&raw)?
    } else if parts.len() == 1 {
        crate::host::personal::credentials::uuid(identity)?;
        let id = identity.to_ascii_lowercase();
        if let Some(record) = get(db, &receipt_key(actor, &id)).await? {
            record
        } else {
            let key = format!(
                "model-profile-operation:{}",
                crate::memory::scope::hash(&json!([actor.namespace, id]))
            );
            let record = get(db, &key).await?.ok_or("not_found")?;
            let complete = record["plan"].is_null() && record["receipt"]["success"] == true;
            json!({"principal_id":principal(db, actor).await?,"request_id":id,
                "status":if complete {"succeeded"} else {"pending"},
                "result_resource_id":"model_profiles","result_revision":record["receipt"]["revision"],
                "created_at":null,"updated_at":null,"expected_revision":null})
        }
    } else {
        return Err("validation_error".into());
    };
    let done = matches!(saved["status"].as_str(), Some("succeeded" | "failed"));
    Ok(
        json!({"operation_id":format!("control:{}:{}", saved["principal_id"].as_str().ok_or("invalid_control_receipt")?, saved["request_id"].as_str().ok_or("invalid_control_receipt")?),
        "status":saved["status"],"progress":if done {json!(1.0)} else {Value::Null},"state_epoch":"v2",
        "error_category":saved["error_category"],"created_at":saved["created_at"],"updated_at":saved["updated_at"],
        "result_resource_id":saved["result_resource_id"],"result_revision":saved["result_revision"],"expected_revision":saved["expected_revision"]}),
    )
}

// Match the deterministic failures retained by the fixed v4 command executor.
fn cacheable(reason: &str) -> bool {
    matches!(
        reason,
        "validation_error"
            | "version_conflict"
            | "not_found"
            | "precondition_failed"
            | "binding_ambiguous"
            | "route_ambiguous"
            | "populated_merge_forbidden"
            | "secret_not_readable"
            | "operation_unavailable"
            | "configuration_unavailable"
    )
}

pub(crate) fn outcome(value: Value) -> Result<Value> {
    if let Some(reason) = value["__control_failure"].as_str() {
        return Err(reason.to_owned().into());
    }
    Ok(value)
}

pub(crate) async fn begin(
    db: &mut SqliteConnection,
    actor: &Actor,
    method: &str,
    body: &Value,
) -> Result<Option<Value>> {
    if let Some(old) = get(db, &receipt_key(actor, &operation(body)?)).await? {
        if old["method"] != method {
            return Err("request_conflict".into());
        }
        if old["status"] == "failed" {
            previous(db, actor, method, body).await?;
            let reason = old["error_category"].as_str().ok_or("state_mismatch")?;
            return Ok(Some(json!({"__control_failure":reason})));
        }
        // Accepted operations validate their original representation in the owning
        // service, including retained model-secret records from before CORE-037.
    }
    sqlx::query("SAVEPOINT control_command").execute(db).await?;
    Ok(None)
}

pub(crate) async fn finish(
    db: &mut SqliteConnection,
    actor: &Actor,
    method: &str,
    body: &Value,
    result: Result<Value>,
) -> Result<Value> {
    if let Err(error) = &result
        && cacheable(&error.to_string())
    {
        sqlx::query("ROLLBACK TO control_command")
            .execute(&mut *db)
            .await?;
        if get(db, &receipt_key(actor, &operation(body)?))
            .await?
            .is_some()
        {
            // A later refusal must not replace an already accepted operation.
            sqlx::query("RELEASE control_command").execute(db).await?;
            return result;
        }
        let reason = error.to_string();
        let target = body
            .get("id")
            .or_else(|| body.get("plugin_id"))
            .cloned()
            .unwrap_or_else(|| {
                json!(if method.starts_with("model.") {
                    "model_profiles"
                } else {
                    "yuki"
                })
            });
        record(
            db,
            actor,
            method,
            body,
            &json!({"resource_id":target,"revision":null}),
        )
        .await?;
        let key = receipt_key(actor, &operation(body)?);
        let mut saved = get(db, &key).await?.ok_or("state_mismatch")?;
        saved["status"] = json!("failed");
        saved["error_category"] = json!(reason);
        put(db, &key, &saved).await?;
        sqlx::query("RELEASE control_command").execute(db).await?;
        return Ok(json!({"__control_failure":reason}));
    }
    sqlx::query("RELEASE control_command").execute(db).await?;
    result
}
