use super::*;
use crate::identity::person::{self, Change};
use sqlx::Row;

pub(super) fn capability(method: &str) -> Option<&'static str> {
    Some(match method {
        "person.list" | "person.get" => "identity.person.read",
        "person.create" => "identity.person.create",
        "person.rename" => "identity.person.rename",
        "person.merge" => "identity.person.merge",
        "person.enable" => "identity.person.enable",
        "person.disable" => "identity.person.disable",
        "person.superuser.grant" | "person.superuser.revoke" => "identity.person.superuser",
        "person.forget" => "identity.person.forget",
        "person.client-pair" => "identity.client.pair",
        "device.revoke" => "identity.client.revoke",
        _ => return None,
    })
}
pub(super) fn id(args: &Value) -> Result<String> {
    let id = args["id"]
        .as_str()
        .ok_or("validation_error")?
        .to_ascii_lowercase();
    crate::host::personal::credentials::uuid(&id)?;
    Ok(id)
}
pub(super) fn dto(body: &Value, schema: &str) -> Result<Value> {
    let mut value = body.clone();
    value
        .as_object_mut()
        .ok_or("validation_error")?
        .remove("id");
    validate(schema, &value)?;
    Ok(value)
}
pub(super) async fn read(db: &mut SqliteConnection, method: &str, args: &Value) -> Result<Value> {
    if method == "person.list" {
        let after = args["after"].as_str().unwrap_or("");
        let limit = args["limit"].as_i64().unwrap_or(100);
        if !(1..=250).contains(&limit) {
            return Err("validation_error".into());
        }
        if !after.is_empty() {
            crate::host::personal::credentials::uuid(after)?;
        }
        let rows=sqlx::query("SELECT id,name,enabled,admin,revision FROM persons WHERE current_root IS NULL AND id>? ORDER BY id LIMIT ?")
            .bind(after).bind(limit+1).fetch_all(db).await?;
        let more = rows.len() > limit as usize;
        let items=rows.iter().take(limit as usize).map(|r|Ok(json!({"id":r.try_get::<String,_>("id")?,"display_name":r.try_get::<String,_>("name")?,"enabled":r.try_get::<bool,_>("enabled")?,"is_superuser":r.try_get::<bool,_>("admin")?,"revision":r.try_get::<i64,_>("revision")?}))).collect::<Result<Vec<_>>>()?;
        let next = if more {
            items.last().map(|x| x["id"].clone())
        } else {
            None
        };
        return Ok(json!({"items":items,"next_cursor":next}));
    }
    let original = id(args)?;
    let row=sqlx::query("SELECT p.id AS source,p.enabled AS source_enabled,p.revision AS source_revision,r.* FROM persons p JOIN persons r ON r.id=coalesce(p.current_root,p.id) WHERE p.id=?")
        .bind(&original).fetch_optional(&mut *db).await?.ok_or("not_found")?;
    let root: String = row.try_get("id")?;
    let bindings = super::bindings::family(db, &root).await?;
    Ok(
        json!({"id":root,"source_person_id":original,"source_enabled":row.try_get::<bool,_>("source_enabled")?,"source_revision":row.try_get::<i64,_>("source_revision")?,"revision":row.try_get::<i64,_>("revision")?,"enabled":row.try_get::<bool,_>("enabled")?,"display_name":row.try_get::<String,_>("name")?,"is_superuser":row.try_get::<bool,_>("admin")?,"bindings":bindings}),
    )
}
pub(super) async fn write(
    db: &mut SqliteConnection,
    actor: &Actor,
    method: &str,
    body: &Value,
) -> Result<Value> {
    let schema = match method {
        "person.create" => "PersonBody",
        "person.rename" => "RenameBody",
        "person.merge" => "TransferBody",
        "person.forget" | "device.revoke" => "IdentityRequest",
        "person.client-pair" => "PairCodeBody",
        _ => "CommandBody",
    };
    dto(body, schema)?;
    let prior = previous(db, actor, method, body).await?;
    // Preserve receipts issued by the previous personal Admin adapter during coexistence.
    if prior.is_none()
        && let Some(owner) = actor.namespace.strip_prefix("person:")
    {
        let old:Option<(String,String)>=sqlx::query_as("SELECT digest,result FROM personal_receipts WHERE owner=? AND scope='admin' AND operation=?")
            .bind(owner).bind(operation(body)?).fetch_optional(&mut *db).await?;
        if let Some((digest, result)) = old {
            if !matches!(method, "person.superuser.grant" | "person.superuser.revoke")
                || digest
                    != crate::host::personal::credentials::fingerprint(
                        "admin.person_superuser",
                        &id(body)?,
                        &json!({"enabled":method=="person.superuser.grant","expected_revision":body["expected_revision"]}),
                    )
            {
                return Err("request_conflict".into());
            }
            return Ok(serde_json::from_str(&result)?);
        }
    }
    if method == "person.client-pair" {
        let who = id(body)?;
        let purpose =
            serde_json::from_value(body.get("purpose").cloned().unwrap_or(json!("chat")))?;
        // The credential service persists only the hash. Its one-time code never enters control receipts.
        let result = crate::host::personal::credentials::issue(
            db,
            &actor.namespace,
            &who,
            &operation(body)?,
            purpose,
            None,
        )
        .await?;
        if prior.is_none() {
            let mut safe = result.clone();
            safe["code"] = Value::Null;
            record(db, actor, method, body, &safe).await?;
        }
        return Ok(result);
    }
    if let Some(prior) = prior {
        let mut result = prior["result"].clone();
        if matches!(method, "person.merge" | "person.rename" | "device.revoke") {
            result["replay"] = json!(true);
        }
        return Ok(result);
    }
    let target = if method == "person.create" {
        crate::identity::new_id(db).await?
    } else {
        id(body)?
    };
    let mut before = Value::Null;
    if method.starts_with("person.") && method != "person.create" {
        before = read(db, "person.get", &json!({"id":target})).await?;
        if body.get("expected_revision").is_some()
            && body["expected_revision"]
                != before[if method == "person.enable" {
                    "source_revision"
                } else {
                    "revision"
                }]
        {
            return Err("version_conflict".into());
        }
    } else if method == "person.create" && body["expected_revision"] != 0 {
        return Err("version_conflict".into());
    }
    let root = before["id"].as_str().unwrap_or(&target).to_owned();
    if method.starts_with("person.") && method != "person.forget" && method != "person.create" {
        let forgotten: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM forgotten_people WHERE person IN(?1,?2,?3))",
        )
        .bind(&target)
        .bind(&root)
        .bind(body["to_person"].as_str())
        .fetch_one(&mut *db)
        .await?;
        if forgotten {
            return Err("precondition_failed".into());
        }
    }
    let result = match method {
        "person.forget" => {
            let key = if let Some(owner) = actor.namespace.strip_prefix("person:") {
                format!("admin-forget:{owner}:{}", operation(body)?)
            } else {
                format!("control-forget:{}:{}", actor.namespace, operation(body)?)
            };
            let report = crate::privacy::forget::apply(db, &key, &target).await?;
            json!({"person_id":target,"deleted":true,"peer_cleanup":report["owners_pending"],"managed_storage":report["managed_storage"]})
        }
        "device.revoke" => {
            let mut result = crate::host::personal::credentials::revoke(db, &target, None).await?;
            result["replay"] = json!(false);
            result
        }
        _ => {
            let change = match method {
                "person.create" => Change::Create(
                    target.clone(),
                    body["display_name"].as_str().unwrap_or("").trim().into(),
                ),
                "person.rename" => Change::Rename(
                    root.clone(),
                    body["display_name"]
                        .as_str()
                        .ok_or("validation_error")?
                        .into(),
                ),
                "person.merge" => Change::Merge(
                    root.clone(),
                    body["to_person"]
                        .as_str()
                        .ok_or("validation_error")?
                        .to_ascii_lowercase(),
                ),
                "person.enable" | "person.disable" => {
                    Change::Enable(root.clone(), method == "person.enable")
                }
                "person.superuser.grant" | "person.superuser.revoke" => {
                    Change::Admin(root.clone(), method == "person.superuser.grant")
                }
                _ => return Err("unsupported_control_method".into()),
            };
            person::apply(db, &change).await?;
            if method == "person.enable" && root != target && before["source_enabled"] == false {
                person::apply(db, &Change::Enable(target.clone(), true)).await?;
            }
            let after = read(db, "person.get", &json!({"id":target})).await?;
            if matches!(method, "person.merge" | "person.rename") {
                json!({"person_id":after["id"],"replay":false})
            } else {
                let rev = &after[if method == "person.enable" {
                    "source_revision"
                } else {
                    "revision"
                }];
                let state = if method == "person.disable" {
                    json!({"enabled":false,"revision":rev})
                } else {
                    json!({"resource":target,"revision":rev,"status":match method { "person.superuser.grant"=>"superuser", "person.superuser.revoke"=>"revoked", "person.enable"=>"enabled", _=>"created" }})
                };
                json!({"success":true,"resource_id":target,"revision":rev,"audit_id":operation(body)?,"effective_state":state,"operation":null})
            }
        }
    };
    record(db, actor, method, body, &result).await?;
    Ok(result)
}
