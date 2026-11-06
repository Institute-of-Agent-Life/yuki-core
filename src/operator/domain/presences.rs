use super::*;
use crate::identity::assignment::{self, Action};
use sqlx::{Row, sqlite::SqliteRow};

pub(super) fn capability(method: &str) -> Option<&'static str> {
    Some(match method {
        "presence.list" | "presence.get" => "identity.presence.read",
        "presence.register" => "identity.presence.register",
        "presence.state" => "identity.presence.set_ingest",
        "presence.transfer" => "identity.presence.transfer",
        "presence.unregister" => "identity.presence.unregister",
        _ => return None,
    })
}
const SELECT: &str = "SELECT a.*,c.platform,c.native FROM assignments a JOIN accounts c ON c.id=a.account_id WHERE a.kind='self'";
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
        json!({"id":r.try_get::<String,_>("id")?,"account_id":r.try_get::<String,_>("account_id")?,
        "platform":r.try_get::<String,_>("platform")?,"external_account_id":r.try_get::<String,_>("native")?,
        "enabled":r.try_get::<String,_>("status")?=="active","status":r.try_get::<String,_>("status")?,
        "started_at":stamp("started_at")?,"ended_at":stamp("ended_at")?,
        "ingest_eligible":r.try_get::<bool,_>("ingest_eligible")?,"revision":r.try_get::<i64,_>("revision")?}),
    )
}
pub(super) async fn read(db: &mut SqliteConnection, method: &str, args: &Value) -> Result<Value> {
    if method == "presence.get" {
        return view(
            &sqlx::query(&format!("{SELECT} AND a.id=?"))
                .bind(identity::id(args)?)
                .fetch_optional(db)
                .await?
                .ok_or("not_found")?,
        );
    }
    let limit = args["limit"].as_i64().unwrap_or(100);
    if !(1..=250).contains(&limit) {
        return Err("validation_error".into());
    }
    let rows = sqlx::query(&format!("{SELECT} AND a.id>? ORDER BY a.id LIMIT ?"))
        .bind(args["after"].as_str().unwrap_or(""))
        .bind(limit + 1)
        .fetch_all(db)
        .await?;
    let items = rows
        .iter()
        .take(limit as usize)
        .map(view)
        .collect::<Result<Vec<_>>>()?;
    let next = if rows.len() > limit as usize {
        items.last().map(|v| v["id"].clone())
    } else {
        None
    };
    Ok(json!({"items":items,"next_cursor":next}))
}
async fn register(db: &mut SqliteConnection, body: &Value) -> Result<String> {
    let platform = body["platform"].as_str().ok_or("validation_error")?.trim();
    let native = body["external_account_id"]
        .as_str()
        .ok_or("validation_error")?
        .trim();
    if platform.is_empty() || native.is_empty() {
        return Err("validation_error".into());
    }
    // The first-party credentials service owns its receiving account. An Admin caller
    // cannot invent an app/connector receiver or transfer its credentials to SELF.
    if matches!(
        platform,
        "yuki-app" | "yuki-connector" | "app" | "connector"
    ) {
        return Err("precondition_failed".into());
    }
    let namespaces: Vec<String> = sqlx::query_scalar("SELECT namespace FROM accounts WHERE platform=?1 UNION SELECT namespace FROM host_installations WHERE platform=?1")
        .bind(platform).fetch_all(&mut *db).await?;
    if namespaces.len() != 1 {
        return Err("binding_ambiguous".into());
    }
    let current = sqlx::query("SELECT a.id,a.kind,a.revision FROM assignments a JOIN accounts c ON c.id=a.account_id WHERE c.platform=? AND c.namespace=? AND c.native=? AND a.status<>'ended'")
        .bind(platform).bind(&namespaces[0]).bind(native).fetch_optional(&mut *db).await?;
    let expected = body["expected_assignment_id"].as_str();
    if current.as_ref().map(|v| v.get::<&str, _>("id")) != expected {
        return Err("binding_ambiguous".into());
    }
    if body["expected_revision"] != current.as_ref().map_or(0, |v| v.get::<i64, _>("revision")) {
        return Err("version_conflict".into());
    }
    let id = if let Some(row) = current {
        let id: String = row.get("id");
        if row.get::<&str, _>("kind") == "self" {
            id
        } else {
            let at = bindings::transition_time(db, &id).await?;
            assignment::apply(
                db,
                &Action::Transfer {
                    stage: id,
                    at,
                    observed: at,
                    person: None,
                },
            )
            .await?["assignment"]
                .as_str()
                .ok_or("precondition_failed")?
                .to_owned()
        }
    } else {
        let now = crate::host::personal::credentials::now()? * 1000;
        assignment::apply(
            db,
            &Action::Open {
                platform: platform.into(),
                namespace: namespaces[0].clone(),
                native: native.into(),
                observed: now,
                person: None,
                start: Some(now),
            },
        )
        .await?["assignment"]
            .as_str()
            .ok_or("precondition_failed")?
            .to_owned()
    };
    sqlx::query("UPDATE assignments SET display_name=? WHERE id=?")
        .bind(body["display_name"].as_str().unwrap_or("").trim())
        .bind(&id)
        .execute(db)
        .await?;
    Ok(id)
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
            "presence.register" => "PresenceRegistrationBody",
            "presence.state" => "PresenceBody",
            "presence.transfer" => "PresenceTransferBody",
            _ => "CommandBody",
        },
    )?;
    if let Some(old) = previous(db, actor, method, body).await? {
        return Ok(old["result"].clone());
    }
    let id = if method == "presence.register" {
        register(db, body).await?
    } else {
        let id = identity::id(body)?;
        let before = read(db, "presence.get", &json!({"id":id})).await?;
        if body["expected_revision"] != before["revision"] {
            return Err("version_conflict".into());
        }
        match method {
            "presence.state" => {
                let mut changed = false;
                if let Some(enabled) = body["enabled"].as_bool() {
                    if before["status"] == "ended" {
                        return Err("precondition_failed".into());
                    }
                    changed = before["enabled"] != enabled;
                    if changed {
                        let at = bindings::transition_time(db, &id).await?;
                        assignment::apply(
                            db,
                            &Action::Enabled {
                                stage: id.clone(),
                                enabled,
                                at,
                            },
                        )
                        .await?;
                    }
                }
                if let Some(ingest) = body["ingest_eligible"].as_bool()
                    && before["ingest_eligible"] != ingest
                {
                    sqlx::query(
                        "UPDATE assignments SET ingest_eligible=?,revision=revision+? WHERE id=?",
                    )
                    .bind(ingest)
                    .bind(i64::from(!changed))
                    .bind(&id)
                    .execute(&mut *db)
                    .await?;
                }
                id
            }
            "presence.unregister" => {
                let at = bindings::transition_time(db, &id).await?;
                assignment::apply(
                    db,
                    &Action::End {
                        stage: id.clone(),
                        at,
                    },
                )
                .await?;
                id
            }
            "presence.transfer" => {
                if before["status"] == "ended"
                    || matches!(
                        before["platform"].as_str(),
                        Some("app" | "connector" | "yuki-app" | "yuki-connector")
                    )
                {
                    return Err("precondition_failed".into());
                }
                let target = body["to_person"].as_str().ok_or("validation_error")?;
                let person = identity::read(db, "person.get", &json!({"id":target})).await?;
                if person["enabled"] != true || person["source_enabled"] != true {
                    return Err("precondition_failed".into());
                }
                let at = bindings::transition_time(db, &id).await?;
                assignment::apply(
                    db,
                    &Action::Transfer {
                        stage: id,
                        at,
                        observed: at,
                        person: Some(target.into()),
                    },
                )
                .await?["assignment"]
                    .as_str()
                    .ok_or("precondition_failed")?
                    .to_owned()
            }
            _ => return Err("unsupported_control_method".into()),
        }
    };
    let row = sqlx::query("SELECT revision,status FROM assignments WHERE id=?")
        .bind(&id)
        .fetch_one(&mut *db)
        .await?;
    let revision: i64 = row.get("revision");
    let status: &str = row.get("status");
    let result = json!({"success":true,"resource_id":id,"revision":revision,"audit_id":operation(body)?,
        "effective_state":{"resource":id,"revision":revision,"status":status},"operation":null});
    record(db, actor, method, body, &result).await?;
    Ok(result)
}
