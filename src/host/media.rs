use super::calls;
use crate::{
    Result,
    media::{
        blob::{MAX_BYTES, Snapshot},
        handles,
    },
    privacy::source,
    runtime::effects,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::collections::BTreeSet;

#[derive(Deserialize)]
#[serde(untagged)]
enum Input {
    Event(EventSlot),
    Artifact(Artifact),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventSlot {
    event: i64,
    slot: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    artifact_id: String,
    version: String,
}

pub(super) async fn select(
    conn: &mut SqliteConnection,
    req: &calls::Request,
    handle: &str,
    actor: &str,
) -> Result<()> {
    let purpose = req.args["purpose"]
        .as_str()
        .ok_or("selected media purpose required")?;
    if !(matches!(purpose, "inspect" | "transcript" | "frames")
        || purpose == "deliver" && req.method == "gateway.dispatch_part")
    {
        return Err("unknown media purpose".into());
    }
    let mut dependencies = BTreeSet::new();
    let (version, digest, mime, source_id, segment) = match serde_json::from_value::<Input>(
        req.args["media"].clone(),
    )? {
        Input::Event(s) => {
            let origin = handles::original(conn, &req.execution, actor, s.event, s.slot).await?;
            let original = origin["source"].as_i64().ok_or("missing source")?;
            if origin["state"] != "cached"
                || purpose == "transcript"
                    && (!matches!(origin["kind"].as_str(), Some("audio" | "record"))
                        || s.event != original)
            {
                return Err("selected original media not available for this purpose".into());
            }
            dependencies.extend([s.event, original]);
            (
                format!("event:{}:{}", original, origin["slot"]),
                origin["digest"]
                    .as_str()
                    .ok_or("uncached source")?
                    .to_owned(),
                origin["mime"].as_str().ok_or("uncached MIME")?.to_owned(),
                Some(original),
                origin["segment"].as_i64(),
            )
        }
        Input::Artifact(a) => {
            if purpose == "transcript" {
                return Err("transcript requires the original audio event".into());
            }
            let r=sqlx::query("SELECT v.mime,v.execution,v.source FROM media_versions v JOIN media_version_readers r ON r.version=v.id WHERE v.id=?1 AND v.digest=?2 AND r.reader=?3").bind(&a.artifact_id).bind(&a.version).bind(&req.execution).fetch_one(&mut *conn).await?;
            let producer = r.get::<String, _>("execution");
            let src: Option<i64> = sqlx::query_scalar("SELECT source FROM executions WHERE id=?")
                .bind(&producer)
                .fetch_one(&mut *conn)
                .await?;
            if source::execution_erased(conn, &producer, src).await? {
                return Err("original version erased".into());
            }
            dependencies.extend(
                sqlx::query_scalar::<_, i64>(
                    "SELECT event FROM main_dependencies WHERE execution=?",
                )
                .bind(producer)
                .fetch_all(&mut *conn)
                .await?,
            );
            dependencies.extend(src);
            dependencies.extend(r.get::<Option<i64>, _>("source"));
            (
                a.artifact_id,
                a.version,
                r.get("mime"),
                r.get("source"),
                None,
            )
        }
    };
    dependencies.extend(sqlx::query_scalar::<_,i64>("SELECT e.id FROM events e WHERE e.id IN(SELECT source FROM executions WHERE id=?1 UNION SELECT event FROM main_dependencies WHERE execution=?1)").bind(&req.execution).fetch_all(&mut *conn).await?);
    for id in dependencies {
        source::acquire(conn, &req.plugin, handle, id).await?;
        sqlx::query("INSERT OR IGNORE INTO main_dependencies VALUES(?,?)")
            .bind(&req.execution)
            .bind(id)
            .execute(&mut *conn)
            .await?;
    }
    let saved=sqlx::query_scalar::<_,String>("INSERT INTO host_media_selected VALUES(?,?,?,?,?,?,?) ON CONFLICT(handle) DO UPDATE SET handle=excluded.handle WHERE version=excluded.version AND digest=excluded.digest AND purpose=excluded.purpose RETURNING handle")
        .bind(handle).bind(version).bind(digest).bind(mime).bind(purpose).bind(source_id).bind(segment).fetch_optional(conn).await?;
    if saved.is_none() {
        return Err("original media selection conflicts".into());
    }
    Ok(())
}

pub(super) async fn retain_inputs(
    db: &mut SqliteConnection,
    req: &calls::Request,
    handle: &str,
) -> Result<()> {
    let applicable:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM host_installations i,json_each(i.grants) g WHERE i.plugin=? AND g.value='media.publish')").bind(&req.plugin).fetch_one(&mut *db).await?;
    if applicable {
        let ids:Vec<i64>=sqlx::query_scalar("SELECT e.id FROM events e WHERE e.raw_actor IS NOT NULL AND e.id IN(SELECT source FROM executions WHERE id=?1 UNION SELECT event FROM main_dependencies WHERE execution=?1)").bind(&req.execution).fetch_all(&mut *db).await?;
        for id in ids {
            source::acquire(db, &req.plugin, handle, id).await?;
        }
    }
    Ok(())
}
pub(super) async fn callback(
    conn: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    method: &str,
    p: &Value,
) -> Result<Value> {
    let handle = p["handle"].as_str().ok_or("missing call handle")?;
    super::selected_processor::check(conn, handle).await?;
    let (exec,actor):(String,String)=sqlx::query_as("SELECT c.execution,coalesce(x.original_actor,x.origin) FROM host_calls c JOIN executions x ON x.id=c.execution JOIN host_installations i ON i.plugin=c.plugin AND i.revision=c.revision WHERE c.handle=? AND c.plugin=? AND c.generation=? AND c.state='dispatched'").bind(handle).bind(plugin).bind(generation).fetch_one(&mut *conn).await?;
    effects::authorize(conn, &exec, &actor, true).await?;
    if method == "media.publish" {
        return publish(conn, handle, &exec, &actor, p).await;
    }
    let offset = p["offset"].as_i64().ok_or("missing offset")?;
    let count = p["length"].as_i64().ok_or("missing length")?;
    let row=sqlx::query("SELECT s.digest,s.mime,b.size,substr(b.bytes,?2+1,?3) AS bytes FROM host_media_selected s JOIN media_objects b ON b.digest=s.digest WHERE s.handle=?1 AND ?2<=b.size AND ((s.segment IS NULL AND EXISTS(SELECT 1 FROM media_versions v WHERE v.id=s.version AND v.digest=s.digest)) OR EXISTS(SELECT 1 FROM media_sources src WHERE src.event=s.source AND src.segment=s.segment AND src.digest=s.digest AND src.state='cached' AND src.expires>unixepoch('subsec')*1000))")
        .bind(handle).bind(offset).bind(count).fetch_one(conn).await?;
    let bytes = row.get::<Vec<u8>, _>("bytes");
    let size = row.get::<i64, _>("size");
    Ok(
        json!({"version":row.get::<String,_>("digest"),"mime":row.get::<String,_>("mime"),"total":size,"offset":offset,"bytes":STANDARD.encode(&bytes),"done":offset+bytes.len() as i64==size}),
    )
}
async fn publish(
    conn: &mut SqliteConnection,
    handle: &str,
    exec: &str,
    actor: &str,
    p: &Value,
) -> Result<Value> {
    let field =
        |k: &str| -> Result<&str> { p[k].as_str().ok_or_else(|| "missing media field".into()) };
    let key = field("key")?;
    let name = field("filename")?;
    let mime = field("mime")?;
    let kind = field("kind")?;
    let expected = field("digest")?;
    let offset = p["offset"].as_i64().ok_or("missing media offset")?;
    let size = p["size"].as_i64().ok_or("missing media size")?;
    let ttl = p["ttl_seconds"].as_i64().unwrap_or(86_400);
    let bytes = STANDARD.decode(field("bytes")?)?;
    if !(60..=7 * 86_400).contains(&ttl)
        || bytes.len() > 16384
        || !(0..=MAX_BYTES as i64).contains(&size)
        || offset < 0
        || offset + bytes.len() as i64 > size
        || bytes.is_empty() && size != 0
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
        || matches!(name, "." | "..")
    {
        return Err("invalid media chunk or filename".into());
    }
    let old = sqlx::query("SELECT * FROM host_media_uploads WHERE handle=? AND name=?")
        .bind(handle)
        .bind(key)
        .fetch_optional(&mut *conn)
        .await?;
    let mut cursor = 0;
    if let Some(row) = old {
        if row.get::<&str, _>("filename") != name
            || row.get::<&str, _>("kind") != kind
            || row.get::<&str, _>("mime") != mime
            || row.get::<&str, _>("digest") != expected
            || row.get::<i64, _>("size") != size
            || row
                .get::<Option<i64>, _>("notification_ttl")
                .is_some_and(|original| original != ttl)
        {
            return Err("publication conflicts with immutable key".into());
        }
        cursor = row.get("next_offset");
        if let Some(id) = row.get::<Option<String>, _>("version") {
            let retained:Vec<u8>=sqlx::query_scalar("SELECT substr(b.bytes,?2+1,?3) FROM media_versions v JOIN media_objects b ON b.digest=v.digest WHERE v.id=?1 AND v.digest=?4").bind(&id).bind(offset).bind(bytes.len() as i64).bind(expected).fetch_one(conn).await?;
            if bytes != retained {
                return Err("conflicting replay of retained version".into());
            }
            return Ok(
                json!({"complete":true,"artifact_id":id,"version":expected,"next_offset":cursor}),
            );
        }
    } else {
        if offset != 0 {
            return Err("first chunk must begin at zero".into());
        }
        let (n, total): (i64, i64) = sqlx::query_as(
            "SELECT count(*),coalesce(sum(size),0) FROM host_media_uploads WHERE handle=?",
        )
        .bind(handle)
        .fetch_one(&mut *conn)
        .await?;
        if n >= 8 || total + size > MAX_BYTES as i64 {
            return Err("original media publication bound exceeded".into());
        }
        sqlx::query("INSERT INTO host_media_uploads(handle,name,filename,kind,mime,digest,size,notification_ttl,notification_expires) VALUES(?,?,?,?,?,?,?,?,CAST(unixepoch('subsec')*1000 AS INTEGER)+?*1000)")
            .bind(handle).bind(key).bind(name).bind(kind).bind(mime).bind(expected).bind(size).bind(ttl).bind(ttl).execute(&mut *conn).await?;
    }
    if offset > cursor {
        return Err("media chunk gap".into());
    }
    if offset < cursor {
        let old: Vec<u8> = sqlx::query_scalar(
            "SELECT bytes FROM host_media_chunks WHERE handle=? AND name=? AND offset=?",
        )
        .bind(handle)
        .bind(key)
        .bind(offset)
        .fetch_one(&mut *conn)
        .await?;
        if old != bytes {
            return Err("media chunk conflict".into());
        }
    } else {
        cursor += bytes.len() as i64;
        sqlx::query("INSERT INTO host_media_chunks VALUES(?,?,?,?)")
            .bind(handle)
            .bind(key)
            .bind(offset)
            .bind(&bytes)
            .execute(&mut *conn)
            .await?;
        sqlx::query("UPDATE host_media_uploads SET next_offset=? WHERE handle=? AND name=?")
            .bind(cursor)
            .bind(handle)
            .bind(key)
            .execute(&mut *conn)
            .await?;
    }
    if cursor < size {
        return Ok(
            json!({"complete":false,"artifact_id":null,"version":null,"next_offset":cursor}),
        );
    }
    let chunks: Vec<Vec<u8>> = sqlx::query_scalar(
        "SELECT bytes FROM host_media_chunks WHERE handle=? AND name=? ORDER BY offset",
    )
    .bind(handle)
    .bind(key)
    .fetch_all(&mut *conn)
    .await?;
    let data = Snapshot::new(chunks.concat())?;
    if data.digest != expected || data.data.len() != size as usize {
        return Err("published bytes disagree with claimed version".into());
    }
    if kind == "image" && (crate::media::mime(&data) != mime || !mime.starts_with("image/")) {
        return Err("image content and MIME disagree".into());
    }
    let result = handles::version(
        conn,
        &handles::Version {
            exec,
            actor,
            op: &format!("host-media:{handle}:{key}"),
            requested: None,
            basis: expected,
            filename: name,
            mime,
            kind,
            source: None,
        },
        &data,
    )
    .await?;
    let id = result["artifact_id"]
        .as_str()
        .ok_or("missing original version")?;
    sqlx::query("UPDATE host_media_uploads SET version=? WHERE handle=? AND name=?")
        .bind(id)
        .bind(handle)
        .bind(key)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM host_media_chunks WHERE handle=? AND name=?")
        .bind(handle)
        .bind(key)
        .execute(&mut *conn)
        .await?;
    Ok(json!({"complete":true,"artifact_id":id,"version":expected,"next_offset":cursor}))
}

// The external effect is already confirmed. A rejected image or transcript is
// recorded as a presentation failure without replaying the extension handler.
pub(super) async fn project(
    conn: &mut SqliteConnection,
    handle: &str,
    exec: &str,
    actor: &str,
    method: &str,
    body: &mut Value,
) -> Result<()> {
    if !body["data"].is_object() {
        return Ok(());
    }
    if method == "media.process" {
        let r =
            sqlx::query("SELECT purpose,source,segment FROM host_media_selected WHERE handle=?")
                .bind(handle)
                .fetch_one(&mut *conn)
                .await?;
        if r.get::<&str, _>("purpose") == "transcript" {
            let text = body["data"]["text"].as_str();
            let result = if let (Some(text), Some(event), Some(segment)) = (
                text,
                r.get::<Option<i64>, _>("source"),
                r.get::<Option<i64>, _>("segment"),
            ) {
                if text.len() > 32768 {
                    false
                } else {
                    crate::history::apply(
                        conn,
                        &crate::history::Change::Transcript(
                            exec.into(),
                            actor.into(),
                            event,
                            json!([{"source":"current","segment_index":segment,"text":text}])
                                .to_string(),
                        ),
                    )
                    .await
                    .is_ok()
                }
            } else {
                false
            };
            body["data"]["transcript_stored"] = json!(result);
        }
    }
    let Some(images) = body["data"]["images"].as_array() else {
        return Ok(());
    };
    let visual: bool = sqlx::query_scalar(
        "SELECT coalesce((SELECT vision FROM host_media_profiles WHERE handle=?),0)",
    )
    .bind(handle)
    .fetch_one(&mut *conn)
    .await?;
    let mut status = Vec::new();
    for (ordinal, image) in images.iter().take(8).enumerate() {
        let version:Option<String>=sqlx::query_scalar("SELECT v.id FROM host_media_uploads u JOIN media_versions v ON v.id=u.version AND v.digest=u.digest AND v.kind='image' WHERE u.handle=? AND v.id=? AND v.digest=?").bind(handle).bind(image["artifact_id"].as_str()).bind(image["version"].as_str()).fetch_optional(&mut *conn).await?;
        let state = match (&version, visual) {
            (None, _) => "unavailable",
            (Some(_), false) => "no_vision",
            (Some(_), true) => "ready",
        };
        sqlx::query("INSERT INTO host_media_presentations VALUES(?,?,?,?)")
            .bind(handle)
            .bind(ordinal as i64)
            .bind(&version)
            .bind(state)
            .execute(&mut *conn)
            .await?;
        status.push(json!({"ordinal":ordinal,"state":state,"artifact_id":version}));
    }
    if images.len() > 8 {
        status.push(json!({"state":"image_limit"}));
    }
    body["data"]["images"] = json!(status);
    Ok(())
}
