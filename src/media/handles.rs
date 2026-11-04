use super::blob::Snapshot;
use crate::{Result, history::query, identity::new_id, runtime::effects};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
pub const LIFETIME: i64 = 24 * 60 * 60 * 1000;
pub fn now() -> Result<i64> {
    Ok(i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis(),
    )?)
}
pub async fn display(conn: &mut SqliteConnection, event: i64) -> Result<Vec<Value>> {
    let rows=sqlx::query("SELECT h.slot,h.source,s.segment,s.kind,s.state,s.expires FROM media_handles h JOIN media_sources s ON s.event=h.source AND s.slot=h.source_slot WHERE h.event=? AND s.state!='erased' ORDER BY h.slot").bind(event).fetch_all(conn).await?;
    let at = now()?;
    Ok(rows.iter().map(|r|json!({"attachment_index":r.get::<i64,_>("slot"),"source_event":r.get::<i64,_>("source"),"segment_index":r.get::<i64,_>("segment"),"type":r.get::<String,_>("kind"),"state":if r.get::<Option<i64>,_>("expires").is_some_and(|v|v<=at){"expired"}else{r.get("state")}})).collect())
}
pub async fn original(
    conn: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    event: i64,
    slot: i64,
) -> Result<Value> {
    effects::authorize(conn, exec, actor, true).await?;
    let scope = query::scope(conn, exec, actor).await?;
    let r=sqlx::query("SELECT s.*,e.entry_id,h.slot AS handle FROM media_handles h JOIN media_sources s ON s.event=h.source AND s.slot=h.source_slot
        JOIN events e ON e.id=s.event JOIN events current ON current.id=h.event JOIN assignments stage ON stage.id=s.stage
        WHERE h.event=?1 AND h.slot=?2 AND stage.status='active' AND s.state!='erased' AND current.entry_id=e.entry_id
        AND e.entry_id IN(SELECT value FROM json_each(?3)) AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source IN(h.event,s.event))").bind(event).bind(slot).bind(serde_json::to_string(&scope.entries)?).fetch_one(&mut *conn).await?;
    let expires = r.try_get::<Option<i64>, _>("expires")?;
    let at = now()?;
    let state = r.try_get::<String, _>("state")?;
    Ok(
        json!({"source":r.get::<i64,_>("event"),"slot":r.get::<i64,_>("slot"),"handle":r.get::<i64,_>("handle"),"segment":r.get::<i64,_>("segment"),"stage":r.get::<String,_>("stage"),"kind":r.get::<String,_>("kind"),"origin":r.get::<Option<String>,_>("origin"),"digest":r.get::<Option<String>,_>("digest"),"mime":r.get::<Option<String>,_>("mime"),"expires":expires,"state":if expires.is_some_and(|n|n<=at){"expired"}else{&state}}),
    )
}
pub async fn cache(
    conn: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    event: i64,
    slot: i64,
    proof: &Value,
    data: &Snapshot,
) -> Result<Value> {
    let mime = super::mime(data);
    let current = original(conn, exec, actor, event, slot).await?;
    for name in ["source", "slot", "stage", "origin", "kind"] {
        if current[name] != proof[name] {
            return Err("attachment source changed during acquisition".into());
        }
    }
    if current["state"] == "expired" {
        return Err("original attachment expired".into());
    }
    if current["state"] == "cached" {
        if current["digest"] != data.digest {
            return Err("immutable cached bytes conflict".into());
        }
        return Ok(current);
    }
    store_blob(conn, data).await?;
    sqlx::query("UPDATE media_sources SET digest=?,mime=?,expires=?,acquired_via=?,state='cached' WHERE event=? AND slot=? AND state='unprepared'")
        .bind(&data.digest).bind(mime).bind(now()?.checked_add(LIFETIME).ok_or("media expiry overflow")?).bind(proof["acquired_via"].as_str()).bind(current["source"].as_i64()).bind(current["slot"].as_i64()).execute(&mut *conn).await?;
    original(conn, exec, actor, event, slot).await
}
async fn store_blob(conn: &mut SqliteConnection, data: &Snapshot) -> Result<()> {
    sqlx::query(
        "INSERT INTO media_objects(digest,size,bytes) VALUES(?,?,?) ON CONFLICT(digest) DO NOTHING",
    )
    .bind(&data.digest)
    .bind(i64::try_from(data.data.len())?)
    .bind(&data.data)
    .execute(conn)
    .await?;
    Ok(())
}
pub struct Version<'a> {
    pub exec: &'a str,
    pub actor: &'a str,
    pub op: &'a str,
    pub requested: Option<&'a str>,
    pub basis: &'a str,
    pub filename: &'a str,
    pub kind: &'a str,
    pub mime: &'a str,
    pub source: Option<(i64, i64)>,
}
pub async fn version(
    conn: &mut SqliteConnection,
    v: &Version<'_>,
    data: &Snapshot,
) -> Result<Value> {
    effects::authorize(conn, v.exec, v.actor, true).await?;
    if !matches!(v.kind, "image" | "file")
        || v.op.trim().is_empty()
        || v.filename.is_empty()
        || v.filename
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
        || v.filename == "."
        || v.filename == ".."
    {
        return Err("invalid immutable version name or kind".into());
    }
    if let Some((event, slot)) = v.source {
        let source = original(conn, v.exec, v.actor, event, slot).await?;
        if source["state"] != "cached" || source["digest"] != data.digest {
            return Err("original cached version unavailable".into());
        }
        sqlx::query("INSERT OR IGNORE INTO main_dependencies VALUES(?,?)")
            .bind(v.exec)
            .bind(event)
            .execute(&mut *conn)
            .await?;
        sqlx::query("INSERT OR IGNORE INTO main_dependencies VALUES(?,?)")
            .bind(v.exec)
            .bind(source["source"].as_i64())
            .execute(&mut *conn)
            .await?;
    }
    let old=sqlx::query("SELECT id,digest,filename,kind,source,source_slot,basis FROM media_versions WHERE execution=? AND operation=?").bind(v.exec).bind(v.op).fetch_optional(&mut *conn).await?;
    let id = if let Some(r) = old {
        if r.get::<String, _>("basis") != v.basis
            || r.get::<Option<String>, _>("digest").as_deref() != Some(&data.digest)
            || r.get::<String, _>("filename") != v.filename
            || r.get::<String, _>("kind") != v.kind
            || r.get::<Option<i64>, _>("source")
                .zip(r.get::<Option<i64>, _>("source_slot"))
                != v.source
        {
            return Err("immutable operation already selected another version".into());
        }
        r.get::<String, _>("id")
    } else {
        if let Some(id) = v.requested
            && sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM social_media_bindings WHERE id=?)",
            )
            .bind(id)
            .fetch_one(&mut *conn)
            .await?
        {
            return Err(
                "legacy binding has no verified bytes; create a new immutable version".into(),
            );
        }
        let id = match v.requested {
            Some(id) if !id.is_empty() => id.into(),
            Some(_) => return Err("empty version id".into()),
            None => new_id(conn).await?,
        };
        store_blob(conn, data).await?;
        sqlx::query("INSERT INTO media_versions VALUES(?,?,?,?,?,?,?,?,?,?,?)")
            .bind(&id)
            .bind(v.exec)
            .bind(v.op)
            .bind(v.source.map(|s| s.0))
            .bind(v.source.map(|s| s.1))
            .bind(&data.digest)
            .bind(i64::try_from(data.data.len())?)
            .bind(v.filename)
            .bind(v.mime)
            .bind(v.kind)
            .bind(v.basis)
            .execute(conn)
            .await?;
        id
    };
    Ok(
        json!({"artifact_id":id,"sha256":data.digest,"bytes":data.data.len(),"filename":v.filename,"kind":v.kind,"mime":v.mime}),
    )
}
pub async fn expire(conn: &mut SqliteConnection, at: i64) -> Result<Value> {
    let expired = sqlx::query(
        "UPDATE media_sources SET state='expired',digest=NULL,origin=NULL WHERE state='cached' AND expires<=?",
    )
    .bind(at)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    sqlx::query("UPDATE media_reads SET digest=NULL WHERE digest IS NOT NULL AND EXISTS(SELECT 1 FROM media_sources s WHERE s.event=media_reads.source AND s.slot=media_reads.slot AND s.state IN('expired','erased'))").execute(&mut *conn).await?;
    let freed=sqlx::query("DELETE FROM media_objects WHERE digest NOT IN(SELECT digest FROM media_sources WHERE digest IS NOT NULL UNION SELECT digest FROM media_versions WHERE digest IS NOT NULL UNION SELECT digest FROM media_reads WHERE digest IS NOT NULL)").execute(conn).await?.rows_affected();
    Ok(json!({"expired":expired,"unreferenced_blobs_removed":freed}))
}
