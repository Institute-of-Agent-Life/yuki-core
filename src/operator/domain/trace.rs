//! Project original execution facts. Provider secrets and transport media are omitted.
use super::*;
use sqlx::Row;

const EXPORT_SCHEMA: &str = "yuki.trace.export/1";

fn redact(value: &Value, path: &str, paths: &mut Vec<String>, depth: usize, export: bool) -> Value {
    if depth > 48 {
        paths.push(path.into());
        return json!({"trace_omitted":"structure_limit"});
    }
    match value {
        Value::Object(map) => Value::Object(map.iter().map(|(key, value)| {
            let here = format!("{path}.{key}");
            let normalized = key.to_ascii_lowercase().replace(['_','-'], "");
            let kind = map.get("type").and_then(Value::as_str).unwrap_or("");
            let sensitive = ["encryptedcontent","signature","thoughtsignature","apikey","authorization","token","accesstoken","refreshtoken","password","secret","code","pairingcode","proof","cookie"].contains(&normalized.as_str())
                || (["redacted_thinking","reasoning.encrypted"].contains(&kind) && key=="data");
            let media = ["inlinedata","filedata","dataurl","imageurl"].contains(&normalized.as_str())
                || (["image","input_image","input_audio"].contains(&kind) && ["source","url","data"].contains(&key.as_str()));
            let value = if sensitive || media {
                paths.push(here.clone());
                // Opaque provider data retains a digest, credential material does not become a guessing oracle.
                if ["signature","thoughtsignature","encryptedcontent"].contains(&normalized.as_str()) || media {
                    json!({"trace_omitted":if media {"transport_media"} else {"opaque_provider_state"},"sha256":crate::host::personal::credentials::hash(&value.to_string()),"bytes":value.to_string().len()})
                } else { json!({"trace_omitted":"credential_material"}) }
            } else { redact(value, &here, paths, depth+1, export) };
            (key.clone(),value)
        }).collect()),
        Value::Array(items) => Value::Array(items.iter().enumerate().map(|(i,v)|redact(v,&format!("{path}[{i}]"),paths,depth+1,export)).collect()),
        Value::String(text) if matches!(text.trim_start().chars().next(), Some('{' | '[')) => {
            if text.len() > 6144 {
                paths.push(path.into());
                return json!({"trace_omitted":"encoded_structure_limit"});
            }
            if let Ok(nested @ (Value::Object(_) | Value::Array(_))) = serde_json::from_str::<Value>(text) {
                // Provider tool arguments and tool-message receipts may themselves
                // be encoded JSON. Preserve the string shape after redaction.
                Value::String(redact(&nested, path, paths, depth + 1, export).to_string())
            } else if export {
                paths.push(path.into());
                json!({"trace_omitted":"unparsed_structure"})
            } else { value.clone() }
        }
        Value::String(text) if export && (text.contains("://") || text.contains("data:")) => {
            paths.push(path.into());
            json!({"trace_omitted":"transport_reference"})
        }
        _ => value.clone(),
    }
}
async fn payload(
    db: &mut SqliteConnection,
    execution: &str,
    kind: &str,
    operation: Option<&str>,
    attempt: Option<&str>,
    state: &str,
) -> Result<Option<Value>> {
    let mut data = json!({"state":state});
    match kind {
        "tool" => {
            let Some(row) = sqlx::query(
                "SELECT target,payload,result FROM operations WHERE namespace=? AND operation_id=?",
            )
            .bind(execution)
            .bind(operation)
            .fetch_optional(db)
            .await?
            else {
                return Ok(None);
            };
            data["tool"] = json!(row.try_get::<String, _>("target")?);
            data["arguments"] = serde_json::from_str(row.try_get("payload")?)?;
            if matches!(state, "confirmed" | "failed") {
                data["result"] = row
                    .try_get::<Option<String>, _>("result")?
                    .map(|v| serde_json::from_str::<Value>(&v))
                    .transpose()?
                    .unwrap_or(Value::Null);
            }
        }
        "model" => {
            let Some(row) = sqlx::query("SELECT r.request_json,a.response,a.usage_json,a.status_code FROM model_attempts a JOIN model_requests r ON r.id=a.logical WHERE a.id=?")
                .bind(attempt).fetch_optional(db).await? else { return Ok(None); };
            data["request"] = serde_json::from_str(row.try_get("request_json")?)?;
            if matches!(state, "response" | "response_held" | "paired" | "rejected") {
                for (key, column) in [("response", "response"), ("usage", "usage_json")] {
                    data[key] = row
                        .try_get::<Option<String>, _>(column)?
                        .map(|v| serde_json::from_str::<Value>(&v))
                        .transpose()?
                        .unwrap_or(Value::Null);
                }
                data["http_status"] = json!(row.try_get::<Option<i64>, _>("status_code")?);
            }
        }
        _ => (),
    }
    let mut paths = Vec::new();
    let mut data = redact(&data, "$", &mut paths, 0, false);
    // Bound independent sections so a large tool declaration never hides a small response.
    for (key, value) in data
        .as_object_mut()
        .ok_or("invalid_trace_payload")?
        .iter_mut()
    {
        let raw = serde_json::to_vec(value)?;
        if raw.len() > 6144 {
            paths.push(format!("$.{key}"));
            *value = json!({"trace_omitted":"size","sha256":crate::host::personal::credentials::hash(std::str::from_utf8(&raw)?),"bytes":raw.len()});
        }
    }
    let value = json!({"data":data,"redactions":paths});
    if serde_json::to_vec(&value)?.len() > 16384 {
        return Ok(None);
    }
    Ok(Some(value))
}
pub(super) async fn read(
    db: &mut SqliteConnection,
    args: &Value,
    authority: &Authority,
) -> Result<Value> {
    projection(db, args, authority, false).await
}

async fn projection(
    db: &mut SqliteConnection,
    args: &Value,
    authority: &Authority,
    export: bool,
) -> Result<Value> {
    let kind = args["kind"].as_str().ok_or("validation_error")?;
    let id = args["identity"].as_str().ok_or("validation_error")?;
    let after = args["after"].as_i64().unwrap_or(0);
    let limit = args["limit"].as_i64().unwrap_or(100);
    if after < 0 || !(1..=250).contains(&limit) {
        return Err("validation_error".into());
    }
    let selector = match kind {
        "turn" => "t.execution=?1",
        "work" => "coalesce(w.work,x.work_id)=?1",
        "event" => {
            "(x.source=?1 OR EXISTS(SELECT 1 FROM main_dependencies d WHERE d.execution=x.id AND d.event=?1))"
        }
        _ => return Err("validation_error".into()),
    };
    if kind == "event" && id.parse::<i64>().map_or(true, |n| n < 1) {
        return Err("validation_error".into());
    }
    let sql = format!(
        "SELECT t.*,x.source,coalesce(w.work,x.work_id) AS work,coalesce(m.conversation,e.entry_id,o.conversation) AS conversation,m.window,o.kind AS origin FROM control_trace_entries t JOIN executions x ON x.id=t.execution LEFT JOIN main_turns m ON m.execution=x.id LEFT JOIN events e ON e.id=x.source LEFT JOIN main_origins o ON o.ref=x.origin LEFT JOIN work_runs w ON w.execution=x.id WHERE {selector} AND t.id>?2 AND t.expires>unixepoch() AND NOT EXISTS(SELECT 1 FROM invalidated_sources s WHERE s.source=x.source) AND NOT EXISTS(SELECT 1 FROM main_dependencies d JOIN invalidated_sources s ON s.source=d.event WHERE d.execution=x.id) ORDER BY t.id LIMIT ?3"
    );
    let rows = sqlx::query(&sql)
        .bind(id)
        .bind(after)
        .bind(limit + 1)
        .fetch_all(&mut *db)
        .await?;
    let more = rows.len() > limit as usize;
    let content = args["content"].as_bool().unwrap_or(false);
    let mut items = Vec::new();
    let mut page_executions = std::collections::BTreeSet::new();
    for row in rows.iter().take(limit as usize) {
        let execution: &str = row.try_get("execution")?;
        page_executions.insert(execution);
        let operation: Option<String> = row.try_get("operation")?;
        let attempt: Option<String> = row.try_get("attempt")?;
        let kind: String = row.try_get("kind")?;
        let state: String = row.try_get("state")?;
        let mut item = json!({"id":row.try_get::<i64,_>("id")?,"conversation_id":row.try_get::<Option<String>,_>("conversation")?,
            "turn_id":execution,"work_id":row.try_get::<Option<String>,_>("work")?,"operation_id":operation.as_ref().or(attempt.as_ref()),
            "source_event_id":row.try_get::<Option<i64>,_>("source")?,"delivered_event_id":null,"kind":kind,
            "origin":row.try_get::<Option<String>,_>("origin")?.unwrap_or_else(||"human".into()),"generation":row.try_get::<Option<i64>,_>("window")?,
            "created_at":crate::host::personal::credentials::stamp(row.try_get("created")?)?,"expires_at":crate::host::personal::credentials::stamp(row.try_get("expires")?)?,"payload_status":"available"});
        if content {
            match payload(
                db,
                execution,
                &kind,
                operation.as_deref(),
                attempt.as_deref(),
                &state,
            )
            .await
            {
                Ok(Some(body)) => item["payload"] = body,
                Ok(None) => {
                    item["payload"] = Value::Null;
                    item["payload_status"] = json!("omitted_size_or_unavailable");
                }
                Err(_) => return Err("state_mismatch".into()),
            }
        }
        items.push(item);
    }
    let next = if more {
        items.last().map(|v| v["id"].clone())
    } else {
        None
    };
    if let Authority::Invocation { handle, .. } = authority
        && !page_executions.is_empty()
    {
        // Commit the sources for the projected page before returning it to the plugin.
        // The writer will then fence late results and scrub previously saved copies.
        sqlx::query(
            "WITH page(id) AS (SELECT value FROM json_each(?1)),
                  sources(event) AS (
                   SELECT x.source FROM page JOIN executions x USING(id) WHERE x.source IS NOT NULL
                   UNION SELECT d.event FROM page JOIN main_dependencies d ON d.execution=page.id
                  )
             INSERT OR IGNORE INTO main_dependencies(execution,event)
             SELECT c.execution,s.event FROM host_calls c CROSS JOIN sources s WHERE c.handle=?2",
        )
        .bind(serde_json::to_string(&page_executions)?)
        .bind(handle)
        .execute(db)
        .await?;
    }
    if export {
        for item in &mut items {
            let Some(payload) = item.get_mut("payload").filter(|value| value.is_object()) else {
                continue;
            };
            let mut paths = Vec::new();
            payload["data"] = redact(&payload["data"], "$", &mut paths, 0, true);
            if let Some(redactions) = payload["redactions"].as_array_mut() {
                redactions.extend(paths.into_iter().map(Value::String));
            }
        }
    }
    Ok(json!({"items":items,"next_cursor":next}))
}

pub(super) async fn export(
    db: &mut SqliteConnection,
    args: &Value,
    authority: &Authority,
) -> Result<Value> {
    let Authority::Invocation { handle, .. } = authority else {
        return Err("trace export requires an original Host invocation".into());
    };
    if args.as_object().is_none_or(|fields| {
        fields.keys().any(|key| {
            !matches!(
                key.as_str(),
                "kind" | "identity" | "after" | "limit" | "content"
            )
        })
    }) || args["identity"]
        .as_str()
        .is_none_or(|value| value.len() > 128)
        || args.get("content").is_some_and(|value| !value.is_boolean())
        || args
            .get("after")
            .is_some_and(|value| value.as_i64().is_none())
        || args.get("limit").is_some_and(|value| {
            value
                .as_i64()
                .is_none_or(|limit| !(1..=20).contains(&limit))
        })
    {
        return Err("invalid trace export arguments".into());
    }
    let (exec, operation): (String, String) = sqlx::query_as(
        "SELECT execution,operation FROM host_calls WHERE handle=? AND method='command.invoke'",
    )
    .bind(handle)
    .fetch_one(&mut *db)
    .await?;
    let mut bounded = args.clone();
    bounded["limit"] = json!(args["limit"].as_i64().unwrap_or(20));
    bounded["after"] = json!(args["after"].as_i64().unwrap_or(0));
    bounded["content"] = json!(args["content"].as_bool().unwrap_or(false));
    let prior: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT handle,version,body FROM tool_artifacts WHERE execution=? AND operation=?",
    )
    .bind(&exec)
    .bind(&operation)
    .fetch_optional(&mut *db)
    .await?;
    if let Some((artifact, version, body)) = prior {
        let saved: Value =
            serde_json::from_str(body.as_deref().ok_or("original trace export erased")?)?;
        let original = &saved["result"];
        if original["schema"] != EXPORT_SCHEMA || original["selector"] != bounded {
            return Err("original trace export selector changed".into());
        }
        let actor: String = sqlx::query_scalar("SELECT original_actor FROM executions WHERE id=?")
            .bind(&exec)
            .fetch_one(&mut *db)
            .await?;
        authorize_artifact(db, &artifact, &actor).await?;
        return Ok(descriptor(&artifact, &version, &original["page"]));
    }
    let page = projection(db, &bounded, authority, true).await?;
    // Use the original operation's artifact slot. The invocation and queried
    // source bindings are committed together with the version.
    let raw = json!({
        "result":{"schema":EXPORT_SCHEMA,"selector":bounded,"page":page}
    })
    .to_string();
    let (artifact, version) = crate::tools::results::retain(
        db,
        &exec,
        &operation,
        &raw,
        crate::tools::results::DEFAULT_QUOTA,
    )
    .await?
    .ok_or("trace artifact storage full")?;
    Ok(descriptor(&artifact, &version, &page))
}

fn descriptor(handle: &str, version: &str, page: &Value) -> Value {
    json!({"handle":handle,"version":version,"schema":EXPORT_SCHEMA,"path":"/result",
           "next_cursor":page["next_cursor"]})
}

pub(crate) async fn authorize_artifact(
    db: &mut SqliteConnection,
    handle: &str,
    actor: &str,
) -> Result<()> {
    let Some((content, expiry)) = sqlx::query_as::<_, (bool, i64)>(
        "SELECT coalesce(json_extract(body,'$.result.selector.content'),0),
                coalesce((SELECT min(unixepoch(json_extract(value,'$.expires_at')))
                          FROM json_each(body,'$.result.page.items')),9223372036854775807)
         FROM tool_artifacts WHERE handle=? AND json_extract(body,'$.result.schema')=?",
    )
    .bind(handle)
    .bind(EXPORT_SCHEMA)
    .fetch_optional(&mut *db)
    .await?
    else {
        return Ok(());
    };
    let current = crate::host::personal::credentials::now()?;
    if expiry <= current {
        return Err("original trace export expired".into());
    }
    let admin: bool = sqlx::query_scalar(
        "SELECT r.admin FROM persons p JOIN persons r ON r.id=coalesce(p.current_root,p.id)
         WHERE p.id=? AND p.enabled AND r.enabled",
    )
    .bind(actor)
    .fetch_one(db)
    .await?;
    config::person_access(actor.to_owned(), admin, "control.execution.metadata.read")?;
    if content {
        config::person_access(actor.to_owned(), admin, "control.execution.content.read")?;
    }
    Ok(())
}
