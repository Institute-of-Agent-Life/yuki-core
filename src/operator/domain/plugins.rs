use super::*;

pub(super) async fn discovery_request(
    db: &mut SqliteConnection,
    actor: &Actor,
    body: &Value,
) -> Result<Value> {
    let mut dto = body.clone();
    dto.as_object_mut()
        .ok_or("validation_error")?
        .remove("plugin_id");
    validate("CommandBody", &dto)?;
    Ok(previous(db, actor, "plugin.discover", body)
        .await?
        .map(|v| {
            if v["status"] == "failed" {
                json!({"__control_failure":v["error_category"]})
            } else {
                v["result"].clone()
            }
        })
        .unwrap_or(json!({"new":true})))
}

pub(super) async fn discover(
    db: &mut SqliteConnection,
    actor: &Actor,
    body: &Value,
    installs: &[crate::host::Install],
) -> Result<Value> {
    let mut dto = body.clone();
    dto.as_object_mut()
        .ok_or("validation_error")?
        .remove("plugin_id");
    validate("CommandBody", &dto)?;
    if let Some(old) = previous(db, actor, "plugin.discover", body).await? {
        return Ok(old["result"].clone());
    }
    if body["expected_revision"] != 0 {
        return Err("version_conflict".into());
    }
    let inventory = crate::host::lifecycle::discover(db, installs).await?;
    let result = json!({"prepared":true,"inventory":inventory,"resource_id":"yuki","revision":1});
    record(db, actor, "plugin.discover", body, &result).await?;
    let key = receipt_key(actor, &operation(body)?);
    let mut saved = get(db, &key).await?.ok_or("not_found")?;
    saved["status"] = json!("pending");
    put(db, &key, &saved).await?;
    Ok(result)
}
pub(super) async fn discovered(
    db: &mut SqliteConnection,
    actor: &Actor,
    body: &Value,
) -> Result<Value> {
    let old = previous(db, actor, "plugin.discover", body)
        .await?
        .ok_or("state_mismatch")?;
    if old["status"] == "succeeded" {
        return Ok(old["result"].clone());
    }
    if old["result"]["inventory"] != crate::host::lifecycle::inventory(db).await? {
        return Err("state_mismatch".into());
    }
    let result =
        json!({"resource_id":"yuki","revision":1,"effective_state":{"status":"discovered"}});
    record(db, actor, "plugin.discover", body, &result).await?;
    Ok(result)
}

pub(super) async fn prepare(
    db: &mut SqliteConnection,
    actor: &Actor,
    method: &str,
    body: &Value,
) -> Result<Value> {
    let mut dto = body.clone();
    dto.as_object_mut()
        .ok_or("validation_error")?
        .remove("plugin_id");
    validate("PluginBody", &dto)?;
    if method != "plugin.approve" && body.get("permissions").is_some() {
        return Err("validation_error".into());
    }
    if let Some(old) = previous(db, actor, method, body).await? {
        if old["status"] == "failed" {
            return Ok(json!({"error_category":old["error_category"]}));
        }
        return Ok(old["result"].clone());
    }
    let state = crate::host::lifecycle::prepare(db, method, body).await?;
    if method == "plugin.doctor" {
        let value = crate::host::lifecycle::result(
            db,
            method,
            body["plugin_id"].as_str().ok_or("validation_error")?,
        )
        .await?;
        record(db, actor, method, body, &value).await?;
        return Ok(value);
    }
    let value = json!({"prepared":true,"request_id":operation(body)?,"plugin_id":body["plugin_id"],"revision":state["revision"],"enabled":state["enabled"],"approved":state["approved_permissions"],"manifest":state["manifest_hash"]});
    record(db, actor, method, body, &value).await?;
    let key = receipt_key(actor, &operation(body)?);
    let mut saved = get(db, &key).await?.ok_or("not_found")?;
    saved["status"] = json!("pending");
    put(db, &key, &saved).await?;
    Ok(value)
}

pub(super) async fn settle(
    db: &mut SqliteConnection,
    actor: &Actor,
    method: &str,
    body: &Value,
    failed: bool,
) -> Result<Value> {
    let old = previous(db, actor, method, body)
        .await?
        .ok_or("state_mismatch")?;
    if old["status"] != "pending" {
        return Ok(if old["status"] == "failed" {
            json!({"error_category":old["error_category"]})
        } else {
            old["result"].clone()
        });
    }
    let id = body["plugin_id"].as_str().ok_or("validation_error")?;
    let state = crate::host::configuration::status(db, &json!({"plugin_id":id})).await?;
    let planned = &old["result"];
    let changed = planned["enabled"] != state["enabled"]
        || planned["approved"] != state["approved_permissions"]
        || planned["manifest"] != state["manifest_hash"];
    let failed = failed || changed || (method == "plugin.enable" && state["status"] != "running");
    let value = crate::host::lifecycle::result(db, method, id).await?;
    record(db, actor, method, body, &value).await?;
    if failed {
        let key = receipt_key(actor, &operation(body)?);
        let mut saved = get(db, &key).await?.ok_or("not_found")?;
        saved["status"] = json!("failed");
        saved["error_category"] = json!(if changed {
            "state_mismatch"
        } else {
            "precondition_failed"
        });
        put(db, &key, &saved).await?;
        return Ok(json!({"error_category":saved["error_category"]}));
    }
    Ok(value)
}

pub(super) fn outbox_id(args: &Value) -> Result<i64> {
    let id: i64 = args["id"]
        .as_str()
        .ok_or("validation_error")?
        .parse()
        .map_err(|_| "validation_error")?;
    if id <= 0 {
        return Err("validation_error".into());
    }
    Ok(id)
}
pub(super) async fn retry_outbox(
    db: &mut SqliteConnection,
    actor: &Actor,
    body: &Value,
) -> Result<Value> {
    identity::dto(body, "CommandBody")?;
    let id = outbox_id(body)?;
    if let Some(old) = previous(db, actor, "plugin.outbox.retry", body).await? {
        // An old success cannot disclose a notification whose original authority is gone.
        crate::host::notification::retry::read(db, id).await?;
        return Ok(old["result"].clone());
    }
    let result = crate::host::notification::retry::retry(
        db,
        id,
        body["expected_revision"]
            .as_i64()
            .ok_or("validation_error")?,
    )
    .await?;
    record(db, actor, "plugin.outbox.retry", body, &result).await?;
    Ok(result)
}
