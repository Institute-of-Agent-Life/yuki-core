use super::credentials::{self, Principal, Purpose};
use crate::Result;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub(super) async fn proof(
    db: &mut SqliteConnection,
    who: &Principal,
    plugin: &str,
) -> Result<Value> {
    let generation:String=sqlx::query_scalar("SELECT generation FROM host_installations WHERE plugin=? AND enabled AND online AND generation IS NOT NULL AND EXISTS(SELECT 1 FROM json_each(grants) WHERE value='device.connections')")
        .bind(plugin).fetch_optional(&mut *db).await?.ok_or("plugin_grant_unavailable")?;
    super::super::domain::active(db, plugin, &generation, "device.connections").await?;
    let raw = credentials::random::<32>()?;
    let expires = credentials::now()? + 60;
    sqlx::query(
        "INSERT INTO connector_proofs(hash,device,plugin,generation,expires) VALUES(?,?,?,?,?)",
    )
    .bind(credentials::hash(&raw))
    .bind(&who.device)
    .bind(plugin)
    .bind(generation)
    .bind(expires)
    .execute(db)
    .await?;
    Ok(json!({"proof":raw,"scope":"connector","expires_at":credentials::stamp(expires)?}))
}

fn view(device: &str, plugin: &str, epoch: i64, expires: i64, closed: bool) -> Result<Value> {
    Ok(
        json!({"device_id":device,"purpose":"connector","connection_epoch":epoch,
        "journal_namespace":format!("connector:{}",credentials::hash(&json!([plugin,device]).to_string())),
        "expires_at":credentials::stamp(expires)?,"closed":closed}),
    )
}

pub(super) async fn callback(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    method: &str,
    args: &Value,
) -> Result<Value> {
    super::super::domain::active(db, plugin, generation, "device.connections").await?;
    let at = credentials::now()?;
    if method == "device.connect" {
        let raw = args["proof"].as_str().ok_or("proof_required")?;
        let device:String=sqlx::query_scalar("UPDATE connector_proofs SET used=?1 WHERE hash=?2 AND plugin=?3 AND generation=?4 AND used IS NULL AND expires>?1 RETURNING device")
            .bind(at).bind(credentials::hash(raw)).bind(plugin).bind(generation).fetch_optional(&mut *db).await?.ok_or("proof_unavailable")?;
        credentials::device(db, &device, Purpose::Connector).await?;
        let previous: Option<i64> =
            sqlx::query_scalar("SELECT epoch FROM connector_epochs WHERE plugin=? AND device=?")
                .bind(plugin)
                .bind(&device)
                .fetch_optional(&mut *db)
                .await?;
        let epoch = previous
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("connection_epoch_exhausted")?;
        sqlx::query("INSERT INTO connector_epochs VALUES(?,?,?) ON CONFLICT(plugin,device) DO UPDATE SET epoch=excluded.epoch").bind(plugin).bind(&device).bind(epoch).execute(&mut *db).await?;
        let handle = credentials::random::<32>()?;
        let expires = at + 300;
        sqlx::query("INSERT INTO connector_connections(hash,device,plugin,generation,epoch,expires) VALUES(?,?,?,?,?,?)")
            .bind(credentials::hash(&handle)).bind(&device).bind(plugin).bind(generation).bind(epoch).bind(expires).execute(db).await?;
        let mut result = view(&device, plugin, epoch, expires, false)?;
        result["handle"] = json!(handle);
        return Ok(result);
    }
    let handle = args["handle"].as_str().ok_or("handle_required")?;
    let hash = credentials::hash(handle);
    let row=sqlx::query("SELECT c.device,c.epoch,c.expires,c.closed FROM connector_connections c JOIN connector_epochs e ON e.plugin=c.plugin AND e.device=c.device AND e.epoch=c.epoch WHERE c.hash=? AND c.plugin=? AND c.generation=? AND c.expires>?")
        .bind(&hash).bind(plugin).bind(generation).bind(at).fetch_optional(&mut *db).await?.ok_or("connection_unavailable")?;
    let device: &str = row.get("device");
    credentials::device(db, device, Purpose::Connector).await?;
    let closed: bool = row.get("closed");
    if method == "device.check" && closed {
        return Err("connection_unavailable".into());
    }
    if method == "device.close" {
        sqlx::query("UPDATE connector_connections SET closed=1 WHERE hash=?")
            .bind(hash)
            .execute(&mut *db)
            .await?;
    } else if method != "device.check" {
        return Err("unsupported_connection_operation".into());
    }
    view(
        device,
        plugin,
        row.get("epoch"),
        row.get("expires"),
        method == "device.close",
    )
}
