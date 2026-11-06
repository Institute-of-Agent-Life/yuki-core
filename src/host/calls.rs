use super::{domain, grants};
use crate::{Result, identity::new_id, privacy::source, runtime::effects};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

#[derive(Clone)]
pub struct Request {
    pub vision: bool,
    pub plugin: String,
    pub revision: String,
    pub execution: String,
    pub operation: String,
    pub method: String,
    pub name: String,
    pub args: Value,
    pub parent: Option<String>,
    pub ordinal: Option<i64>,
}
pub enum Change {
    Open(Request),
    Dispatch {
        handle: String,
        plugin: String,
        generation: String,
    },
    Settle {
        handle: String,
        plugin: String,
        generation: String,
        body: Value,
        error: bool,
    },
    Unsent {
        handle: String,
        generation: String,
    },
}
async fn actor(conn: &mut SqliteConnection, exec: &str) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT coalesce(original_actor,origin) FROM executions WHERE id=?")
            .bind(exec)
            .fetch_one(conn)
            .await?,
    )
}
async fn gate(conn: &mut SqliteConnection, r: &Request, id: &str, generation: &str) -> Result<()> {
    let grant = grants::methods()["methods"]
        .get(&r.method)
        .and_then(|v| v["grant"].as_str())
        .ok_or("unknown delegated method")?;
    domain::active(conn, id, generation, grant).await?;
    let who = actor(conn, &r.execution).await?;
    effects::authorize(conn, &r.execution, &who, true).await?;
    if r.method == "command.invoke" {
        let bound:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM host_command_events WHERE execution=? AND path=? AND plugin=? AND generation=? AND state='queued')").bind(&r.execution).bind(&r.name).bind(id).bind(generation).fetch_one(&mut *conn).await?;
        if !bound {
            return Err("command has no original matched source".into());
        }
    }
    if r.method == "tool.call" {
        let frozen:String=sqlx::query_scalar("SELECT m.body FROM execution_tool_views v JOIN tool_manifests m USING(revision) WHERE v.execution=?").bind(&r.execution).fetch_one(&mut *conn).await?;
        let manifest: crate::tools::registry::Manifest = serde_json::from_str(&frozen)?;
        let tool = manifest.0.get(&r.name).ok_or("unknown original tool")?;
        if tool
            .provider
            .as_ref()
            .is_none_or(|p| p.id != id || p.revision != r.revision)
            || !jsonschema::validator_for(&tool.schema)?.is_valid(&r.args)
        {
            return Err("plugin call outside original execution manifest".into());
        }
    }
    if r.method.starts_with("gateway.") {
        let mut own:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM host_installations i JOIN executions x ON x.id=? JOIN events e ON e.id=x.source AND e.bot_stage=i.bot_stage WHERE i.plugin=? AND x.original_actor=e.raw_actor)").bind(&r.execution).bind(id).fetch_one(&mut *conn).await?;
        let scene = match super::participation::self_binding(conn, &r.execution).await? {
            Some(scene) => Some(scene),
            None => super::notification::binding(conn, &r.execution).await?,
        };
        if let Some((_, entry, receiver)) = scene {
            own = r.args["route"]["entry"]==entry && sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM host_installations i JOIN assignments s ON s.id=i.bot_stage JOIN accounts a ON a.id=s.account_id WHERE i.plugin=? AND i.bot_stage=? AND a.native=?)")
                .bind(id).bind(receiver).bind(r.args["route"]["bot"].as_str()).fetch_one(&mut *conn).await?;
        }
        if !own {
            return Err("gateway delivery requires original gateway source and route".into());
        }
    }
    Ok(())
}
pub async fn status(
    conn: &mut SqliteConnection,
    id: &str,
    generation: &str,
    h: &str,
    method: &str,
) -> Result<Value> {
    let row=sqlx::query("SELECT c.*,i.revision AS current_revision FROM host_calls c JOIN host_installations i ON i.plugin=c.plugin WHERE c.handle=? AND c.plugin=?").bind(h).bind(id).fetch_one(&mut *conn).await?;
    let exec: String = row.try_get("execution")?;
    let who = actor(conn, &exec).await?;
    effects::authorize(conn, &exec, &who, false).await?;
    let kind: String = row.try_get("method")?;
    if !matches!(
        (method, kind.as_str()),
        ("tool.status", "tool.call") | ("gateway.part_status", "gateway.dispatch_part")
    ) || row.try_get::<String, _>("revision")? != row.try_get::<String, _>("current_revision")?
    {
        return Err("handle outside method or original manifest".into());
    }
    if super::terminal_work::receipt(conn, h, id, generation).await? {
        return Ok(json!({"handle":h,"state":row.try_get::<String,_>("state")?,"result":null}));
    }
    Ok(
        json!({"handle":h,"state":row.try_get::<String,_>("state")?,"result":row.try_get::<Option<String>,_>("result")?.as_deref().map(serde_json::from_str::<Value>).transpose()?}),
    )
}
pub async fn apply(conn: &mut SqliteConnection, change: &Change) -> Result<Value> {
    match change {
        Change::Open(r) => {
            let install=sqlx::query("SELECT revision,generation FROM host_installations WHERE plugin=? AND enabled AND online").bind(&r.plugin).fetch_one(&mut *conn).await?;
            let revision: String = install.try_get("revision")?;
            let generation: String = install.try_get("generation")?;
            if revision != r.revision {
                return Err("plugin manifest differs from paid revision".into());
            }
            gate(conn, r, &r.plugin, &generation).await?;
            let who = actor(conn, &r.execution).await?;
            let selected = super::selected_processor::selected(r);
            let payload = if selected {
                super::selected_processor::original_payload(conn, r).await?
            } else {
                r.args.to_string()
            };
            effects::apply(
                conn,
                &effects::Action::Intent {
                    execution: r.execution.clone(),
                    id: r.operation.clone(),
                    actor: who.clone(),
                    action: effects::Operation::External,
                    target: r.name.clone(),
                    payload,
                    parent: r.parent.clone(),
                    ordinal: r.ordinal,
                },
            )
            .await?;
            let original=sqlx::query("SELECT handle,plugin,revision,method,state FROM host_calls WHERE execution=? AND operation=?").bind(&r.execution).bind(&r.operation).fetch_optional(&mut *conn).await?;
            let handle = if let Some(row) = original {
                if row.try_get::<String, _>("plugin")? != r.plugin
                    || row.try_get::<String, _>("revision")? != revision
                    || row.try_get::<String, _>("method")? != r.method
                {
                    return Err("original plugin call conflicts".into());
                }
                let h: String = row.try_get("handle")?;
                if row.try_get::<String, _>("state")? == "not_dispatched" {
                    sqlx::query("UPDATE host_calls SET state='queued',generation=? WHERE handle=?")
                        .bind(&generation)
                        .bind(&h)
                        .execute(&mut *conn)
                        .await?;
                } else {
                    return Ok(
                        json!({"handle":h,"state":row.try_get::<String,_>("state")?,"send":false}),
                    );
                }
                h
            } else {
                let h = new_id(conn).await?;
                sqlx::query("INSERT INTO host_calls VALUES(?,?,?,?,?,?,?,?, 'queued',NULL)")
                    .bind(&h)
                    .bind(&r.plugin)
                    .bind(&revision)
                    .bind(&generation)
                    .bind(&r.execution)
                    .bind(&r.operation)
                    .bind(&r.method)
                    .bind(r.args.to_string())
                    .execute(&mut *conn)
                    .await?;
                h
            };
            if r.method == "tool.call" {
                super::terminal_work::open(conn, r, &handle, &generation).await?;
                super::composition::open(conn, r, &handle).await?;
                super::media::retain_inputs(conn, r, &handle).await?;
                sqlx::query("INSERT OR IGNORE INTO host_media_profiles VALUES(?,?)")
                    .bind(&handle)
                    .bind(r.vision)
                    .execute(&mut *conn)
                    .await?;
            }
            if selected {
                super::selected_processor::record(conn, r, &handle).await?;
                sqlx::query("INSERT OR IGNORE INTO host_media_profiles VALUES(?,?)")
                    .bind(&handle)
                    .bind(r.vision)
                    .execute(&mut *conn)
                    .await?;
            }
            let params = json!({"handle":handle,"operation":r.operation,"parent":r.parent,"ordinal":r.ordinal,"revision":revision,"name":r.name,"arguments":r.args});
            grants::schema(&r.method, "host_to_plugin", &params)?;
            if r.method=="media.process" || (r.method=="gateway.dispatch_part" && r.args.get("media").is_some()) || (r.method=="tool.call" && r.args.get("media").is_some() && sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM host_installations i,json_each(i.grants) g WHERE i.plugin=? AND g.value='media.open')").bind(&r.plugin).fetch_one(&mut *conn).await?) {
                super::media::select(conn,r,&handle,&who).await?;
            }
            Ok(
                json!({"handle":handle,"generation":generation,"params":params,"state":"queued","send":true}),
            )
        }
        Change::Dispatch {
            handle,
            plugin,
            generation,
        } => {
            let row=sqlx::query("SELECT c.execution,c.operation,c.method FROM host_calls c JOIN host_installations i ON i.plugin=c.plugin AND i.revision=c.revision WHERE c.handle=? AND c.plugin=? AND c.generation=? AND c.state='queued'").bind(handle).bind(plugin).bind(generation).fetch_one(&mut *conn).await?;
            let method: String = row.try_get("method")?;
            let exec: String = row.try_get("execution")?;
            let who = actor(conn, &exec).await?;
            domain::active(
                conn,
                plugin,
                generation,
                grants::methods()["methods"][&method]["grant"]
                    .as_str()
                    .ok_or("unknown grant")?,
            )
            .await?;
            super::terminal_work::check(conn, handle, plugin, generation).await?;
            super::selected_processor::check(conn, handle).await?;
            let resume=sqlx::query("UPDATE code_resume_attempts SET state='dispatched' WHERE handle=? AND generation=? AND state='queued'").bind(handle).bind(generation).execute(&mut *conn).await?.rows_affected()==1;
            if resume {
                domain::active(conn, plugin, generation, "composition.execute").await?;
                effects::authorize(conn, &exec, &who, true).await?;
            } else {
                let permit = effects::apply(
                    conn,
                    &effects::Action::Dispatch {
                        execution: exec,
                        id: row.try_get("operation")?,
                        actor: who,
                    },
                )
                .await?;
                if permit["dispatch_now"] != true {
                    return Err("operation already crossed dispatch boundary".into());
                }
            }
            sqlx::query("UPDATE host_calls SET state='dispatched' WHERE handle=?")
                .bind(handle)
                .execute(conn)
                .await?;
            Ok(Value::Null)
        }
        Change::Unsent { handle, generation } => {
            sqlx::query("UPDATE host_calls SET state='not_dispatched' WHERE handle=? AND generation=? AND state='queued'").bind(handle).bind(generation).execute(conn).await?;
            Ok(Value::Null)
        }
        Change::Settle {
            handle,
            plugin,
            generation,
            body,
            error,
        } => {
            let row=sqlx::query("SELECT c.execution,c.operation,c.method,c.state,c.args,i.enabled,i.online,i.generation=c.generation AS same_generation FROM host_calls c JOIN host_installations i ON i.plugin=c.plugin
                WHERE c.handle=? AND c.plugin=? AND c.generation=?").bind(handle).bind(plugin).bind(generation).fetch_one(&mut *conn).await?;
            let exec: String = row.try_get("execution")?;
            let original_state: String = row.try_get("state")?;
            if !matches!(original_state.as_str(), "dispatched" | "unknown") {
                return Err("response is not for a dispatched call".into());
            }
            let method: String = row.try_get("method")?;
            let permitted = row.try_get::<bool, _>("enabled")?
                && row.try_get::<bool, _>("online")?
                && row.try_get::<bool, _>("same_generation")?;
            let who = actor(conn, &exec).await?;
            let publish = permitted
                && super::terminal_work::check(conn, handle, plugin, generation)
                    .await
                    .is_ok()
                && super::selected_processor::check(conn, handle).await.is_ok()
                && effects::authorize(conn, &exec, &who, true).await.is_ok()
                && !source::execution_erased(conn, &exec, None).await?;
            let valid = !*error
                && jsonschema::validator_for(&grants::methods()["methods"][&method]["result"])?
                    .is_valid(body);
            let state = if *error || !valid {
                "unknown"
            } else if method == "gateway.dispatch_part" {
                body["state"].as_str().unwrap_or("unknown")
            } else if body["ok"] == false {
                "failed"
            } else {
                "confirmed"
            };
            let data = if publish && valid {
                body.clone()
            } else {
                json!({"unavailable":true})
            };
            if original_state == "unknown" {
                let saved = if publish && valid {
                    Some(data.to_string())
                } else {
                    None
                };
                if let Some((old, raw)) = sqlx::query_as::<_, (String, Option<String>)>(
                    "SELECT observed_state,body FROM host_late_receipts WHERE handle=?",
                )
                .bind(handle)
                .fetch_optional(&mut *conn)
                .await?
                {
                    if old != state || raw.is_some() && raw != saved {
                        return Err("conflicting late host receipt".into());
                    }
                } else {
                    sqlx::query("INSERT INTO host_late_receipts VALUES(?,?,?)")
                        .bind(handle)
                        .bind(state)
                        .bind(saved)
                        .execute(conn)
                        .await?;
                }
                return Ok(json!({"handle":handle,"state":"unknown","observed_state":state}));
            }
            let mut data = data;
            if publish && valid {
                super::composition::finished(conn, handle, &mut data).await?;
            }
            if state == "confirmed" && publish {
                super::media::project(conn, handle, &exec, &who, &method, &mut data).await?;
            }
            let outcome = match state {
                "confirmed" => effects::Outcome::Confirmed,
                "failed" => effects::Outcome::Failed,
                _ => effects::Outcome::Unknown,
            };
            effects::apply(
                conn,
                &effects::Action::Settle {
                    execution: exec.clone(),
                    id: row.try_get("operation")?,
                    outcome,
                    result: data.to_string(),
                },
            )
            .await?;
            if publish && state == "confirmed" && method == "gateway.dispatch_part" {
                let args: Value = serde_json::from_str(row.try_get("args")?)?;
                let entry = args["route"]["entry"]
                    .as_str()
                    .ok_or("missing original delivery route")?;
                if let Some(version) = args["media"]["artifact_id"].as_str() {
                    sqlx::query("INSERT OR IGNORE INTO social_part_versions SELECT ?1,?2,v.id FROM media_versions v JOIN host_media_selected sel ON sel.version=v.id AND sel.digest=v.digest WHERE sel.handle=?3 AND v.id=?4").bind(&exec).bind(row.get::<String,_>("operation")).bind(handle).bind(version).execute(&mut *conn).await?;
                }
                if let Some((origin, scene, stage)) =
                    super::participation::self_binding(conn, &exec).await?
                {
                    if scene != entry {
                        return Err("SELF delivery scene changed".into());
                    }
                    sqlx::query("INSERT INTO social_outbound(stage,entry,locator,namespace,child,original_actor,body,observed_at) VALUES(?,?,?,?,?,?,?,CAST(unixepoch('subsec')*1000 AS INTEGER))")
                        .bind(stage).bind(entry).bind(body["locator"].as_str()).bind(&exec).bind(row.try_get::<String,_>("operation")?).bind(origin).bind(args["part"].to_string()).execute(&mut *conn).await?;
                } else {
                    sqlx::query("INSERT INTO social_outbound(stage,entry,locator,namespace,child,original_actor,body,observed_at) SELECT e.bot_stage,e.entry_id,?,x.id,?,e.raw_actor,?,CAST(unixepoch('subsec')*1000 AS INTEGER) FROM executions x JOIN events e ON e.id=x.source WHERE x.id=? AND e.entry_id=?")
                    .bind(body["locator"].as_str()).bind(row.try_get::<String,_>("operation")?).bind(args["part"].to_string()).bind(row.try_get::<String,_>("execution")?).bind(entry).execute(&mut *conn).await?;
                }
            }
            sqlx::query("UPDATE host_calls SET state=?,result=? WHERE handle=?")
                .bind(state)
                .bind(if publish {
                    Some(data.to_string())
                } else {
                    None
                })
                .bind(handle)
                .execute(conn)
                .await?;
            Ok(json!({"handle":handle,"state":state,"result":if publish{data}else{Value::Null}}))
        }
    }
}
