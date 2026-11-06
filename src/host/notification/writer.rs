use super::{Call, scope};
use crate::{
    Result,
    identity::new_id,
    runtime::{effects, main},
};
use chrono::{DateTime, NaiveDateTime, SecondsFormat};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

fn text<'a>(params: &'a Value, key: &str) -> Result<&'a str> {
    params[key]
        .as_str()
        .ok_or_else(|| format!("notification {key} required").into())
}

fn normalized(params: &Value) -> Result<Value> {
    let mut value = params.clone();
    let raw = text(params, "occurred_at")?;
    let instant = match DateTime::parse_from_rfc3339(raw) {
        Ok(time) => time.to_utc(),
        Err(_) => NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f")?.and_utc(),
    };
    value["occurred_at"] = json!(instant.to_rfc3339_opts(SecondsFormat::AutoSi, true));
    for (key, default) in [
        ("payload", json!({})),
        ("text", json!("")),
        ("media_handles", json!([])),
        ("ask_agent", json!(false)),
        ("resume_waiting_work", json!(false)),
        ("agent_intent", json!("")),
    ] {
        if value.get(key).is_none() {
            value[key] = default;
        }
    }
    if value["ask_agent"] != true && !text(&value, "agent_intent")?.is_empty() {
        return Err("notification agent intent requires Main evaluation".into());
    }
    if serde_json::to_vec(&value["payload"])?.len() > 32 * 1024 {
        return Err("notification payload exceeds 32 KiB".into());
    }
    Ok(value)
}

struct Media {
    id: String,
    digest: Option<String>,
    kind: Option<String>,
}

async fn media(
    db: &mut SqliteConnection,
    plugin: &str,
    target: &scope::Target,
    original: Option<&str>,
    handles: &Value,
) -> Result<Vec<Media>> {
    let mut result = Vec::new();
    let now = crate::work::wait::clock()?;
    for value in handles
        .as_array()
        .ok_or("notification media handles required")?
    {
        let id = value.as_str().ok_or("invalid notification media handle")?;
        if let Some(publication) = original
            && let Some((digest,)) = sqlx::query_as::<_, (Option<String>,)>(
                "SELECT digest FROM notification_media WHERE publication=? AND version=? LIMIT 1",
            )
            .bind(publication)
            .bind(id)
            .fetch_optional(&mut *db)
            .await?
        {
            result.push(Media {
                id: id.into(),
                digest,
                kind: None,
            });
            continue;
        }
        let row = sqlx::query(
            "SELECT v.digest,v.kind,v.execution,u.notification_expires,
                    coalesce(x.original_actor,x.origin) AS actor,coalesce(e.entry_id,o.conversation) AS entry
             FROM host_media_uploads u JOIN host_calls c ON c.handle=u.handle AND c.plugin=?
             JOIN media_versions v ON v.id=u.version JOIN executions x ON x.id=v.execution
             LEFT JOIN events e ON e.id=x.source LEFT JOIN main_origins o ON o.ref=x.origin
             WHERE v.id=? AND v.digest IS NOT NULL",
        ).bind(plugin).bind(id).fetch_one(&mut *db).await?;
        if row.get::<Option<&str>, _>("entry") != Some(&target.entry)
            || original.is_none()
                && row
                    .get::<Option<i64>, _>("notification_expires")
                    .is_none_or(|expires| expires <= now)
        {
            return Err("notification media lease or original scene unavailable".into());
        }
        effects::authorize(db, row.get("execution"), row.get("actor"), false).await?;
        result.push(Media {
            id: id.into(),
            digest: row.get("digest"),
            kind: row.get("kind"),
        });
    }
    Ok(result)
}

pub(super) async fn publish(db: &mut SqliteConnection, call: &Call) -> Result<Value> {
    let (revision, policy) = scope::policy(db, &call.plugin).await?;
    let mut manifest = normalized(&call.params)?;
    manifest["plugin_id"] = json!(call.plugin);
    let target = scope::select(db, &policy, &manifest["target"]).await?;
    let key = text(&manifest, "event_key")?;
    let existing: Option<(String, String)> = sqlx::query_as(
        "SELECT id,digest FROM notification_publications WHERE plugin=? AND event_key=?",
    )
    .bind(&call.plugin)
    .bind(key)
    .fetch_optional(&mut *db)
    .await?;
    let selected = media(
        db,
        &call.plugin,
        &target,
        existing.as_ref().map(|(id, _)| id.as_str()),
        &manifest["media_handles"],
    )
    .await?;
    manifest["media_handles"] = json!(
        selected
            .iter()
            .enumerate()
            .map(|(index, item)| {
                match &item.digest {
                    Some(digest) => json!({"index":index,"sha256":digest}),
                    None => json!({"index":index,"legacy_handle_id":item.id}),
                }
            })
            .collect::<Vec<_>>()
    );
    let digest = crate::memory::scope::hash(&manifest);
    if let Some((id, original)) = existing {
        if original != digest {
            return Err("original notification manifest conflicts".into());
        }
        return receipt(db, &call.plugin, &id, true).await;
    }
    if (manifest["ask_agent"] == true || manifest["resume_waiting_work"] == true) && !policy.agent {
        return Err("notification agent permission not installed".into());
    }
    if manifest["ask_agent"] == true && call.config.is_none() {
        return Err("notification Main evaluation unavailable".into());
    }
    let id = new_id(db).await?;
    let origin = format!("notification:{id}");
    let execution = new_id(db).await?;
    main::window(db, &target.entry).await?;
    sqlx::query("INSERT INTO main_origins(ref,kind,conversation) VALUES(?,'plugin',?)")
        .bind(&origin)
        .bind(&target.entry)
        .execute(&mut *db)
        .await?;
    sqlx::query("INSERT INTO executions(id,origin,state) VALUES(?,?,'running')")
        .bind(&execution)
        .bind(&origin)
        .execute(&mut *db)
        .await?;
    let envelope = json!({"kind":"external_event","publisher":call.plugin,
        "event_type":manifest["event_type"],"external_source":manifest["external_source"],
        "summary":manifest["summary"],"payload":manifest["payload"],
        "segments":[{"type":"text","text":manifest["text"]}]});
    let occurred =
        DateTime::parse_from_rfc3339(text(&manifest, "occurred_at")?)?.timestamp_millis();
    let event: i64 = sqlx::query_scalar(
        "INSERT INTO events(source_scope,source_key,entry_id,account_id,bot_stage,occurred_at,observed_at,envelope,mentions)
         SELECT ?,?,?,account,?,?,CAST(unixepoch('subsec')*1000 AS INTEGER),?,'[]' FROM notification_publishers WHERE plugin=? RETURNING id",
    ).bind(json!(["plugin",call.plugin]).to_string()).bind(text(&manifest,"event_key")?)
        .bind(&target.entry).bind(&target.receiver).bind(occurred).bind(envelope.to_string())
        .bind(&call.plugin).fetch_one(&mut *db).await?;
    sqlx::query(
        "INSERT INTO notification_publications(id,plugin,event_key,revision,digest,manifest,event,event_type,entry,receiver,target_person,target_space,window,origin,delivery_execution,resume_waiting_work)
         VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    ).bind(&id).bind(&call.plugin).bind(text(&manifest,"event_key")?).bind(revision)
        .bind(digest).bind(manifest.to_string()).bind(event).bind(text(&manifest,"event_type")?)
        .bind(&target.entry).bind(&target.receiver).bind(&target.person).bind(&target.space)
        .bind(target.window).bind(&origin).bind(&execution).bind(manifest["resume_waiting_work"]==true)
        .execute(&mut *db).await?;
    sqlx::query("INSERT INTO main_dependencies VALUES(?,?)")
        .bind(&execution)
        .bind(event)
        .execute(&mut *db)
        .await?;
    let mut messages = Vec::new();
    for (index, item) in selected.iter().enumerate() {
        sqlx::query("INSERT INTO notification_media VALUES(?,?,?,?)")
            .bind(&id)
            .bind(index as i64)
            .bind(&item.id)
            .bind(&item.digest)
            .execute(&mut *db)
            .await?;
        crate::media::bind_version_sources(db, &execution, &item.id).await?;
        messages.push(json!({"artifact_id":item.id,"attachment_kind":item.kind}));
    }
    if !text(&manifest, "text")?.trim().is_empty() {
        messages.push(json!({"text":manifest["text"]}));
    }
    sqlx::query(
        "INSERT INTO notification_sources SELECT ?,event FROM main_dependencies WHERE execution=?",
    )
    .bind(&id)
    .bind(&execution)
    .execute(&mut *db)
    .await?;
    for (ordinal, message) in messages.iter().enumerate() {
        let operation = format!("notification:{id}:{ordinal}");
        effects::apply(
            db,
            &effects::Action::Intent {
                execution: execution.clone(),
                id: operation.clone(),
                actor: origin.clone(),
                action: effects::Operation::External,
                target: "send".into(),
                payload: message.to_string(),
                parent: None,
                ordinal: None,
            },
        )
        .await?;
        sqlx::query("INSERT INTO notification_deliveries(publication,ordinal,execution,operation) VALUES(?,?,?,?)")
            .bind(&id).bind(ordinal as i64).bind(&execution).bind(operation).execute(&mut *db).await?;
    }
    if messages.is_empty() {
        sqlx::query("UPDATE executions SET state='completed' WHERE id=?")
            .bind(&execution)
            .execute(&mut *db)
            .await?;
    }
    let resumed = manifest["resume_waiting_work"] == true
        && crate::work::wait::route_event(db, event).await?;
    if manifest["ask_agent"] == true && !resumed {
        let result = main::apply(
            db,
            call.config.as_ref().ok_or("Main unavailable")?,
            &main::Change::Operator(main::Action::Trigger {
                id: origin,
                key: id.clone(),
                body: json!({"text":manifest["summary"],"context":[{
                    "publisher":call.plugin,"event":event,"intent":manifest["agent_intent"],
                    "payload":manifest["payload"],"text":manifest["text"]}]})
                .to_string(),
            }),
        )
        .await?;
        let agent = result["execution"]
            .as_str()
            .ok_or("original notification Main missing")?;
        sqlx::query("UPDATE notification_publications SET agent_execution=? WHERE id=?")
            .bind(agent)
            .bind(&id)
            .execute(&mut *db)
            .await?;
        sqlx::query("INSERT OR IGNORE INTO main_dependencies SELECT ?,source FROM notification_sources WHERE publication=?")
            .bind(agent).bind(&id).execute(&mut *db).await?;
    }
    receipt(db, &call.plugin, &id, false).await
}

pub(super) async fn read(db: &mut SqliteConnection, call: &Call) -> Result<Value> {
    receipt(
        db,
        &call.plugin,
        text(&call.params, "notification_id")?,
        true,
    )
    .await
}

async fn receipt(db: &mut SqliteConnection, plugin: &str, id: &str, replay: bool) -> Result<Value> {
    let row = sqlx::query("SELECT * FROM notification_publications WHERE id=? AND plugin=?")
        .bind(id)
        .bind(plugin)
        .fetch_one(&mut *db)
        .await?;
    super::transport::authorize(db, row.get("delivery_execution")).await?;
    let deliveries = sqlx::query("SELECT ordinal,operation,state FROM notification_deliveries WHERE publication=? ORDER BY ordinal")
        .bind(id).fetch_all(&mut *db).await?;
    let deliveries = deliveries
        .iter()
        .map(|d| {
            json!({"ordinal":d.get::<i64,_>("ordinal"),
        "operation":d.get::<&str,_>("operation"),"state":d.get::<&str,_>("state")})
        })
        .collect::<Vec<_>>();
    let sources: Vec<i64> = sqlx::query_scalar(
        "SELECT source FROM notification_sources WHERE publication=? ORDER BY source",
    )
    .bind(id)
    .fetch_all(&mut *db)
    .await?;
    for source in &sources {
        crate::privacy::source::acquire(db, plugin, &format!("notification:{id}"), *source).await?;
    }
    let sources = json!({"ids":sources.iter().take(128).collect::<Vec<_>>(),"count":sources.len(),
        "complete":sources.len()<=128,"version":crate::memory::scope::hash(&json!(sources))});
    Ok(
        json!({"notification_id":id,"source_event_id":row.get::<i64,_>("event"),
        "event_created":!replay,"delivery_enqueued":!deliveries.is_empty(),
        "agent_turn_enqueued":row.get::<Option<String>,_>("agent_execution").is_some(),
        "deduplicated":replay,"agent_execution":row.get::<Option<String>,_>("agent_execution"),
        "state":row.get::<&str,_>("state"),"deliveries":deliveries,"sources":sources}),
    )
}

pub(super) async fn sources(db: &mut SqliteConnection, call: &Call) -> Result<Value> {
    let id = text(&call.params, "notification_id")?;
    let receipt = receipt(db, &call.plugin, id, true).await?;
    if call.params["version"] != receipt["sources"]["version"] {
        return Err("original notification dependency set changed".into());
    }
    let after = call.params["after"].as_i64().unwrap_or(0);
    let mut ids: Vec<i64> = sqlx::query_scalar("SELECT source FROM notification_sources WHERE publication=? AND source>? ORDER BY source LIMIT 129")
        .bind(id).bind(after).fetch_all(db).await?;
    let more = ids.len() > 128;
    ids.truncate(128);
    Ok(
        json!({"notification_id":id,"ids":ids,"count":receipt["sources"]["count"],
        "version":receipt["sources"]["version"],"next":if more{ids.last().copied()}else{None}}),
    )
}

pub(super) async fn delivered(
    db: &mut SqliteConnection,
    id: &str,
    ordinal: i64,
    state: &str,
) -> Result<Value> {
    if !matches!(state, "confirmed" | "failed" | "unknown") {
        return Err("notification delivery requires an original terminal receipt".into());
    }
    let changed = sqlx::query("UPDATE notification_deliveries SET state=? WHERE publication=? AND ordinal=? AND state='pending'")
        .bind(state).bind(id).bind(ordinal).execute(&mut *db).await?.rows_affected();
    if changed == 1 {
        sqlx::query("UPDATE notification_outbox_control SET attempts=attempts+1,error_category=CASE WHEN ?='confirmed' THEN NULL ELSE error_category END WHERE publication=? AND ordinal=?")
            .bind(state).bind(id).bind(ordinal).execute(&mut *db).await?;
    }
    sqlx::query(
        "UPDATE executions SET state=CASE WHEN EXISTS(SELECT 1 FROM notification_deliveries WHERE publication=?1 AND state IN('failed','unknown')) THEN 'held' ELSE 'completed' END
         WHERE id=(SELECT delivery_execution FROM notification_publications WHERE id=?1) AND state='running'
         AND (NOT EXISTS(SELECT 1 FROM notification_deliveries WHERE publication=?1 AND state='pending')
           OR EXISTS(SELECT 1 FROM notification_deliveries WHERE publication=?1 AND state IN('failed','unknown')))",
    ).bind(id).execute(db).await?;
    Ok(json!({"notification_id":id,"ordinal":ordinal,"state":state}))
}
