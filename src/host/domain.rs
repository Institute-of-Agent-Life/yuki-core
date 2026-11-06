use super::{
    Change,
    grants::{self, Install},
};
use crate::{Result, identity::new_id, ingress::admit};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

pub async fn active(
    conn: &mut SqliteConnection,
    id: &str,
    generation: &str,
    grant: &str,
) -> Result<()> {
    let ok:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM host_installations i WHERE plugin=?1 AND generation=?2 AND online AND enabled AND EXISTS(SELECT 1 FROM json_each(i.grants) WHERE value=?3) AND (?3='forget.owner' OR (NOT EXISTS(SELECT 1 FROM host_cleanup_generations c WHERE c.plugin=i.plugin AND c.generation=i.generation) AND NOT EXISTS(SELECT 1 FROM owner_erasures e WHERE e.owner=i.plugin AND e.proof IS NULL))))").bind(id).bind(generation).bind(grant).fetch_one(conn).await?;
    if !ok {
        return Err("plugin instance or grant unavailable".into());
    }
    Ok(())
}
pub(super) async fn close(conn: &mut SqliteConnection, id: &str, generation: &str) -> Result<()> {
    sqlx::query("UPDATE host_installations SET online=0 WHERE plugin=? AND generation=?")
        .bind(id)
        .bind(generation)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE host_calls SET state=CASE WHEN state='queued' THEN 'not_dispatched' ELSE 'unknown' END
        WHERE plugin=? AND generation=? AND state IN ('queued','dispatched')").bind(id).bind(generation).execute(&mut *conn).await?;
    sqlx::query("UPDATE operations SET state='unknown',result='null' WHERE (namespace,operation_id) IN(SELECT execution,operation FROM host_calls WHERE plugin=? AND generation=? AND state='unknown') AND state='dispatched'").bind(id).bind(generation).execute(&mut *conn).await?;
    sqlx::query("UPDATE model_attempts SET state='unknown',ever_unknown=1 WHERE state='dispatched' AND logical IN(SELECT logical FROM host_aux_requests WHERE plugin=? AND dispatched_generation=?)")
        .bind(id).bind(generation).execute(&mut *conn).await?;
    sqlx::query("UPDATE host_command_events SET state=CASE WHEN EXISTS(SELECT 1 FROM host_calls c WHERE c.execution=host_command_events.execution AND c.method='command.invoke' AND c.state='unknown') THEN 'unknown' ELSE 'unavailable' END WHERE plugin=? AND generation=? AND state='queued'").bind(id).bind(generation).execute(conn).await?;
    Ok(())
}
async fn configure(conn: &mut SqliteConnection, installs: &[Install]) -> Result<Value> {
    let previous: Vec<(String, String)> = sqlx::query_as(
        "SELECT plugin,generation FROM host_installations WHERE generation IS NOT NULL",
    )
    .fetch_all(&mut *conn)
    .await?;
    for (id, generation) in previous {
        close(conn, &id, &generation).await?;
    }
    sqlx::query("DELETE FROM core_meta WHERE key LIKE 'plugin-config-definition:%'")
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "UPDATE source_owners SET enabled=0 WHERE id IN(SELECT plugin FROM host_installations)",
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM host_commands")
        .execute(&mut *conn)
        .await?;
    super::plugin_sessions::start(conn).await?;
    let names: Vec<_> = installs.iter().map(|i| &i.id).collect();
    sqlx::query("DELETE FROM core_meta WHERE key LIKE 'plugin-install:%' AND substr(key,16) NOT IN(SELECT value FROM json_each(?))").bind(serde_json::to_string(&names)?).execute(&mut *conn).await?;
    sqlx::query("UPDATE host_installations SET enabled=0 WHERE plugin NOT IN(SELECT value FROM json_each(?))").bind(serde_json::to_string(&names)?).execute(&mut *conn).await?;
    for install in installs {
        let approved = super::lifecycle::configure(conn, install).await?;
        let enabled =
            sqlx::query_scalar::<_, bool>("SELECT enabled FROM host_installations WHERE plugin=?")
                .bind(&install.id)
                .fetch_optional(&mut *conn)
                .await?
                .unwrap_or(install.enabled);
        let decision =
            crate::operator::domain::get(conn, &format!("plugin-approval:{}", install.id))
                .await?
                .ok_or("missing_approval")?;
        register(
            conn,
            install,
            &approved,
            enabled && decision["approved"].is_array(),
        )
        .await?;
    }
    Ok(json!({"configured":names}))
}
pub(super) async fn register(
    conn: &mut SqliteConnection,
    install: &Install,
    approved: &std::collections::BTreeSet<String>,
    enabled: bool,
) -> Result<()> {
    crate::operator::domain::put(
        conn,
        &format!("plugin-install:{}", install.id),
        &serde_json::to_value(install)?,
    )
    .await?;
    sqlx::query("DELETE FROM host_commands WHERE plugin=?")
        .bind(&install.id)
        .execute(&mut *conn)
        .await?;
    super::configuration::configure(conn, install).await?;
    super::aux_model::configure(conn, install).await?;
    super::participation::configure(conn, install).await?;
    super::maintenance::configure(conn, install).await?;
    super::plugin_sessions::configure(conn, install).await?;
    super::notification::configure(conn, install).await?;
    super::composition::configure(conn, install).await?;
    sqlx::query("INSERT INTO host_installations(plugin,namespace,platform,bot_stage,revision,grants,tools,enabled) VALUES(?,?,?,?,?,?,?,?)
          ON CONFLICT(plugin) DO UPDATE SET namespace=excluded.namespace,platform=excluded.platform,bot_stage=excluded.bot_stage,revision=excluded.revision,grants=excluded.grants,tools=excluded.tools,enabled=excluded.enabled")
          .bind(&install.id).bind(&install.namespace).bind(&install.platform).bind(&install.bot_stage).bind(install.revision()?).bind(serde_json::to_string(approved)?).bind(serde_json::to_string(&install.tools)?).bind(enabled).execute(&mut *conn).await?;
    for path in &install.commands {
        sqlx::query("INSERT INTO host_commands VALUES(?,?)")
            .bind(path)
            .bind(&install.id)
            .execute(&mut *conn)
            .await?;
    }
    if approved.contains("forget.owner") {
        sqlx::query("INSERT INTO source_owners(id,enabled) VALUES(?,?) ON CONFLICT(id) DO UPDATE SET enabled=excluded.enabled").bind(&install.id).bind(enabled).execute(&mut *conn).await?;
    }
    Ok(())
}
pub async fn route_command(
    conn: &mut SqliteConnection,
    event: i64,
    f: &admit::Frame,
) -> Result<()> {
    let text = f.text.clone().unwrap_or_else(|| {
        f.segments
            .iter()
            .filter_map(|s| {
                if let admit::Segment::Text { text } = s {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<String>()
    });
    let token = text.split_whitespace().next().unwrap_or("");
    if !token.starts_with('/') {
        return Ok(());
    }
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM host_command_events WHERE event=?)",
    )
    .bind(event)
    .fetch_one(&mut *conn)
    .await?
    {
        return Ok(());
    }
    let binding:Option<(String,String,bool,Option<String>)>=sqlx::query_as("SELECT c.path,c.plugin,i.online AND i.enabled,i.generation FROM host_commands c JOIN host_installations i ON i.plugin=c.plugin WHERE (c.path=? OR substr(?,1,length(c.path)+1)=c.path||'/') AND EXISTS(SELECT 1 FROM json_each(i.grants) WHERE value='command.invoke') ORDER BY length(c.path) DESC LIMIT 1")
        .bind(token).bind(token).fetch_optional(&mut *conn).await?;
    if let Some((path, id, online, generation)) = binding {
        // Claim once under the original human. The Main scheduler excludes this source even when offline.
        let claim = admit::claim(conn, event).await?;
        let exec = claim["execution"]
            .as_str()
            .ok_or("command lacks original execution")?;
        sqlx::query("INSERT INTO host_command_events(event,plugin,path,execution,state,generation) VALUES(?,?,?,?,?,?)")
            .bind(event)
            .bind(id)
            .bind(path)
            .bind(exec)
            .bind(if online { "queued" } else { "unavailable" }).bind(generation)
            .execute(conn)
            .await?;
    }
    Ok(())
}
async fn inbound(
    conn: &mut SqliteConnection,
    id: &str,
    generation: &str,
    p: &Value,
    status: bool,
) -> Result<Value> {
    active(conn, id, generation, "gateway.inbound").await?;
    let (namespace, platform, bot): (String, String, String) = sqlx::query_as(
        "SELECT namespace,platform,bot_stage FROM host_installations WHERE plugin=?",
    )
    .bind(id)
    .fetch_one(&mut *conn)
    .await?;
    let sender = p["sender"].as_str().ok_or("native sender required")?;
    let locator = json!([
        bot,
        p["space"],
        p["topic"],
        sender,
        p["source"],
        p["native_version"]
    ])
    .to_string();
    let key = format!("{:x}", Sha256::digest(locator.as_bytes()));
    if status {
        let (event, accepted): (i64, bool) =
            sqlx::query_as("SELECT event,accepted FROM host_ingress WHERE plugin=? AND source=?")
                .bind(id)
                .bind(key)
                .fetch_one(&mut *conn)
                .await?;
        if crate::privacy::source::is_erased(conn, event).await? {
            return Ok(json!({"event":event,"accepted":false,"erased":true}));
        }
        let mut receipt = json!({"event":event,"accepted":accepted});
        if let Some((path, state)) = sqlx::query_as::<_, (String, String)>(
            "SELECT path,state FROM host_command_events WHERE event=?",
        )
        .bind(event)
        .fetch_optional(conn)
        .await?
        {
            receipt["command"] = json!({"path":path,"state":state});
        }
        return Ok(receipt);
    }
    let bot_ok:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM assignments s JOIN accounts a ON a.id=s.account_id WHERE s.id=? AND s.kind='self' AND s.status='active' AND a.platform=? AND a.namespace=?)").bind(&bot).bind(&platform).bind(&namespace).fetch_one(&mut *conn).await?;
    if !bot_ok {
        return Err("gateway bot does not belong to this installed namespace".into());
    }
    let account = new_id(conn).await?;
    sqlx::query("INSERT INTO accounts(id,platform,namespace,native) VALUES(?,?,?,?) ON CONFLICT(platform,namespace,native) DO NOTHING").bind(account).bind(&platform).bind(&namespace).bind(sender).execute(&mut *conn).await?;
    let account: String =
        sqlx::query_scalar("SELECT id FROM accounts WHERE platform=? AND namespace=? AND native=?")
            .bind(&platform)
            .bind(&namespace)
            .bind(sender)
            .fetch_one(&mut *conn)
            .await?;
    let mut segments: Vec<admit::Segment> = serde_json::from_value(p["segments"].clone())?;
    for segment in &mut segments {
        if let admit::Segment::Mention {
            account, native, ..
        } = segment
        {
            if account.is_some() {
                return Err("gateway cannot supply core account IDs".into());
            }
            if let Some(native) = native {
                *account = sqlx::query_scalar(
                    "SELECT id FROM accounts WHERE platform=? AND namespace=? AND native=?",
                )
                .bind(&platform)
                .bind(&namespace)
                .bind(native.as_str())
                .fetch_optional(&mut *conn)
                .await?;
            }
        }
    }
    let frame = admit::Frame {
        source: key.clone(),
        native_id: p["native_id"].as_str().map(String::from),
        account,
        bot_stage: bot,
        space: p["space"]
            .as_str()
            .map(|space| json!([platform, namespace, space]).to_string()),
        topic: p["topic"].as_str().map(String::from),
        occurred: p["occurred"].as_i64(),
        observed: i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis(),
        )?,
        kind: serde_json::from_value(p["kind"].clone())?,
        segments,
        sender: None,
        answer_work_id: None,
        reply_bot: None,
        text: None,
    };
    let result = admit::from_gateway(conn, frame, p["accept"].as_bool().unwrap_or(false)).await?;
    sqlx::query("INSERT INTO host_ingress VALUES(?,?,?,?) ON CONFLICT(plugin,source) DO UPDATE SET accepted=excluded.accepted WHERE host_ingress.event=excluded.event")
       .bind(id).bind(key).bind(result["event"].as_i64()).bind(result["accepted"]==true).execute(conn).await?;
    Ok(result)
}
pub async fn apply(conn: &mut SqliteConnection, change: &Change) -> Result<Value> {
    match change {
        Change::Reported(event, state) => {
            sqlx::query("UPDATE host_command_events SET reply_state=? WHERE event=? AND state='unavailable' AND reply_state IS NULL").bind(state).bind(event).execute(conn).await?;
            Ok(Value::Null)
        }
        Change::CommandDone(event, state) => {
            sqlx::query("UPDATE host_command_events SET state=? WHERE event=? AND state='queued'")
                .bind(state)
                .bind(event)
                .execute(conn)
                .await?;
            Ok(Value::Null)
        }
        Change::Configure(installs) => configure(conn, installs).await,
        Change::Start(id, cleanup, declaration) => {
            let install = crate::operator::domain::get(conn, &format!("plugin-install:{id}"))
                .await?
                .ok_or("plugin_not_installed")?;
            if crate::memory::scope::hash(&install) != *declaration {
                return Err("plugin installation changed".into());
            }
            let pending: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM owner_erasures WHERE owner=? AND proof IS NULL)",
            )
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            if pending != *cleanup {
                return Err("plugin launch mode disagrees with erasure watermark".into());
            }
            let instance = new_id(conn).await?;
            if sqlx::query(
                "UPDATE host_installations SET generation=?,online=0 WHERE plugin=? AND enabled",
            )
            .bind(&instance)
            .bind(id)
            .execute(&mut *conn)
            .await?
            .rows_affected()
                != 1
            {
                return Err("plugin disabled".into());
            }
            if *cleanup {
                sqlx::query("INSERT INTO host_cleanup_generations VALUES(?,?)")
                    .bind(id)
                    .bind(&instance)
                    .execute(&mut *conn)
                    .await?;
            }
            Ok(json!(instance))
        }
        Change::Online(id, instance) => {
            if sqlx::query("UPDATE host_installations SET online=1 WHERE plugin=? AND generation=? AND enabled AND (NOT EXISTS(SELECT 1 FROM owner_erasures e WHERE e.owner=host_installations.plugin AND e.proof IS NULL) OR EXISTS(SELECT 1 FROM host_cleanup_generations c WHERE c.plugin=host_installations.plugin AND c.generation=host_installations.generation))").bind(id).bind(instance).execute(conn).await?.rows_affected()!=1{return Err("plugin instance superseded".into());}
            Ok(Value::Null)
        }
        Change::Close(id, instance) => {
            close(conn, id, instance).await?;
            Ok(Value::Null)
        }
        Change::Enable(id, enabled) => {
            if *enabled {
                let approval = crate::operator::domain::get(conn, &format!("plugin-approval:{id}"))
                    .await?
                    .ok_or("precondition_failed")?;
                if !approval["approved"].is_array()
                    || crate::operator::domain::get(conn, &format!("plugin-install:{id}"))
                        .await?
                        .is_none()
                {
                    return Err("precondition_failed".into());
                }
            }
            let g: Option<String> =
                sqlx::query_scalar("SELECT generation FROM host_installations WHERE plugin=?")
                    .bind(id)
                    .fetch_one(&mut *conn)
                    .await?;
            if !enabled && let Some(g) = g {
                close(conn, id, &g).await?;
            }
            sqlx::query("UPDATE host_installations SET enabled=? WHERE plugin=?")
                .bind(enabled)
                .bind(id)
                .execute(&mut *conn)
                .await?;
            sqlx::query("UPDATE source_owners SET enabled=? AND EXISTS(SELECT 1 FROM host_installations,json_each(grants) WHERE plugin=? AND value='forget.owner') WHERE id=?")
                .bind(enabled)
                .bind(id)
                .bind(id)
                .execute(conn)
                .await?;
            Ok(json!({"enabled":enabled}))
        }
        Change::Callback(id, instance, method, params) => {
            let def = grants::schema(method, "plugin_to_host", params)?;
            active(conn, id, instance, def["grant"].as_str().unwrap()).await?;
            let result = match method.as_str() {
                "plugin.config.get" | "plugin.secrets.get" => {
                    super::configuration::callback(conn, id, method, params).await
                }
                "gateway.inbound" | "gateway.status" => {
                    inbound(conn, id, instance, params, method == "gateway.status").await
                }
                "tool.status" | "gateway.part_status" => {
                    super::calls::status(
                        conn,
                        id,
                        instance,
                        params["handle"].as_str().unwrap(),
                        method,
                    )
                    .await
                }
                "media.open" | "media.publish" => {
                    super::media::callback(conn, id, instance, method, params).await
                }
                "device.auth" | "device.connect" | "device.check" | "device.close" | "work.get"
                | "work.input" | "work.cancel" | "native.notify" | "native.queryAck" => {
                    super::personal::callback(conn, id, instance, method, params).await
                }
                "forget.ack" => {
                    crate::privacy::apply(
                        conn,
                        &crate::privacy::Action::Ack {
                            owner: id.clone(),
                            key: params["key"].as_str().unwrap().into(),
                            watermark: params["watermark"].as_i64().ok_or("invalid watermark")?,
                            proof: params["proof"].as_str().unwrap().into(),
                        },
                    )
                    .await
                }
                _ => Err("undeclared plugin method".into()),
            }?;
            if !jsonschema::validator_for(&def["result"])?.is_valid(&result) {
                return Err("host result violates advertised schema".into());
            }
            Ok(result)
        }
        Change::PluginSession(change) => super::plugin_sessions::apply(conn, change).await,
        Change::Notification(change) => super::notification::apply(conn, change).await,
        Change::Maintenance(change) => super::maintenance::apply(conn, change).await,
        Change::Participation(change) => super::participation::apply(conn, change).await,
        Change::AuxModel(change) => super::aux_model::apply(conn, change).await,
        Change::Call(call) => super::calls::apply(conn, call).await,
        Change::Composition(change) => super::composition::apply(conn, change).await,
    }
}
