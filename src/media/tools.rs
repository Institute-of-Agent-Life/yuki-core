use super::{Change, Snapshot, acquire, handles, runtime, write};
use crate::{
    Result,
    gateway::qq::Hub,
    model::wire::Protocol,
    runtime::{effects, main::Config as Profile},
    store::Store,
    tools::registry::Tool,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::sync::LazyLock;

static ORIGINAL: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../fixtures/media/v4-index.json"))
        .expect("original media declarations")
});
pub fn declaration(name: &str) -> Option<Tool> {
    if !matches!(
        name,
        "inspect_conversation_attachment" | "workspace_inspect"
    ) {
        return None;
    }
    let item = &ORIGINAL["tools"][name];
    let mut schema = item["parameters"].clone();
    if name == "workspace_inspect" {
        schema["properties"].as_object_mut()?.remove("device_id");
    }
    let description = if name == "workspace_inspect" {
        "按授权核心输入根的相对路径或本次执行的不可变 artifact_id 检查原件；图片只交给具备原生视觉能力的当前模型。"
    } else {
        item["description"].as_str()?
    };
    Some(Tool {
        name: name.into(),
        description: description.into(),
        schema,
        provider: None,
        cacheable: false,
        parallel_read: false,
        code_direct: false,
    })
}
fn material(data: &Snapshot, content_type: &str, vision: bool) -> Value {
    if content_type == "text/plain" {
        let text = std::str::from_utf8(&data.data).expect("verified text");
        let mut chars = text.chars();
        let page: String = chars.by_ref().take(8192).collect();
        json!({"state":"text","text":page,"truncated":chars.next().is_some()})
    } else if content_type.starts_with("image/") && vision {
        json!({"state":"native_image"})
    } else {
        json!({"state":"unread","reason":if content_type.starts_with("image/"){"profile_has_no_vision"}else{"codec_unavailable"}})
    }
}
async fn inspect(
    store: &Store,
    ctx: &Call<'_>,
    name: &str,
    args: &Value,
) -> Result<(Value, Option<Value>)> {
    let Call {
        cfg,
        exec,
        actor,
        op,
        hub,
    } = *ctx;
    effects::authorize(&mut *store.readers().acquire().await?, exec, actor, true).await?;
    if name == "inspect_conversation_attachment" {
        let event = args["event_id"].as_i64().ok_or("event required")?;
        let slot = args["attachment_index"]
            .as_i64()
            .ok_or("attachment slot required")?;
        let original = handles::original(
            &mut *store.readers().acquire().await?,
            exec,
            actor,
            event,
            slot,
        )
        .await?;
        if original["state"] == "expired" || (original["kind"] == "image" && !cfg.vision_input) {
            return Ok((
                json!({"event_id":event,"attachment_index":slot,"state":"unread","reason":if original["state"]=="expired"{"expired"}else{"profile_has_no_vision"}}),
                None,
            ));
        }
        let mut proof = acquire(store, hub, exec, actor, event, slot, None).await?;
        let bytes: Vec<u8> = sqlx::query_scalar("SELECT bytes FROM media_objects WHERE digest=?")
            .bind(proof["digest"].as_str())
            .fetch_one(store.readers())
            .await?;
        let data = Snapshot::new(bytes)?;
        let kind = proof["mime"].as_str().ok_or("missing media type")?;
        let mut out = material(&data, kind, cfg.vision_input);
        out["event_id"] = json!(event);
        out["attachment_index"] = json!(slot);
        out["sha256"] = json!(data.digest);
        out["mime"] = json!(kind);
        proof["event"] = json!(event);
        proof["handle"] = json!(slot);
        return Ok((out, Some(proof)));
    }
    if args.get("device_id").is_some() {
        return Err("device workspace requires its registered owner".into());
    }
    if args.get("path").is_some() == args.get("artifact_id").is_some() {
        return Err("exactly one workspace path or version required".into());
    }
    let id = if let Some(path) = args["path"].as_str() {
        let basis = super::fingerprint(&json!(["workspace_inspect", args]));
        let v = if let Some(v) = super::existing(store, exec, op, &basis).await? {
            v
        } else {
            let root = runtime(store)?.inputs.clone();
            let relative = path.to_owned();
            let data = tokio::task::spawn_blocking(move || root.snapshot(&relative)).await??;
            if args["expected_version"]
                .as_str()
                .is_some_and(|v| v != data.digest)
            {
                return Err("workspace version changed".into());
            }
            let kind = if super::mime(&data).starts_with("image/") {
                "image"
            } else {
                "file"
            };
            write(
                store,
                Change::Version {
                    exec: exec.into(),
                    actor: actor.into(),
                    op: op.into(),
                    requested: None,
                    basis,
                    filename: path.rsplit('/').next().ok_or("missing filename")?.into(),
                    kind: kind.into(),
                    data,
                    source: None,
                },
            )
            .await?
        };
        super::bind(store, exec, actor, &v).await?;
        v["artifact_id"]
            .as_str()
            .ok_or("missing snapshot")?
            .to_owned()
    } else {
        args["artifact_id"]
            .as_str()
            .ok_or("version id required")?
            .into()
    };
    let row=sqlx::query("SELECT v.digest,v.mime,v.filename,b.bytes FROM media_versions v JOIN media_objects b ON b.digest=v.digest WHERE v.id=? AND v.execution=?").bind(&id).bind(exec).fetch_one(store.readers()).await?;
    let data = Snapshot::new(row.get("bytes"))?;
    let mut out = material(&data, row.get("mime"), cfg.vision_input);
    out["artifact_id"] = json!(id);
    out["sha256"] = json!(data.digest);
    out["mime"] = json!(row.get::<String, _>("mime"));
    Ok((
        out,
        Some(json!({"artifact_id":id,"digest":data.digest,"mime":row.get::<String,_>("mime")})),
    ))
}
struct Call<'a> {
    cfg: &'a Profile,
    exec: &'a str,
    actor: &'a str,
    op: &'a str,
    hub: &'a Hub,
}
pub async fn perform(
    store: &Store,
    cfg: &Profile,
    hub: &Hub,
    exec: &str,
    actor: &str,
    op: &str,
) -> Result<()> {
    let row=sqlx::query("SELECT target,payload,state FROM operations WHERE namespace=? AND operation_id=? AND action='external'").bind(exec).bind(op).fetch_one(store.readers()).await?;
    if row.get::<String, _>("state") != "intent" {
        return Ok(());
    }
    let args: Value = serde_json::from_str(row.get("payload"))?;
    let (value, proof, success) = match inspect(
        store,
        &Call {
            cfg,
            hub,
            exec,
            actor,
            op,
        },
        row.get("target"),
        &args,
    )
    .await
    {
        Ok((out, proof)) => (out, proof, true),
        Err(e) => (json!({"error":e.to_string()}), None, false),
    };
    write(
        store,
        Change::Result {
            exec: exec.into(),
            actor: actor.into(),
            op: op.into(),
            value,
            proof,
            success,
        },
    )
    .await?;
    Ok(())
}
pub(super) async fn finish(
    conn: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    op: &str,
    value: &Value,
    proof: Option<&Value>,
    success: bool,
) -> Result<Value> {
    effects::authorize(conn, exec, actor, true).await?;
    if let Some(p) = proof {
        if let Some(event) = p["event"].as_i64() {
            let slot = p["handle"].as_i64().ok_or("missing original attachment")?;
            let original = handles::original(conn, exec, actor, event, slot).await?;
            if original["state"] != "cached" || original["digest"] != p["digest"] {
                return Err("attachment changed before presentation".into());
            }
            for id in [event, original["source"].as_i64().ok_or("missing source")?] {
                sqlx::query("INSERT OR IGNORE INTO main_dependencies VALUES(?,?)")
                    .bind(exec)
                    .bind(id)
                    .execute(&mut *conn)
                    .await?;
            }
            if value["state"] == "native_image" {
                sqlx::query("INSERT OR IGNORE INTO media_reads VALUES(?,?,?,?,?,?)")
                    .bind(exec)
                    .bind(op)
                    .bind(original["source"].as_i64())
                    .bind(original["slot"].as_i64())
                    .bind(p["digest"].as_str())
                    .bind(p["mime"].as_str())
                    .execute(&mut *conn)
                    .await?;
            }
        } else if value["state"] == "native_image" {
            sqlx::query("INSERT OR IGNORE INTO media_version_reads SELECT ?,?,id FROM media_versions WHERE id=? AND execution=? AND digest=?")
                .bind(exec).bind(op).bind(p["artifact_id"].as_str()).bind(exec).bind(p["digest"].as_str()).execute(&mut *conn).await?;
        }
    }
    sqlx::query("UPDATE operations SET state=?,result=? WHERE namespace=? AND operation_id=? AND state='intent'").bind(if success{"confirmed"}else{"failed"}).bind(value.to_string()).bind(exec).bind(op).execute(conn).await?;
    Ok(Value::Null)
}
pub async fn append_image(
    conn: &mut SqliteConnection,
    cfg: &Profile,
    exec: &str,
    op: &str,
    body: &mut Value,
) -> Result<()> {
    if !cfg.vision_input {
        return Ok(());
    }
    let row=sqlx::query("SELECT r.mime,b.bytes FROM media_reads r JOIN media_sources s ON s.event=r.source AND s.slot=r.slot JOIN media_objects b ON b.digest=r.digest WHERE r.execution=?1 AND r.operation=?2 AND s.state='cached' AND s.expires>?3
       UNION ALL SELECT v.mime,b.bytes FROM media_version_reads r JOIN media_versions v ON v.id=r.version JOIN media_objects b ON b.digest=v.digest WHERE r.execution=?1 AND r.operation=?2")
        .bind(exec).bind(op).bind(handles::now()?).fetch_optional(&mut *conn).await?;
    let Some(row) = row else { return Ok(()) };
    let content_type: String = row.try_get("mime")?;
    let encoded = STANDARD.encode(row.get::<Vec<u8>, _>("bytes"));
    native_image(cfg.protocol, body, &content_type, &encoded)
}
fn native_image(p: Protocol, body: &mut Value, mime: &str, encoded: &str) -> Result<()> {
    let uri = format!("data:{mime};base64,{encoded}");
    let (key, part) = if p.is_chat() {
        (
            "messages",
            json!({"role":"user","content":[{"type":"image_url","image_url":{"url":uri}}]}),
        )
    } else if p.is_gemini() {
        (
            "contents",
            json!({"role":"user","parts":[{"inlineData":{"mimeType":mime,"data":encoded}}]}),
        )
    } else if p.is_messages() {
        let image =
            json!({"type":"image","source":{"type":"base64","media_type":mime,"data":encoded}});
        if let Some(v) = body["messages"]
            .as_array_mut()
            .and_then(|msgs| msgs.last_mut())
            .filter(|m| m["role"] == "user")
        {
            v["content"]
                .as_array_mut()
                .ok_or("Messages content missing")?
                .push(image);
            return Ok(());
        }
        ("messages", json!({"role":"user","content":[image]}))
    } else {
        (
            "input",
            json!({"role":"user","content":[{"type":"input_image","image_url":uri}]}),
        )
    };
    body[key]
        .as_array_mut()
        .ok_or("original provider inputs missing")?
        .push(part);
    Ok(())
}
