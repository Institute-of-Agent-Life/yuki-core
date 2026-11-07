//! Native resource preparation keeps its own original effect and application authority.
use super::{calls, domain, grants};
use crate::Result;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub(super) fn validate(parts: &[Value]) -> bool {
    for (index, part) in parts.iter().enumerate() {
        if let Some(preparation) = part.get("preparation")
            && (preparation != true
                || grants::methods()["methods"]
                    .get("gateway.prepare_part")
                    .is_none()
                || !matches!(part["kind"].as_str(), Some("image" | "file"))
                || part.get("media").is_none()
                || part.get("reply").is_some()
                || part.get("prepared_part").is_some()
                || parts
                    .iter()
                    .filter(|p| p["prepared_part"].as_u64() == Some(index as u64))
                    .count()
                    != 1)
        {
            return false;
        }
        if let Some(prior) = part.get("prepared_part") {
            let Some(prior) = prior
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .filter(|v| *v < index)
            else {
                return false;
            };
            if parts[prior]["preparation"] != true
                || parts[prior]["kind"] != part["kind"]
                || part.get("media").is_some()
            {
                return false;
            }
        }
    }
    true
}

async fn origin(db: &mut SqliteConnection, plugin: &str, execution: &str) -> Result<Value> {
    let row = sqlx::query("SELECT i.namespace,i.platform,i.bot_stage,json_extract(a.value,'$.deployment') AS deployment,
        json_extract(c.value,'$.gateway_media') AS identity FROM host_installations i
        JOIN core_meta a ON a.key='plugin-approval:'||i.plugin
        JOIN core_meta c ON c.key='plugin-install:'||i.plugin WHERE i.plugin=?")
        .bind(plugin).fetch_one(&mut *db).await?;
    let identity: super::gateway_media::Identity = serde_json::from_str(row.try_get("identity")?)?;
    if !identity.valid() {
        return Err("native preparation requires original application identity".into());
    }
    let current = json!({"namespace":row.try_get::<&str,_>("namespace")?,"platform":row.try_get::<&str,_>("platform")?,
        "stage":row.try_get::<&str,_>("bot_stage")?,"deployment":row.try_get::<&str,_>("deployment")?,"identity":identity});
    let original = sqlx::query("SELECT o.* FROM gateway_media_origins o JOIN executions x ON x.source=o.event WHERE x.id=? AND o.plugin=?")
        .bind(execution).bind(plugin).fetch_one(&mut *db).await?;
    super::gateway_media::cached_authority(db, original.get("event")).await?;
    let prior = json!({"namespace":original.get::<&str,_>("namespace"),"platform":original.get::<&str,_>("platform"),
        "stage":original.get::<&str,_>("stage"),"deployment":original.get::<&str,_>("deployment"),
        "identity":serde_json::from_str::<Value>(original.get("identity"))?});
    if current != prior {
        return Err("native preparation belongs to a different original application".into());
    }
    Ok(prior)
}

async fn receipt(
    db: &mut SqliteConnection,
    plugin: &str,
    execution: &str,
    parent: &str,
    args: &Value,
) -> Result<Value> {
    let prior = args["part"]["prepared_part"]
        .as_i64()
        .filter(|p| *p >= 0)
        .ok_or("invalid preparation index")?;
    let current = args["part_ordinal"]
        .as_i64()
        .ok_or("missing delivery ordinal")?;
    if prior >= current {
        return Err("preparation must precede delivery".into());
    }
    let operation = format!("{parent}:gateway:{}", prior + 1);
    let row = sqlx::query(
        "SELECT c.args,c.result,s.version,s.digest FROM host_calls c
        JOIN operations o ON o.namespace=c.execution AND o.operation_id=c.operation
        JOIN host_installations i ON i.plugin=c.plugin AND i.revision=c.revision
        JOIN host_media_selected s ON s.handle=c.handle
        JOIN media_versions v ON v.id=s.version AND v.digest=s.digest
        JOIN media_version_readers r ON r.version=v.id AND r.reader=c.execution
        WHERE c.plugin=? AND c.execution=? AND c.operation=? AND c.method='gateway.prepare_part'
        AND c.state='confirmed' AND o.state='confirmed' AND o.parent=? AND o.ordinal=?",
    )
    .bind(plugin)
    .bind(execution)
    .bind(&operation)
    .bind(parent)
    .bind(prior + 1)
    .fetch_one(&mut *db)
    .await?;
    let original: Value = serde_json::from_str(row.try_get("args")?)?;
    let result: Value = serde_json::from_str(row.try_get("result")?)?;
    if original["preparation_origin"] != origin(db, plugin, execution).await?
        || original["route"] != args["route"]
        || original["part"]["kind"] != args["part"]["kind"]
        || result["resource"]["kind"] != args["part"]["kind"]
        || original["media"]
            != json!({"artifact_id":row.try_get::<&str,_>("version")?,"version":row.try_get::<&str,_>("digest")?})
    {
        return Err("original native preparation changed".into());
    }
    Ok(
        json!({"operation":operation,"media":original["media"],"origin":original["preparation_origin"],"resource":result["resource"]}),
    )
}

pub(super) async fn attach(db: &mut SqliteConnection, request: &mut calls::Request) -> Result<()> {
    if request.method == "gateway.prepare_part" {
        request.args["preparation_origin"] =
            origin(db, &request.plugin, &request.execution).await?;
    } else if request.args["part"].get("prepared_part").is_some() {
        let proof = receipt(
            db,
            &request.plugin,
            &request.execution,
            request
                .parent
                .as_deref()
                .ok_or("preparation requires original send")?,
            &request.args,
        )
        .await?;
        request.args["media"] = proof["media"].clone();
        request.args["purpose"] = json!("deliver");
        request.args["prepared"] = proof;
    }
    Ok(())
}

pub(super) async fn check(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    execution: &str,
    method: &str,
    parent: Option<&str>,
    args: &Value,
) -> Result<()> {
    if method == "gateway.prepare_part" {
        domain::active(db, plugin, generation, "media.open").await?;
        if args["preparation_origin"] != origin(db, plugin, execution).await?
            || args["part"]["preparation"] != true
            || args["media"] != args["part"]["media"]
        {
            return Err("original preparation authority differs".into());
        }
    } else if args["part"].get("prepared_part").is_some() {
        domain::active(db, plugin, generation, "media.open").await?;
        let proof = receipt(
            db,
            plugin,
            execution,
            parent.ok_or("missing original send")?,
            args,
        )
        .await?;
        if args["prepared"] != proof || args["media"] != proof["media"] {
            return Err("prepared resource differs from original receipt".into());
        }
    }
    Ok(())
}
