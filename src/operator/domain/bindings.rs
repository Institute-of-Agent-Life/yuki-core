use super::*;
use crate::identity::assignment::{self, Action};
use sqlx::{Row, sqlite::SqliteRow};

pub(super) fn capability(method: &str) -> Option<&'static str> {
    Some(match method {
        "binding.list" | "binding.get" => "identity.binding.read",
        "binding.attach" => "identity.binding.attach",
        "binding.enable" => "identity.binding.enable",
        "binding.disable" => "identity.binding.disable",
        "binding.unbind" => "identity.binding.unbind",
        "binding.transfer" => "identity.binding.transfer",
        _ => return None,
    })
}
const SELECT: &str = "SELECT a.*,c.platform,c.native,coalesce(p.current_root,p.id) AS current_person_id FROM assignments a JOIN accounts c ON c.id=a.account_id JOIN persons p ON p.id=a.original_person";
fn view(r: &SqliteRow) -> Result<Value> {
    let stamp = |field| -> Result<Option<String>> {
        r.try_get::<Option<i64>, _>(field)?
            .map(|t| {
                chrono::DateTime::from_timestamp_millis(t)
                    .map(|v| v.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
                    .ok_or_else(|| "invalid timestamp".into())
            })
            .transpose()
    };
    Ok(
        json!({"id":r.try_get::<String,_>("id")?,"account_id":r.try_get::<String,_>("account_id")?,"person_id":r.try_get::<String,_>("original_person")?,
        "platform":r.try_get::<String,_>("platform")?,"external_account_id":r.try_get::<String,_>("native")?,"display_name":r.try_get::<String,_>("display_name")?,
        "status":r.try_get::<String,_>("status")?,"started_at":stamp("started_at")?,"ended_at":stamp("ended_at")?,"revision":r.try_get::<i64,_>("revision")?}),
    )
}
pub(super) async fn family(db: &mut SqliteConnection, root: &str) -> Result<Vec<Value>> {
    sqlx::query(&format!(
        "{SELECT} WHERE coalesce(p.current_root,p.id)=? ORDER BY a.id"
    ))
    .bind(root)
    .fetch_all(db)
    .await?
    .iter()
    .map(view)
    .collect()
}
pub(super) async fn read(db: &mut SqliteConnection, method: &str, args: &Value) -> Result<Value> {
    if method == "binding.get" {
        let row = sqlx::query(&format!("{SELECT} WHERE a.id=?"))
            .bind(identity::id(args)?)
            .fetch_optional(db)
            .await?
            .ok_or("not_found")?;
        let mut result = view(&row)?;
        result["current_person_id"] = json!(row.try_get::<String, _>("current_person_id")?);
        return Ok(result);
    }
    let after = args["after"].as_str().unwrap_or("");
    let limit = args["limit"].as_i64().unwrap_or(100);
    if !(1..=250).contains(&limit) {
        return Err("validation_error".into());
    }
    let owner = args["person_id"].as_str();
    if let Some(id) = owner {
        crate::host::personal::credentials::uuid(id)?;
    }
    let rows=sqlx::query(&format!("{SELECT} WHERE a.id>?1 AND (?2 IS NULL OR coalesce(p.current_root,p.id)=(SELECT coalesce(current_root,id) FROM persons WHERE id=?2)) AND (?3 IS NULL OR c.platform=?3) AND (?4 IS NULL OR c.native=?4) ORDER BY a.id LIMIT ?5"))
        .bind(after).bind(owner).bind(args["platform"].as_str()).bind(args["external_account_id"].as_str()).bind(limit+1).fetch_all(db).await?;
    let items = rows
        .iter()
        .take(limit as usize)
        .map(|r| {
            let mut v = view(r)?;
            v["current_person_id"] = json!(r.try_get::<String, _>("current_person_id")?);
            Ok(v)
        })
        .collect::<Result<Vec<_>>>()?;
    let next = if rows.len() > limit as usize {
        items.last().map(|i| i["id"].clone())
    } else {
        None
    };
    Ok(json!({"items":items,"next_cursor":next}))
}
pub(super) async fn transition_time(db: &mut SqliteConnection, stage: &str) -> Result<i64> {
    let latest: i64 = sqlx::query_scalar("SELECT max(at) FROM assignment_states WHERE stage=?")
        .bind(stage)
        .fetch_one(db)
        .await?;
    Ok((crate::host::personal::credentials::now()? * 1000)
        .max(latest.checked_add(1).ok_or("precondition_failed")?))
}
pub(super) async fn write(
    db: &mut SqliteConnection,
    actor: &Actor,
    method: &str,
    body: &Value,
) -> Result<Value> {
    identity::dto(
        body,
        match method {
            "binding.attach" => "BindingBody",
            "binding.transfer" => "TransferBody",
            _ => "CommandBody",
        },
    )?;
    if let Some(old) = previous(db, actor, method, body).await? {
        let mut result = old["result"].clone();
        if method == "binding.transfer" {
            result["replay"] = json!(true);
        }
        return Ok(result);
    }
    let id = identity::id(body)?;
    let result = if method == "binding.attach" {
        let person = identity::read(db, "person.get", &json!({"id":id})).await?;
        if body["expected_revision"] != person["revision"] {
            return Err("version_conflict".into());
        }
        let root = person["id"].as_str().ok_or("not_found")?;
        let platform = body["platform"].as_str().ok_or("validation_error")?.trim();
        let native = body["external_account_id"]
            .as_str()
            .ok_or("validation_error")?
            .trim();
        if platform.is_empty()
            || native.is_empty()
            || matches!(
                platform,
                "yuki-app" | "yuki-connector" | "app" | "connector"
            )
        {
            return Err("precondition_failed".into());
        }
        // Admin v1 has no namespace field. Resolve only actual configured account/installation facts.
        let namespaces: Vec<String>=sqlx::query_scalar("SELECT namespace FROM accounts WHERE platform=?1 UNION SELECT namespace FROM host_installations WHERE platform=?1")
            .bind(platform).fetch_all(&mut *db).await?;
        if namespaces.len() != 1 {
            return Err("binding_ambiguous".into());
        }
        let existing=sqlx::query("SELECT a.id,a.original_person,p.current_root FROM assignments a JOIN accounts c ON c.id=a.account_id LEFT JOIN persons p ON p.id=a.original_person WHERE c.platform=? AND c.namespace=? AND c.native=? AND a.status<>'ended'")
            .bind(platform).bind(&namespaces[0]).bind(native).fetch_optional(&mut *db).await?;
        let binding = if let Some(row) = existing {
            let owner: Option<String> = row.try_get("original_person")?;
            let current: Option<String> = row.try_get("current_root")?;
            if current.as_ref().or(owner.as_ref()).map(String::as_str) != Some(root) {
                return Err("binding_ambiguous".into());
            }
            row.try_get::<String, _>("id")?
        } else {
            let now = crate::host::personal::credentials::now()? * 1000;
            let created = assignment::apply(
                db,
                &Action::Open {
                    platform: platform.into(),
                    namespace: namespaces[0].clone(),
                    native: native.into(),
                    observed: now,
                    person: Some(root.into()),
                    start: Some(now),
                },
            )
            .await?;
            sqlx::query("UPDATE persons SET revision=revision+1 WHERE id=?")
                .bind(root)
                .execute(&mut *db)
                .await?;
            created["assignment"]
                .as_str()
                .ok_or("precondition_failed")?
                .to_owned()
        };
        sqlx::query("UPDATE assignments SET display_name=? WHERE id=?")
            .bind(body["display_name"].as_str().unwrap_or("").trim())
            .bind(&binding)
            .execute(&mut *db)
            .await?;
        let state = read(db, "binding.get", &json!({"id":binding})).await?;
        json!({"success":true,"resource_id":binding,"revision":state["revision"],"audit_id":operation(body)?,"effective_state":{"binding_id":binding,"person_id":root,"platform":platform,"status":state["status"],"revision":state["revision"]},"operation":null})
    } else {
        let before = read(db, "binding.get", &json!({"id":id})).await?;
        if method != "binding.transfer" && body["expected_revision"] != before["revision"] {
            return Err("version_conflict".into());
        }
        if method == "binding.transfer" {
            if before["status"] == "ended" {
                return Err("precondition_failed".into());
            }
            let target = body["to_person"].as_str().ok_or("validation_error")?;
            let owner = identity::read(db, "person.get", &json!({"id":target})).await?;
            if owner["enabled"] != true || owner["source_enabled"] != true {
                return Err("precondition_failed".into());
            }
            let now = transition_time(db, &id).await?;
            let next = assignment::apply(
                db,
                &Action::Transfer {
                    stage: id.clone(),
                    at: now,
                    observed: now,
                    person: Some(target.into()),
                },
            )
            .await?;
            json!({"old_binding_id":id,"binding_id":next["assignment"],"person_id":target,"replay":false})
        } else {
            match method {
                "binding.unbind" => {
                    let at = transition_time(db, &id).await?;
                    assignment::apply(
                        db,
                        &Action::End {
                            stage: id.clone(),
                            at,
                        },
                    )
                    .await?;
                }
                "binding.enable" if before["status"] == "ended" => {
                    return Err("precondition_failed".into());
                }
                "binding.enable" | "binding.disable"
                    if before["status"] != "ended"
                        && before["status"]
                            != if method == "binding.enable" {
                                "active"
                            } else {
                                "disabled"
                            } =>
                {
                    let at = transition_time(db, &id).await?;
                    assignment::apply(
                        db,
                        &Action::Enabled {
                            stage: id.clone(),
                            enabled: method == "binding.enable",
                            at,
                        },
                    )
                    .await?;
                }
                _ => {}
            }
            let state = read(db, "binding.get", &json!({"id":id})).await?;
            json!({"success":true,"resource_id":id,"revision":state["revision"],"audit_id":operation(body)?,"effective_state":{"resource":id,"revision":state["revision"],"status":state["status"]},"operation":null})
        }
    };
    record(db, actor, method, body, &result).await?;
    Ok(result)
}
