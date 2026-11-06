use super::*;
use crate::host::personal::credentials::{hash, now, stamp};

fn revision(content: &str) -> String {
    format!("sha256:{}", hash(content))
}
fn snapshot(service: &Service) -> Result<Value> {
    let file = service.root.snapshot("SOUL.md")?;
    let content = String::from_utf8(file.data).map_err(|_| "soul_document_invalid")?;
    Ok(json!({"revision":revision(&content),"content":content}))
}
pub(super) async fn start(db: &mut SqliteConnection, service: &Service) -> Result<Value> {
    if get(db, "control-soul-active-operation").await?.is_none() {
        if service.root.snapshot_optional("SOUL.md")?.is_none() {
            return Ok(json!({"status":"unavailable"}));
        }
        put(db, "character-soul-active", &snapshot(service)?).await?;
    } else if get(db, "character-soul-active").await?.is_none() {
        return Err("original_soul_unavailable".into());
    }
    Ok(json!({"status":"loaded"}))
}
pub(super) async fn read(db: &mut SqliteConnection, service: &Service) -> Result<Value> {
    let doc = snapshot(service)?;
    let active = get(db, "character-soul-active")
        .await?
        .ok_or("soul_unavailable")?;
    let writable = service.root.writable()?;
    // The modified timestamp comes from the same pinned directory, not an arbitrary path.
    let modified = service
        .root
        .modified("SOUL.md")?
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let value = json!({"file_name":"SOUL.md","content":doc["content"],"revision":doc["revision"],
        "active_revision":active["revision"],"updated_at":stamp(i64::try_from(modified)?)?,
        "updated_by":if active["revision"]==doc["revision"] {active["updated_by"].as_str().unwrap_or("external")} else {"external"},
        "writable":writable,"reason":if writable {Value::Null} else {json!("directory_not_writable")}});
    validate("SoulDocument", &value)?;
    Ok(value)
}
pub(super) async fn prepare(
    db: &mut SqliteConnection,
    service: &Service,
    actor: &Actor,
    body: &Value,
) -> Result<Value> {
    validate("SoulSaveBody", body)?;
    let id = operation(body)?;
    if let Some(old) = previous(db, actor, "character.soul.save", body).await? {
        return Ok(if old["status"] == "succeeded" {
            old["result"].clone()
        } else {
            json!({"prepared":true,"request_id":id})
        });
    }
    if get(db, "control-soul-active-operation").await?.is_some() {
        return Err("version_conflict".into());
    }
    if !service.root.writable()? {
        return Err("configuration_not_writable".into());
    }
    let before = snapshot(service)?;
    if before["revision"] != body["expected_revision"] {
        return Err("version_conflict".into());
    }
    let content = body["content"].as_str().ok_or("validation_error")?;
    if content.len() > 1024 * 1024 {
        return Err("soul_document_too_large".into());
    }
    let key = receipt_key(actor, &id);
    let principal = receipts::principal(db, actor).await?;
    let at = stamp(now()?)?;
    put(db,&key,&json!({"method":"character.soul.save","request_id":id,"principal":actor.namespace,"principal_id":principal,"source_person":actor.source_person,"created_at":at,"updated_at":at,"expected_revision":body["expected_revision"],
        "digest":crate::memory::scope::hash(&json!(["character.soul.save",body])),"status":"running",
        "before":before["revision"],"after":revision(content),"content":content,"result":null})).await?;
    put(db, "control-soul-active-operation", &json!(key)).await?;
    Ok(json!({"prepared":true,"request_id":id}))
}
pub(super) async fn commit(
    db: &mut SqliteConnection,
    service: &Service,
    actor: &Actor,
    id: &str,
) -> Result<Value> {
    crate::host::personal::credentials::uuid(id)?;
    let key = receipt_key(actor, id);
    let mut pending = get(db, &key).await?.ok_or("not_found")?;
    if pending["method"] != "character.soul.save" {
        return Err("request_conflict".into());
    }
    if pending["status"] == "succeeded" {
        return Ok(pending["result"].clone());
    }
    if get(db, "control-soul-active-operation").await? != Some(json!(key)) {
        return Err("original_soul_operation_unavailable".into());
    }
    let current = snapshot(service)?;
    if current["revision"] != pending["before"] && current["revision"] != pending["after"] {
        return Err("version_conflict".into());
    }
    let content = pending["content"]
        .as_str()
        .ok_or("original_soul_operation_unavailable")?;
    if current["revision"] != pending["after"] {
        let staged = format!(".soul-{id}.tmp");
        if let Some(old) = service.root.snapshot_optional(&staged)? {
            if old.digest != hash(content) {
                return Err("precondition_failed".into());
            }
        } else {
            service.root.stage(&staged, content.as_bytes())?
        }
        if snapshot(service)?["revision"] != pending["before"] {
            return Err("version_conflict".into());
        }
        service.root.replace(&staged, "SOUL.md")?;
    }
    #[cfg(test)]
    crate::store::tests::commit_barrier("soul-renamed-before-activation").await;
    let mut doc = snapshot(service)?;
    if doc["revision"] != pending["after"] {
        return Err("version_conflict".into());
    }
    doc["updated_by"] = json!(actor.namespace);
    let result = json!({"revision":doc["revision"],"active_revision":doc["revision"],"applied_at":stamp(now()?)?});
    validate("SoulResult", &result)?;
    put(db, "character-soul-active", &doc).await?;
    pending["status"] = json!("succeeded");
    pending["updated_at"] = json!(stamp(now()?)?);
    pending["result_resource_id"] = json!("SOUL.md");
    pending["result_revision"] = doc["revision"].clone();
    pending["result"] = result.clone();
    pending
        .as_object_mut()
        .ok_or("invalid_control_record")?
        .remove("content");
    put(db, &key, &pending).await?;
    sqlx::query("DELETE FROM core_meta WHERE key='control-soul-active-operation'")
        .execute(db)
        .await?;
    Ok(result)
}
pub(super) async fn reload(
    db: &mut SqliteConnection,
    service: &Service,
    actor: &Actor,
    body: &Value,
) -> Result<Value> {
    validate("SoulReloadBody", body)?;
    if let Some(old) = previous(db, actor, "character.soul.reload", body).await? {
        return Ok(old["result"].clone());
    }
    if get(db, "control-soul-active-operation").await?.is_some() {
        return Err("original_soul_operation_pending".into());
    }
    let mut doc = snapshot(service)?;
    doc["updated_by"] = json!(actor.namespace);
    let result = json!({"revision":doc["revision"],"active_revision":doc["revision"],"applied_at":stamp(now()?)?});
    put(db, "character-soul-active", &doc).await?;
    record(db, actor, "character.soul.reload", body, &result).await?;
    Ok(result)
}
