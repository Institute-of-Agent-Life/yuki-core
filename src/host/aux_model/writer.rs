use super::{Change, Route, scope};
use crate::host::participation::retained_review_reply;
use crate::{
    Result,
    identity::new_id,
    model::{attempt, chat},
    privacy::source,
    runtime::execution,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};

pub(in crate::host) async fn configure(
    db: &mut SqliteConnection,
    install: &super::super::grants::Install,
) -> Result<()> {
    for (name, route) in &install.aux_routes {
        sqlx::query("INSERT INTO host_aux_routes VALUES(?,?,?,?) ON CONFLICT DO NOTHING")
            .bind(&install.id)
            .bind(install.revision()?)
            .bind(name)
            .bind(serde_json::to_string(route)?)
            .execute(&mut *db)
            .await?;
    }
    Ok(())
}

fn text<'a>(params: &'a Value, name: &str) -> Result<&'a str> {
    params[name]
        .as_str()
        .ok_or_else(|| format!("missing {name}").into())
}

async fn chat_body(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    params: &Value,
    profile: &crate::runtime::main::Config,
) -> Result<(Value, Option<chat::Task>)> {
    let mut task: Option<chat::Task> = params
        .get("task")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()?;
    if let Some(task) = &mut task {
        task.mode = crate::model::profile_store::runtime::structured_mode(profile, &task.mode)?;
        jsonschema::validator_for(&task.schema)?;
        if task.validation_retries != 0 {
            return Err("auxiliary route requires explicit new calls after invalid results".into());
        }
    }
    let mut messages = vec![];
    for content in params["context"].as_array().into_iter().flatten() {
        messages.push(json!({"role":"system","content":content}));
    }
    messages.push(json!({"role":"user","content":text(params, "text")?}));
    if let Some(turn) = params["origin"]["session"].as_str() {
        messages = super::super::plugin_sessions::model_input(db, plugin, generation, turn, params)
            .await?;
    }
    let mut layout = json!({"model":profile.model,"max_tokens":profile.max_output,"stream":false,"messages":messages});
    chat::configure(&mut layout, task.as_ref(), vec![]);
    if task.is_none() {
        layout["tool_choice"] = json!("none");
    }
    Ok((
        profile.protocol.request(layout, profile, task.as_ref())?,
        task,
    ))
}

async fn request(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    params: &Value,
) -> Result<Value> {
    super::super::domain::active(db, plugin, generation, "llm.complete").await?;
    let route_name = text(params, "route")?;
    let purpose = text(params, "purpose")?;
    let (revision, mut route) = scope::route(db, plugin, route_name, purpose).await?;
    let original = params["origin"].to_string();
    let hash = crate::memory::scope::hash(params);
    if let Some(row) = sqlx::query(
        "SELECT id,revision,request_hash FROM host_aux_requests WHERE plugin=? AND origin=? AND original_key=?",
    ).bind(plugin).bind(&original).bind(text(params,"call_id")?).fetch_optional(&mut *db).await? {
        if row.get::<&str,_>("request_hash") != hash || row.get::<&str,_>("revision") != revision {
            return Err("original auxiliary request conflicts".into());
        }
        return Ok(json!({"request":row.get::<&str,_>("id")}));
    }
    let origin = scope::resolve(
        db,
        plugin,
        generation,
        &params["origin"],
        route_name,
        &route,
        true,
    )
    .await?;
    if let Some(task) = route.profile.profile_task.clone() {
        route.profile =
            crate::model::profile_store::runtime::select(db, &route.profile, &task).await?;
        route.validate()?;
    }
    let profile = &route.profile;
    let (mut body, task) = match (route.jev.is_some(), params.get("jev")) {
        (true, Some(input)) => (
            json!({"model":profile.model,"state":input["state"],"questions":input["questions"]}),
            None,
        ),
        (false, None) => chat_body(db, plugin, generation, params, profile).await?,
        _ => return Err("auxiliary input does not match the installed wire".into()),
    };
    let contract = route.contract();
    let has_media = params["selected_media"] == true || params.get("media").is_some();
    let transcript = if purpose == "transcript" && has_media {
        let handle = text(&params["origin"], "handle")?;
        Some(transcript_source(db, plugin, handle).await?)
    } else {
        None
    };
    if has_media {
        append_media(
            db,
            plugin,
            text(&params["origin"], "handle")?,
            &route,
            purpose,
            params.get("media"),
            &mut body,
        )
        .await?;
    }
    let raw = body.to_string();
    let task = task
        .map(|value| serde_json::to_string(&value))
        .transpose()?;
    if let Some((event, segment)) = transcript
        && let Some(row) = sqlx::query(
            "SELECT q.id,q.route,q.revision,q.task,r.request_json,a.profile_contract,o.parent_execution,
                    NOT EXISTS(SELECT 1 FROM json_each(?4) declared WHERE NOT EXISTS(
                        SELECT 1 FROM source_holds h WHERE h.owner=q.plugin
                        AND h.item=q.id AND h.source=declared.value)) AS sources_covered
             FROM host_aux_transcripts t JOIN host_aux_requests q ON q.id=t.request
             JOIN host_aux_origins o ON o.plugin=q.plugin AND o.original=q.origin
             JOIN model_requests r ON r.id=q.logical JOIN activations a ON a.id=r.activation
             WHERE t.plugin=?1 AND t.source=?2 AND t.segment=?3",
        )
        .bind(plugin)
        .bind(event)
        .bind(segment)
        .bind(params.get("sources").map_or_else(|| "[]".into(), Value::to_string))
        .fetch_optional(&mut *db)
        .await?
    {
        if row.get::<&str, _>("route") != route_name
            || row.get::<&str, _>("revision") != revision
            || row.get::<Option<String>, _>("task") != task
            || row.get::<&str, _>("request_json") != raw
            || row.get::<&str, _>("profile_contract") != contract
            || row.get::<Option<String>, _>("parent_execution") != origin.parent
            || !row.get::<bool, _>("sources_covered")
        {
            return Err("original transcript request conflicts".into());
        }
        return Ok(json!({"request":row.get::<&str,_>("id"),"observe_only":true}));
    }
    let exec = scope::execution(db, plugin, &origin, &route).await?;
    if let Some(turn) = params["origin"]["session"].as_str() {
        super::super::plugin_sessions::bind_execution(db, plugin, turn, &exec).await?;
    }
    for reference in params["sources"].as_array().into_iter().flatten() {
        let event = reference.as_i64().ok_or("invalid declared source")?;
        let allowed: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM events e JOIN persons p ON p.id=e.raw_actor
             JOIN persons root ON root.id=coalesce(p.current_root,p.id)
             WHERE e.id=?1 AND p.enabled AND root.enabled
             AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)
             AND NOT EXISTS(SELECT 1 FROM participation_erased_mentions f JOIN participation_held_sources h
                 ON h.source=f.event AND h.owner=?3 WHERE f.event=e.id)
             AND (NOT ?6 OR (e.entry_id=?2 AND e.observed_generation=coalesce(
                 (SELECT generation FROM main_windows WHERE conversation=e.entry_id),1)
                 AND NOT EXISTS(SELECT 1 FROM personal_deliveries d WHERE d.event=e.id)))
             AND (e.id=?5 OR (e.entry_id=?2 AND EXISTS(SELECT 1 FROM source_holds WHERE owner=?3 AND source=e.id))
                 OR e.id IN(SELECT source FROM executions WHERE id=?4 UNION SELECT event FROM main_dependencies WHERE execution=?4)))",
        ).bind(event).bind(&origin.entry).bind(plugin).bind(&origin.parent)
            .bind(origin.event).bind(route.jev.is_some()).fetch_one(&mut *db).await?;
        if !allowed
            && (route.jev.is_none()
                || !retained_review_reply(db, plugin, origin.event, event).await?)
        {
            return Err("declared auxiliary source outside original retained scope".into());
        }
        sqlx::query("INSERT OR IGNORE INTO main_dependencies(execution,event) VALUES(?,?)")
            .bind(&exec)
            .bind(event)
            .execute(&mut *db)
            .await?;
    }
    let id = new_id(db).await?;
    let logical = format!("plugin-aux:{id}");
    execution::activate(db, &exec, &logical, &contract).await?;
    sqlx::query("INSERT INTO core_meta(key,value) VALUES(?,?)")
        .bind(format!("aux-model-profile:{logical}"))
        .bind(serde_json::to_string(profile)?)
        .execute(&mut *db)
        .await?;
    attempt::apply(
        db,
        &attempt::Action::Reserve(attempt::Reservation {
            activation: logical.clone(),
            logical: logical.clone(),
            input: i64::try_from(raw.len())?,
            output: profile.max_output,
            safe_rate_limit_once: false,
            native_tools: false,
            request: raw,
        }),
    )
    .await?;
    sqlx::query("INSERT INTO host_aux_requests(id,plugin,revision,origin,original_key,route,purpose,logical,request_hash,task) VALUES(?,?,?,?,?,?,?,?,?,?)")
        .bind(&id)
        .bind(plugin)
        .bind(revision)
        .bind(original)
        .bind(text(params, "call_id")?)
        .bind(route_name)
        .bind(purpose)
        .bind(&logical)
        .bind(hash)
        .bind(task)
        .execute(&mut *db)
        .await?;
    if let Some((event, segment)) = transcript {
        sqlx::query("INSERT INTO host_aux_transcripts VALUES(?,?,?,?)")
            .bind(plugin)
            .bind(event)
            .bind(segment)
            .bind(&id)
            .execute(&mut *db)
            .await?;
    }
    let sources: Vec<(i64, bool)> = sqlx::query_as(
        "SELECT dep.event,d.event IS NOT NULL FROM main_dependencies dep
         LEFT JOIN personal_deliveries d ON d.event=dep.event WHERE dep.execution=?",
    )
    .bind(&exec)
    .fetch_all(&mut *db)
    .await?;
    for (event, self_delivery) in sources {
        if self_delivery && retained_review_reply(db, plugin, origin.event, event).await? {
            source::retain_self_delivery(db, plugin, &id, event).await?;
        } else {
            source::acquire(db, plugin, &id, event).await?;
        }
    }
    Ok(json!({"request":id}))
}

async fn transcript_source(
    db: &mut SqliteConnection,
    plugin: &str,
    handle: &str,
) -> Result<(i64, i64)> {
    let (exec, actor, event, segment): (String, String, i64, i64) = sqlx::query_as(
        "SELECT h.execution,x.original_actor,s.source,s.segment FROM host_calls h
         JOIN executions x ON x.id=h.execution JOIN host_media_selected s ON s.handle=h.handle
         JOIN media_sources m ON m.event=s.source AND m.segment=s.segment AND m.digest=s.digest
         WHERE h.handle=? AND h.plugin=? AND h.method='media.process' AND s.purpose='transcript'
         AND m.kind IN('audio','record') AND m.state='cached' AND m.expires>unixepoch('subsec')*1000",
    )
    .bind(handle)
    .bind(plugin)
    .fetch_one(&mut *db)
    .await?;
    crate::history::authorize_audio_source(db, &exec, &actor, event).await?;
    Ok((event, segment))
}

async fn append_media(
    db: &mut SqliteConnection,
    plugin: &str,
    handle: &str,
    route: &Route,
    purpose: &str,
    reference: Option<&Value>,
    body: &mut Value,
) -> Result<()> {
    let row = if let Some(reference) = reference {
        sqlx::query(
            "SELECT v.mime,s.purpose,b.bytes FROM host_media_uploads u
             JOIN host_calls c ON c.handle=u.handle JOIN host_media_selected s ON s.handle=c.handle
             JOIN media_versions v ON v.id=u.version JOIN media_objects b ON b.digest=v.digest
             WHERE u.handle=? AND c.plugin=? AND v.id=? AND v.digest=?",
        )
        .bind(handle)
        .bind(plugin)
        .bind(text(reference, "artifact_id")?)
        .bind(text(reference, "version")?)
        .fetch_one(db)
        .await?
    } else {
        sqlx::query(
            "SELECT s.mime,s.purpose,b.bytes FROM host_media_selected s
             JOIN host_calls c ON c.handle=s.handle JOIN media_objects b ON b.digest=s.digest
             WHERE s.handle=? AND c.plugin=?",
        )
        .bind(handle)
        .bind(plugin)
        .fetch_one(db)
        .await?
    };
    let bytes: Vec<u8> = row.get("bytes");
    let declared: &str = row.get("mime");
    let detected = crate::media::mime_bytes(&bytes);
    let mime = if declared == "application/octet-stream" && detected == "audio/wav" {
        detected
    } else {
        declared
    };
    if row.get::<&str, _>("purpose") != purpose {
        return Err("media purpose differs from original selection".into());
    }
    let encoded = STANDARD.encode(&bytes);
    if mime.starts_with("image/") && route.media.contains("image") {
        return crate::media::append_native_image(route.profile.protocol, body, mime, &encoded);
    }
    if !mime.starts_with("audio/") || !route.media.contains("audio") {
        return Err("selected media outside route capability".into());
    }
    if route.profile.protocol.is_gemini() {
        body["contents"]
            .as_array_mut()
            .ok_or("missing Gemini contents")?
            .push(json!({"role":"user","parts":[{"inlineData":{"mimeType":mime,"data":encoded}}]}));
    } else {
        let format = match mime {
            "audio/wav" | "audio/x-wav" => "wav",
            "audio/mpeg" => "mp3",
            _ => return Err("audio MIME unavailable on Chat route".into()),
        };
        body["messages"].as_array_mut().ok_or("missing Chat messages")?.push(
            json!({"role":"user","content":[{"type":"input_audio","input_audio":{"data":encoded,"format":format}}]}),
        );
    }
    Ok(())
}

async fn load(
    db: &mut SqliteConnection,
    plugin: &str,
    id: &str,
) -> Result<sqlx::sqlite::SqliteRow> {
    Ok(sqlx::query(
        "SELECT q.*,o.execution,r.root FROM host_aux_requests q
         JOIN host_aux_origins o ON o.plugin=q.plugin AND o.original=q.origin
         JOIN model_requests r ON r.id=q.logical WHERE q.id=? AND q.plugin=?",
    )
    .bind(id)
    .bind(plugin)
    .fetch_one(db)
    .await?)
}

async fn authorized_route(
    db: &mut SqliteConnection,
    row: &sqlx::sqlite::SqliteRow,
    plugin: &str,
    generation: &str,
    dispatch: bool,
) -> Result<Route> {
    let (revision, mut route) =
        scope::route(db, plugin, row.get("route"), row.get("purpose")).await?;
    if revision != row.get::<&str, _>("revision") {
        return Err("original auxiliary route changed".into());
    }
    let origin: Value = serde_json::from_str(row.get("origin"))?;
    let resolved = scope::resolve(
        db,
        plugin,
        generation,
        &origin,
        row.get("route"),
        &route,
        false,
    )
    .await?;
    if route.jev.is_some() {
        let replies: Vec<i64> = sqlx::query_scalar(
            "SELECT h.source FROM source_holds h JOIN personal_deliveries d ON d.event=h.source
             WHERE h.owner=? AND h.item=?",
        )
        .bind(plugin)
        .bind(row.get::<&str, _>("id"))
        .fetch_all(&mut *db)
        .await?;
        for event in replies {
            if !retained_review_reply(db, plugin, resolved.event, event).await? {
                return Err("original SELF reply unavailable for review".into());
            }
        }
    }
    if dispatch && let Some(handle) = origin["handle"].as_str() {
        let closed: bool = sqlx::query_scalar(
            "SELECT state IN('confirmed','failed') FROM host_calls WHERE handle=?",
        )
        .bind(handle)
        .fetch_one(&mut *db)
        .await?;
        if closed {
            return Err("original parent has already settled".into());
        }
    }
    if dispatch {
        execution::eligible(db, row.get("execution")).await?;
    }
    if let Some(value) = sqlx::query_scalar::<_, String>("SELECT value FROM core_meta WHERE key=?")
        .bind(format!(
            "aux-model-profile:{}",
            row.get::<&str, _>("logical")
        ))
        .fetch_optional(&mut *db)
        .await?
    {
        route.profile = serde_json::from_str(&value)?;
        let contract: String = sqlx::query_scalar("SELECT a.profile_contract FROM activations a JOIN model_requests r ON r.activation=a.id WHERE r.id=?")
            .bind(row.get::<&str,_>("logical")).fetch_one(db).await?;
        if route.contract() != contract {
            return Err("original_profile_unavailable".into());
        }
    }
    Ok(route)
}

async fn dispatch(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    id: &str,
    reflection_budget: Option<&crate::host::maintenance::ReflectionBudget>,
    foreground: Option<&(crate::runtime::main::activity::Active, u32)>,
) -> Result<Value> {
    super::super::domain::active(db, plugin, generation, "llm.complete").await?;
    let row = load(db, plugin, id).await?;
    let logical: &str = row.get("logical");
    if sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM model_attempts WHERE logical=?)")
        .bind(logical)
        .fetch_one(&mut *db)
        .await?
    {
        return Ok(json!({"dispatch_now":false}));
    }
    let route = authorized_route(db, &row, plugin, generation, true).await?;
    if route.jev.is_some()
        && route
            .profile
            .key_env
            .as_ref()
            .is_none_or(|name| !std::env::var(name).is_ok_and(|value| !value.trim().is_empty()))
    {
        return Err("original Jev credential unavailable".into());
    }
    let origin: Value = serde_json::from_str(row.get("origin"))?;
    let mut foreground_generation = None;
    if row.get::<&str, _>("purpose") == "reflect" && origin["maintenance"].is_string() {
        foreground_generation = foreground.map(|(active, _)| active.foreground_generation());
        super::super::maintenance::check_reflection_foreground(db, foreground).await?;
    }
    sqlx::query("UPDATE host_aux_requests SET dispatched_generation=? WHERE id=?")
        .bind(generation)
        .bind(id)
        .execute(&mut *db)
        .await?;
    let attempt = format!("{logical}:http-0");
    let mut permit = attempt::apply(
        db,
        &attempt::Action::Dispatch {
            logical: logical.into(),
            attempt: attempt.clone(),
        },
    )
    .await?;
    // A quota denial rolls back the dispatch. Count the attempt using its
    // recorded timestamp, including when this transaction crosses midnight.
    if let Some(budget) = reflection_budget {
        budget
            .admit(db, &attempt, row.get("purpose"), &origin)
            .await?;
    }
    permit["route"] = serde_json::to_value(&route)?;
    if let Some(generation) = foreground_generation {
        permit["foreground_generation"] = json!(generation);
    }
    permit["task"] = row
        .get::<Option<&str>, _>("task")
        .map(serde_json::from_str::<Value>)
        .transpose()?
        .unwrap_or(Value::Null);
    Ok(permit)
}

async fn result(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    params: &Value,
    page: bool,
) -> Result<Value> {
    super::super::domain::active(
        db,
        plugin,
        generation,
        if page { "llm.result" } else { "llm.complete" },
    )
    .await?;
    let id = text(params, "request")?;
    let row = load(db, plugin, id).await?;
    if page && serde_json::from_str::<Value>(row.get("origin"))?["session"].is_string() {
        return Err("isolated transcript content requires its original session scope".into());
    }
    let erased = source::execution_erased(db, row.get("execution"), None).await?;
    let route = if erased {
        None
    } else {
        Some(authorized_route(db, &row, plugin, generation, false).await?)
    };
    let attempt=sqlx::query("SELECT state,response,usage_json FROM model_attempts WHERE logical=? ORDER BY ordinal DESC LIMIT 1")
        .bind(row.get::<&str,_>("logical")).fetch_optional(&mut *db).await?;
    let state = attempt
        .as_ref()
        .map_or("reserved", |r| r.get::<&str, _>("state"));
    let usage = attempt
        .as_ref()
        .and_then(|r| r.get::<Option<&str>, _>("usage_json"))
        .map(serde_json::from_str::<Value>)
        .transpose()?
        .unwrap_or(Value::Null);
    let receipt = json!({"request":id,"logical":row.get::<&str,_>("logical"),"root":row.get::<&str,_>("root"),"state":state,"usage":usage,"erased":erased});
    if !page {
        return Ok(receipt);
    }
    let bytes = if let (Some(route), Some(attempt)) = (
        route,
        attempt.filter(|r| matches!(r.get::<&str, _>("state"), "response" | "paired")),
    ) {
        let task: Option<chat::Task> = row
            .get::<Option<&str>, _>("task")
            .map(serde_json::from_str)
            .transpose()?;
        let paid: String = sqlx::query_scalar("SELECT request_json FROM model_requests WHERE id=?")
            .bind(row.get::<&str, _>("logical"))
            .fetch_one(&mut *db)
            .await?;
        serde_json::to_vec(&decode_result(
            &route,
            attempt.get("response"),
            task.as_ref(),
            &paid,
        )?)?
    } else {
        vec![]
    };
    let offset = usize::try_from(params["offset"].as_u64().unwrap_or(0))?;
    let limit = usize::try_from(params["limit"].as_u64().unwrap_or(12288))?;
    if offset > bytes.len() {
        return Err("original result offset outside content".into());
    }
    let end = offset.saturating_add(limit).min(bytes.len());
    Ok(
        json!({"receipt":receipt,"sha256":if bytes.is_empty(){None}else{Some(format!("{:x}",Sha256::digest(&bytes)))},"size":bytes.len(),"offset":offset,
        "next_offset":if end<bytes.len(){Some(end)}else{None},"bytes":STANDARD.encode(&bytes[offset..end])}),
    )
}

pub(in crate::host) async fn apply(db: &mut SqliteConnection, change: &Change) -> Result<Value> {
    match change {
        Change::Request {
            plugin,
            generation,
            params,
        } => request(db, plugin, generation, params).await,
        Change::Dispatch {
            plugin,
            generation,
            request,
            reflection_budget,
            foreground,
        } => {
            dispatch(
                db,
                plugin,
                generation,
                request,
                reflection_budget.as_ref(),
                foreground.as_ref(),
            )
            .await
        }
        Change::Result {
            plugin,
            generation,
            params,
            page,
        } => result(db, plugin, generation, params, *page).await,
    }
}

/// A maintenance preview can consume only the original successfully decoded structured result.
pub(in crate::host) async fn structured_receipt(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    id: &str,
) -> Result<Value> {
    decoded_receipt(db, plugin, generation, id, true).await
}
pub(in crate::host) async fn visible_receipt(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    id: &str,
) -> Result<Value> {
    decoded_receipt(db, plugin, generation, id, false).await
}
async fn decoded_receipt(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    id: &str,
    structured: bool,
) -> Result<Value> {
    super::super::domain::active(db, plugin, generation, "llm.result").await?;
    let row = load(db, plugin, id).await?;
    if source::execution_erased(db, row.get("execution"), None).await? {
        return Err("original paid sources erased".into());
    }
    let route = authorized_route(db, &row, plugin, generation, false).await?;
    let task: Option<chat::Task> = row
        .get::<Option<&str>, _>("task")
        .map(serde_json::from_str)
        .transpose()?;
    if structured && task.is_none() {
        return Err("structured paid result required".into());
    }
    let response:String=sqlx::query_scalar("SELECT response FROM model_attempts WHERE logical=? AND state IN('response','paired') ORDER BY ordinal DESC LIMIT 1")
        .bind(row.get::<&str,_>("logical")).fetch_one(&mut *db).await?;
    let request: String = sqlx::query_scalar("SELECT request_json FROM model_requests WHERE id=?")
        .bind(row.get::<&str, _>("logical"))
        .fetch_one(&mut *db)
        .await?;
    let decoded = decode_result(&route, &response, task.as_ref(), &request)?;
    let result = if serde_json::from_str::<Value>(row.get("origin"))?["session"].is_string() {
        json!(super::super::plugin_sessions::visible_text(&decoded))
    } else {
        decoded
    };
    Ok(
        json!({"result":result,"execution":row.get::<String,_>("execution"),"logical":row.get::<String,_>("logical"),"route":row.get::<String,_>("route"),"purpose":row.get::<String,_>("purpose")}),
    )
}

fn decode_result(
    route: &Route,
    response: &str,
    task: Option<&chat::Task>,
    paid: &str,
) -> Result<Value> {
    if let Some(jev) = &route.jev {
        return jev.result(response);
    }
    Ok(route
        .profile
        .protocol
        .decode(response, task, &route.profile, &serde_json::from_str(paid)?)?
        .result)
}
