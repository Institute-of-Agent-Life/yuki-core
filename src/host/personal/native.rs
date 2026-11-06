use super::{
    credentials::{self, Principal, Purpose},
    work,
};
use crate::{Result, work::personal as domain};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub async fn bind(
    conn: &mut SqliteConnection,
    who: &Principal,
    work: &str,
    target: &Value,
) -> Result<()> {
    let dev = target["device_id"].as_str().ok_or("device_required")?;
    let native = credentials::device(conn, dev, Purpose::Connector).await?;
    if native.root != who.root {
        return Err("not_found".into());
    }
    if matches!(
        target["intent"].as_str(),
        Some("steer" | "respond" | "cancel" | "detach")
    ) {
        let op = target["operation_id"]
            .as_str()
            .ok_or("original_operation_required")?;
        sqlx::query_scalar::<_, String>(
            "SELECT work FROM personal_native_operations WHERE device=? AND operation=? AND work=? AND coalesce(selected_entry,(SELECT entry FROM personal_devices WHERE id=chat_device))=?",
        )
        .bind(dev)
        .bind(op)
        .bind(work)
        .bind(&who.entry)
        .fetch_optional(conn)
        .await?
        .ok_or("original_operation_required")?;
        return Ok(());
    }
    if let Some(operation) = target["operation_id"].as_str() {
        if operation.trim().is_empty() || operation.len() > 256 {
            return Err("invalid_operation".into());
        }
        sqlx::query_scalar::<_,String>("INSERT INTO personal_native_operations VALUES(?,?,?,?,?) ON CONFLICT(device,operation) DO UPDATE SET operation=excluded.operation WHERE work=excluded.work AND coalesce(selected_entry,(SELECT entry FROM personal_devices WHERE id=chat_device))=excluded.selected_entry AND chat_device IN(SELECT device_id FROM device_origins WHERE original_person=?) RETURNING operation").bind(dev).bind(operation).bind(work).bind(&who.device).bind(&who.entry).bind(&who.owner).fetch_optional(conn).await?.ok_or("request_conflict")?;
    }
    Ok(())
}
pub async fn proof(
    conn: &mut SqliteConnection,
    who: &Principal,
    plugin: &str,
    work: &str,
    native: bool,
) -> Result<Value> {
    domain::owner(conn, &who.root, work).await?;
    let scope = if native { "native" } else { "work" };
    let grant = if native {
        "native.events"
    } else {
        "personal.work"
    };
    let instance:String=sqlx::query_scalar("SELECT generation FROM host_installations WHERE plugin=? AND enabled AND online AND generation IS NOT NULL AND EXISTS(SELECT 1 FROM json_each(grants) WHERE value=?)").bind(plugin).bind(grant).fetch_optional(&mut *conn).await?.ok_or("plugin_grant_unavailable")?;
    if native
        && !sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM personal_native_operations WHERE device=? AND work=?)",
        )
        .bind(&who.device)
        .bind(work)
        .fetch_one(&mut *conn)
        .await?
    {
        return Err("original_operation_required".into());
    }
    let raw = credentials::random::<32>()?;
    let expires = credentials::now()? + 60;
    sqlx::query("INSERT INTO personal_proofs VALUES(?,?,?,?,?,?,?,NULL,?)")
        .bind(credentials::hash(&raw))
        .bind(&who.device)
        .bind(plugin)
        .bind(&instance)
        .bind(scope)
        .bind(work)
        .bind(expires)
        .bind(&who.entry)
        .execute(conn)
        .await?;
    Ok(json!({"proof":raw,"expires_at":credentials::stamp(expires)?,"scope":scope}))
}
async fn redeem(
    conn: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    raw: &str,
) -> Result<Value> {
    let h = credentials::hash(raw);
    let at = credentials::now()?;
    let row=sqlx::query("UPDATE personal_proofs SET used=?1 WHERE hash=?2 AND plugin=?3 AND generation=?4 AND used IS NULL AND expires>?1 RETURNING device,scope,work,selected_entry").bind(at).bind(h).bind(plugin).bind(generation).fetch_optional(&mut *conn).await?.ok_or("proof_unavailable")?;
    let native = row.get::<&str, _>("scope") == "native";
    super::super::domain::active(
        conn,
        plugin,
        generation,
        if native {
            "native.events"
        } else {
            "personal.work"
        },
    )
    .await?;
    let who = super::selection::delegated(
        conn,
        row.get("device"),
        if native {
            Purpose::Connector
        } else {
            Purpose::Chat
        },
        row.get("selected_entry"),
        true,
    )
    .await?;
    let work: &str = row.get("work");
    domain::owner(conn, &who.root, work).await?;
    let handle = credentials::random::<32>()?;
    let expires = at + 300;
    sqlx::query("INSERT INTO personal_sessions VALUES(?,?,?,?,?,?,?,?)")
        .bind(credentials::hash(&handle))
        .bind(&who.device)
        .bind(plugin)
        .bind(generation)
        .bind(row.get::<&str, _>("scope"))
        .bind(work)
        .bind(expires)
        .bind(&who.entry)
        .execute(conn)
        .await?;
    Ok(json!({"handle":handle,"scope":row.get::<&str,_>("scope"),"expires_in":300}))
}
pub async fn callback(
    conn: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    method: &str,
    args: &Value,
) -> Result<Value> {
    if method == "device.auth" {
        return redeem(
            conn,
            plugin,
            generation,
            args["proof"].as_str().ok_or("proof_required")?,
        )
        .await;
    }
    let native = method.starts_with("native.");
    let row=sqlx::query("SELECT device,work,selected_entry FROM personal_sessions WHERE hash=? AND plugin=? AND generation=? AND scope=? AND expires>?").bind(credentials::hash(args["handle"].as_str().ok_or("handle_required")?)).bind(plugin).bind(generation).bind(if native{"native"}else{"work"}).bind(credentials::now()?).fetch_optional(&mut *conn).await?.ok_or("handle_unavailable")?;
    let who = super::selection::delegated(
        conn,
        row.get("device"),
        if native {
            Purpose::Connector
        } else {
            Purpose::Chat
        },
        row.get("selected_entry"),
        method != "work.get",
    )
    .await?;
    let work_id: &str = row.get("work");
    let manager = domain::owner(conn, &who.root, work_id).await?;
    match method {
        "work.get" => work::project(conn, &who, work_id).await,
        "work.input" | "work.cancel" => {
            work::apply(
                conn,
                &who,
                work_id,
                &args["request"],
                method == "work.cancel",
            )
            .await
        }
        "native.notify" | "native.queryAck" => {
            let operation = args["operation"].as_str().ok_or("operation_required")?;
            let key = args["event_key"].as_str().ok_or("event_key_required")?;
            let (chat,selected):(String,Option<String>)=sqlx::query_as("SELECT chat_device,selected_entry FROM personal_native_operations WHERE device=? AND work=? AND operation=?").bind(&who.device).bind(work_id).bind(operation).fetch_optional(&mut *conn).await?.ok_or("original_operation_required")?;
            let authorized =
                super::selection::delegated(conn, &chat, Purpose::Chat, selected.as_deref(), true)
                    .await?;
            if authorized.root != who.root {
                return Err("not_found".into());
            }
            let old=sqlx::query("SELECT fingerprint,event,watermark FROM native_receipts WHERE device=? AND plugin=? AND operation=? AND event_key=?").bind(&who.device).bind(plugin).bind(operation).bind(key).fetch_optional(&mut *conn).await?;
            let fp = credentials::hash(&args["payload"].to_string());
            if let Some(r) = old {
                if method == "native.notify" && fp != r.get::<&str, _>("fingerprint") {
                    return Err("native_event_conflict".into());
                }
                return Ok(
                    json!({"accepted":true,"watermark":r.get::<i64,_>("watermark"),"event":r.get::<i64,_>("event"),"replay":true}),
                );
            }
            if method == "native.queryAck" {
                return Ok(json!({"accepted":false}));
            }
            let source = format!(
                "native:{}",
                credentials::fingerprint(plugin, operation, &json!([who.device, key]))
            );
            let event = work::action_event(conn, &authorized, &source, "native.notify").await?;
            let status: String = sqlx::query_scalar("SELECT status FROM work_nodes WHERE id=?")
                .bind(work_id)
                .fetch_one(&mut *conn)
                .await?;
            if !matches!(status.as_str(), "cancelled" | "completed" | "failed") {
                domain::input(
                    conn,
                    work_id,
                    &manager,
                    &source,
                    event,
                    &args["payload"].to_string(),
                )
                .await?;
            }
            let watermark:i64=sqlx::query_scalar("INSERT INTO native_receipts SELECT ?1,?2,?3,?4,?5,?6,?7,coalesce(max(watermark),0)+1 FROM native_receipts WHERE device=?1 AND plugin=?2 AND operation=?3 RETURNING watermark")
               .bind(&who.device).bind(plugin).bind(operation).bind(key).bind(fp).bind(event).bind(work_id).fetch_one(conn).await?;
            Ok(json!({"accepted":true,"watermark":watermark,"event":event,"replay":false}))
        }
        _ => Err("unknown personal method".into()),
    }
}
