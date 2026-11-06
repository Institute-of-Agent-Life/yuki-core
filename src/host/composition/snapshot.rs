use super::Parent;
use crate::{Result, store::Store};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};

pub(super) struct Prepared {
    pub id: String,
    digest: String,
    codec: String,
    bytes: Vec<u8>,
}
pub(super) async fn put(db: &mut SqliteConnection, parent: &Parent, p: &Value) -> Result<Value> {
    let id = p["snapshot"].as_str().ok_or("missing snapshot")?;
    let digest = p["sha256"].as_str().ok_or("missing hash")?;
    let codec = p["codec"].as_str().ok_or("missing codec")?;
    let size = p["size"].as_u64().ok_or("invalid size")?;
    let offset = p["offset"].as_i64().ok_or("invalid offset")?;
    let bytes = STANDARD.decode(p["bytes"].as_str().ok_or("missing bytes")?)?;
    if size > parent.runtime.max_snapshot_bytes as u64
        || size == 0
        || codec != parent.runtime.codec_sha256
        || bytes.is_empty()
        || bytes.len() > 16384
    {
        return Err("snapshot outside pinned codec or resource limit".into());
    }
    let old = sqlx::query("SELECT * FROM code_snapshots WHERE handle=? AND id=?")
        .bind(&parent.handle)
        .bind(id)
        .fetch_optional(&mut *db)
        .await?;
    let next = if let Some(row) = old {
        if row.get::<&str, _>("sha256") != digest
            || row.get::<i64, _>("size") != size as i64
            || row.get::<&str, _>("codec") != codec
        {
            return Err("original snapshot upload conflicts".into());
        }
        if row.get::<bool, _>("complete") {
            let original: String = sqlx::query_scalar(
                "SELECT sha256 FROM code_snapshot_chunks WHERE handle=? AND id=? AND offset=?",
            )
            .bind(&parent.handle)
            .bind(id)
            .bind(offset)
            .fetch_one(&mut *db)
            .await?;
            if original != format!("{:x}", Sha256::digest(&bytes)) {
                return Err("snapshot chunk conflicts".into());
            }
            return Ok(json!({"snapshot":id,"next_offset":size,"complete":true}));
        }
        row.get::<i64, _>("next_offset")
    } else {
        let staged: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM code_snapshots WHERE handle=? AND NOT complete",
        )
        .bind(&parent.handle)
        .fetch_one(&mut *db)
        .await?;
        if offset != 0 || staged >= 2 {
            return Err("snapshot staging capacity or offset invalid".into());
        }
        sqlx::query("INSERT INTO code_snapshots(handle,id,sha256,size,codec) VALUES(?,?,?,?,?)")
            .bind(&parent.handle)
            .bind(id)
            .bind(digest)
            .bind(size as i64)
            .bind(codec)
            .execute(&mut *db)
            .await?;
        0
    };
    if offset < next {
        let old: Vec<u8> = sqlx::query_scalar(
            "SELECT bytes FROM code_snapshot_chunks WHERE handle=? AND id=? AND offset=?",
        )
        .bind(&parent.handle)
        .bind(id)
        .bind(offset)
        .fetch_one(&mut *db)
        .await?;
        if old != bytes {
            return Err("snapshot chunk conflicts".into());
        }
        return Ok(json!({"snapshot":id,"next_offset":next,"complete":false}));
    }
    if offset != next || next + bytes.len() as i64 > size as i64 {
        return Err("snapshot chunk gap or overflow".into());
    }
    let next = next + bytes.len() as i64;
    sqlx::query("INSERT INTO code_snapshot_chunks VALUES(?,?,?,?,?)")
        .bind(&parent.handle)
        .bind(id)
        .bind(offset)
        .bind(format!("{:x}", Sha256::digest(&bytes)))
        .bind(bytes)
        .execute(&mut *db)
        .await?;
    sqlx::query("UPDATE code_snapshots SET next_offset=? WHERE handle=? AND id=?")
        .bind(next)
        .bind(&parent.handle)
        .bind(id)
        .execute(db)
        .await?;
    Ok(json!({"snapshot":id,"next_offset":next,"complete":false}))
}

pub(super) async fn prepare(
    store: &Store,
    plugin: &str,
    generation: &str,
    p: &Value,
) -> Result<Option<Prepared>> {
    let handle = p["handle"].as_str().ok_or("missing parent")?;
    let id = p
        .get("snapshot")
        .or_else(|| p["ack"].get("snapshot"))
        .and_then(Value::as_str)
        .ok_or("missing snapshot")?
        .to_owned();
    let mut read = store.readers().begin().await?;
    let parent = super::parent(&mut read, plugin, generation, handle, true).await?;
    let row=sqlx::query("SELECT sha256,size,codec,next_offset,complete,bytes FROM code_snapshots WHERE handle=? AND id=?")
        .bind(handle).bind(&id).fetch_one(&mut *read).await?;
    if row.get::<i64, _>("size") != row.get::<i64, _>("next_offset") {
        return Err("snapshot upload incomplete".into());
    }
    let digest: String = row.get("sha256");
    let codec: String = row.get("codec");
    let size = row.get::<i64, _>("size") as usize;
    if size > parent.runtime.max_snapshot_bytes || codec != parent.runtime.codec_sha256 {
        return Err("original codec unavailable".into());
    }
    let bytes = if row.get::<bool, _>("complete") {
        let Some(bytes) = row.get::<Option<Vec<u8>>, _>("bytes") else {
            return Ok(None);
        };
        bytes
    } else {
        let chunks: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT bytes FROM code_snapshot_chunks WHERE handle=? AND id=? ORDER BY offset",
        )
        .bind(handle)
        .bind(&id)
        .fetch_all(&mut *read)
        .await?;
        chunks.concat()
    };
    drop(read);
    tokio::task::spawn_blocking(move || {
        if bytes.len() != size || format!("{:x}", Sha256::digest(&bytes)) != digest {
            return Err("snapshot checksum mismatch".into());
        }
        Ok(Some(Prepared {
            id,
            digest,
            codec,
            bytes,
        }))
    })
    .await?
}

pub(super) async fn publish(
    db: &mut SqliteConnection,
    parent: &Parent,
    id: &str,
    prepared: Option<&Prepared>,
) -> Result<()> {
    let data = prepared
        .filter(|v| v.id == id)
        .ok_or("original snapshot unavailable")?;
    let updated=sqlx::query("UPDATE code_snapshots SET complete=1,bytes=? WHERE handle=? AND id=? AND sha256=? AND codec=? AND size=? AND next_offset=size")
        .bind(&data.bytes).bind(&parent.handle).bind(id).bind(&data.digest).bind(&data.codec).bind(data.bytes.len() as i64).execute(&mut *db).await?.rows_affected();
    if updated != 1 {
        return Err("snapshot changed during preparation".into());
    }
    sqlx::query("UPDATE code_snapshot_chunks SET bytes=NULL WHERE handle=? AND id=?")
        .bind(&parent.handle)
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

pub(super) async fn get(db: &mut SqliteConnection, parent: &Parent, p: &Value) -> Result<Value> {
    let id = p["snapshot"].as_str().ok_or("missing snapshot")?;
    let offset = p["offset"].as_i64().ok_or("missing offset")?;
    let row=sqlx::query("SELECT sha256,size,codec,length(bytes) AS available,substr(bytes,?3+1,16384) AS chunk FROM code_snapshots WHERE handle=?1 AND id=?2 AND complete")
        .bind(&parent.handle).bind(id).bind(offset).fetch_one(db).await?;
    let size: i64 = row.get("size");
    if offset > size || offset < 0 || row.get::<Option<i64>, _>("available") != Some(size) {
        return Err("snapshot unavailable or offset invalid".into());
    }
    let bytes: Vec<u8> = row.get("chunk");
    Ok(
        json!({"snapshot":id,"sha256":row.get::<&str,_>("sha256"),"codec":row.get::<&str,_>("codec"),"size":size,"offset":offset,"next_offset":offset+bytes.len() as i64,"bytes":STANDARD.encode(bytes)}),
    )
}

pub(super) async fn prune(db: &mut SqliteConnection, handle: &str) -> Result<()> {
    sqlx::query("UPDATE code_snapshots SET bytes=NULL WHERE handle=?1 AND complete AND id!=(SELECT snapshot FROM code_parents WHERE handle=?1) AND id NOT IN(SELECT snapshot FROM code_children WHERE handle=?1 AND fed_snapshot IS NULL)")
        .bind(handle).execute(db).await?;
    Ok(())
}
