//! Operator approval is retained separately from the installed request.
use crate::{
    Result,
    operator::domain::{get, put},
};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
use std::collections::BTreeSet;

pub(super) async fn configure(
    db: &mut SqliteConnection,
    install: &super::Install,
) -> Result<BTreeSet<String>> {
    let key = format!("plugin-approval:{}", install.id);
    let manifest = install.revision()?;
    let declaration = crate::memory::scope::hash(&serde_json::to_value(install)?);
    let previous = get(db, &key).await?;
    if let Some(mut saved) = previous.clone()
        && saved["manifest"] == manifest
        && (saved["declaration"].is_null() || saved["declaration"] == declaration)
    {
        saved["declaration"] = json!(declaration);
        if saved["deployment"].is_null() {
            saved["deployment"] = json!(crate::identity::new_id(db).await?);
        }
        put(db, &key, &saved).await?;
        return Ok(if saved["approved"].is_array() {
            serde_json::from_value(saved["approved"].clone())?
        } else {
            BTreeSet::new()
        });
    }
    // Only the first local configuration boot supplies the initial operator grant.
    // Changed declarations require a fresh explicit approval, including after restart.
    let approved = if previous.is_none() {
        json!(install.grants)
    } else {
        Value::Null
    };
    let deployment = crate::identity::new_id(db).await?;
    put(db,&key,&json!({"manifest":manifest,"declaration":declaration,"deployment":deployment,"requested":install.grants,"approved":approved})).await?;
    Ok(if previous.is_none() {
        install.grants.clone()
    } else {
        BTreeSet::new()
    })
}

pub(crate) async fn prepare(
    db: &mut SqliteConnection,
    method: &str,
    body: &Value,
) -> Result<Value> {
    let id = body["plugin_id"].as_str().ok_or("validation_error")?;
    let before = super::configuration::status(db, &json!({"plugin_id":id})).await?;
    if before["revision"] != body["expected_revision"] {
        return Err("version_conflict".into());
    }
    match method {
        "plugin.approve" => {
            let requested: BTreeSet<String> =
                serde_json::from_value(before["requested_permissions"].clone())?;
            let selected: BTreeSet<String> = match body.get("permissions") {
                Some(v) => serde_json::from_value(v.clone())?,
                None => requested.clone(),
            };
            if !selected.is_subset(&requested) {
                return Err("precondition_failed".into());
            }
            let approved: BTreeSet<String> =
                serde_json::from_value(before["approved_permissions"].clone())?;
            let approval_key = format!("plugin-approval:{id}");
            let mut decision = get(db, &approval_key).await?.ok_or("precondition_failed")?;
            if get(db, &format!("plugin-install:{id}")).await?.is_none() {
                return Err("precondition_failed".into());
            }
            if selected != approved || !decision["approved"].is_array() {
                super::domain::apply(db, &super::Change::Enable(id.into(), false)).await?;
                sqlx::query("UPDATE host_installations SET grants=? WHERE plugin=?")
                    .bind(serde_json::to_string(&selected)?)
                    .bind(id)
                    .execute(&mut *db)
                    .await?;
                decision["approved"] = json!(selected);
                put(db, &approval_key, &decision).await?;
                if selected.contains("forget.owner") {
                    sqlx::query("INSERT INTO source_owners(id,enabled) VALUES(?,0) ON CONFLICT(id) DO NOTHING").bind(id).execute(&mut *db).await?;
                }
            }
        }
        "plugin.enable" | "plugin.disable" => {
            super::domain::apply(
                db,
                &super::Change::Enable(id.into(), method == "plugin.enable"),
            )
            .await?;
        }
        "plugin.doctor" => {}
        _ => return Err("unsupported_control_method".into()),
    }
    super::configuration::status(db, &json!({"plugin_id":id})).await
}

pub(crate) async fn discover(
    db: &mut SqliteConnection,
    installs: &[super::Install],
) -> Result<String> {
    super::grants::validate(installs)?;
    let mut changed = Vec::new();
    for install in installs {
        let key = format!("plugin-install:{}", install.id);
        if get(db, &key).await?.as_ref() != Some(&serde_json::to_value(install)?) {
            changed.push(install.id.clone());
        }
    }
    let known: Vec<String> = sqlx::query_scalar("SELECT plugin FROM host_installations")
        .fetch_all(&mut *db)
        .await?;
    for id in known {
        if changed.contains(&id) || !installs.iter().any(|i| i.id == id) {
            super::domain::apply(db, &super::Change::Enable(id.clone(), false)).await?;
            sqlx::query("DELETE FROM host_commands WHERE plugin=?")
                .bind(&id)
                .execute(&mut *db)
                .await?;
            sqlx::query("DELETE FROM core_meta WHERE key=?")
                .bind(format!("plugin-install:{id}"))
                .execute(&mut *db)
                .await?;
        }
    }
    for install in installs.iter().filter(|i| changed.contains(&i.id)) {
        let declaration = crate::memory::scope::hash(&serde_json::to_value(install)?);
        let deployment = crate::identity::new_id(db).await?;
        put(db,&format!("plugin-approval:{}",install.id),&json!({"manifest":install.revision()?,"declaration":declaration,"deployment":deployment,"requested":install.grants,"approved":null})).await?;
        super::domain::register(db, install, &BTreeSet::new(), false).await?;
    }
    inventory(db).await
}
pub(crate) async fn inventory(db: &mut SqliteConnection) -> Result<String> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT key,value FROM core_meta WHERE key LIKE 'plugin-install:%' ORDER BY key",
    )
    .fetch_all(db)
    .await?;
    Ok(crate::memory::scope::hash(&json!(rows)))
}

pub(crate) async fn result(db: &mut SqliteConnection, method: &str, id: &str) -> Result<Value> {
    let state = super::configuration::status(db, &json!({"plugin_id":id})).await?;
    let mut result = json!({"resource_id":id,"revision":state["revision"],"effective_state":{"status":state["status"]}});
    if method == "plugin.doctor" {
        let pending: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM owner_erasures WHERE owner=? AND proof IS NULL)",
        )
        .bind(id)
        .fetch_one(&mut *db)
        .await?;
        let mut problems = Vec::new();
        if pending {
            problems.push("erasure_pending");
        }
        if state["status"] == "failed" {
            problems.push("instance_unavailable");
        }
        if state["status"] == "discovered" {
            problems.push("approval_required");
        }
        if state["status"] == "unavailable" {
            problems.push("installation_unavailable");
        }
        if state["status"] != "discovered"
            && state["status"] != "unavailable"
            && get(db, &format!("plugin-config-definition:{id}"))
                .await?
                .is_some()
        {
            let config = super::configuration::read(db, &json!({"plugin_id":id})).await?;
            if config["valid"] != true {
                problems.push("configuration_invalid");
            }
        }
        result["effective_state"] = json!({"status":if problems.is_empty(){"healthy"}else{"unhealthy"},"diagnostics":{"plugin_id":id,"problems":problems,"runtime_status":state["status"]}});
    }
    Ok(result)
}
