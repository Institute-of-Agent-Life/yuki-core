use super::{Change, Route, scope};
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

async fn request(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    params: &Value,
) -> Result<Value> {
    super::super::domain::active(db, plugin, generation, "llm.complete").await?;
    let (revision, route) =
        scope::route(db, plugin, text(params, "route")?, text(params, "purpose")?).await?;
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
    let origin = scope::resolve(db, plugin, generation, &params["origin"], &route, true).await?;
    let task: Option<chat::Task> = params
        .get("task")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()?;
    if let Some(task) = &task {
        jsonschema::validator_for(&task.schema)?;
        if task.validation_retries != 0 {
            return Err("auxiliary route requires explicit new calls after invalid results".into());
        }
    }
    let input = text(params, "text")?;
    let mut messages = vec![];
    for content in params["context"].as_array().into_iter().flatten() {
        messages.push(json!({"role":"system","content":content}));
    }
    messages.push(json!({"role":"user","content":input}));
    let profile = &route.profile;
    let mut layout = json!({"model":profile.model,"max_tokens":profile.max_output,"stream":false,"messages":messages});
    chat::configure(&mut layout, task.as_ref(), vec![]);
    if task.is_none() {
        layout["tool_choice"] = json!("none");
    }
    let mut body = profile.protocol.request(layout, profile, task.as_ref())?;
    if params["selected_media"] == true || params.get("media").is_some() {
        append_media(
            db,
            plugin,
            text(&params["origin"], "handle")?,
            &route,
            text(params, "purpose")?,
            params.get("media"),
            &mut body,
        )
        .await?;
    }
    let raw = body.to_string();
    let exec = scope::execution(db, plugin, &origin, &route).await?;
    for reference in params["sources"].as_array().into_iter().flatten() {
        let event = reference.as_i64().ok_or("invalid declared source")?;
        let allowed: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM events e JOIN persons p ON p.id=e.raw_actor
             JOIN persons root ON root.id=coalesce(p.current_root,p.id)
             WHERE e.id=?1 AND p.enabled AND root.enabled
             AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)
             AND (e.id=?5 OR (e.entry_id=?2 AND EXISTS(SELECT 1 FROM source_holds WHERE owner=?3 AND source=e.id))
                 OR e.id IN(SELECT source FROM executions WHERE id=?4 UNION SELECT event FROM main_dependencies WHERE execution=?4)))",
        ).bind(event).bind(&origin.entry).bind(plugin).bind(&origin.parent)
            .bind(origin.event).fetch_one(&mut *db).await?;
        if !allowed {
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
    execution::activate(db, &exec, &logical, &profile.contract()).await?;
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
        .bind(text(params, "route")?)
        .bind(text(params, "purpose")?)
        .bind(&logical)
        .bind(hash)
        .bind(task.map(|v| serde_json::to_string(&v)).transpose()?)
        .execute(&mut *db)
        .await?;
    let sources: Vec<i64> =
        sqlx::query_scalar("SELECT event FROM main_dependencies WHERE execution=?")
            .bind(&exec)
            .fetch_all(&mut *db)
            .await?;
    for event in sources {
        source::acquire(db, plugin, &id, event).await?;
    }
    Ok(json!({"request":id}))
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
    let (revision, route) = scope::route(db, plugin, row.get("route"), row.get("purpose")).await?;
    if revision != row.get::<&str, _>("revision") {
        return Err("original auxiliary route changed".into());
    }
    let origin: Value = serde_json::from_str(row.get("origin"))?;
    scope::resolve(db, plugin, generation, &origin, &route, false).await?;
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
    Ok(route)
}

async fn dispatch(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    id: &str,
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
            attempt,
        },
    )
    .await?;
    permit["profile"] = serde_json::to_value(route.profile)?;
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
        let decoded = route.profile.protocol.decode(
            attempt.get("response"),
            task.as_ref(),
            &route.profile,
            &serde_json::from_str(&paid)?,
        )?;
        serde_json::to_vec(&decoded.result)?
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
        } => dispatch(db, plugin, generation, request).await,
        Change::Result {
            plugin,
            generation,
            params,
            page,
        } => result(db, plugin, generation, params, *page).await,
    }
}
