//! Registration is a scoped command; current use of bytes has its own authorization.
use super::{
    credentials::{self, Principal, Purpose},
    http,
    selection::{self, Selection},
    upload_body,
};
use crate::{Result, store::Store};
use axum::{
    body::Body,
    extract::{Query, State},
    http::HeaderMap,
    response::Response,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use sqlx::{Row, SqliteConnection};
use std::fs::File;

#[derive(Deserialize)]
pub struct Spec {
    pub request_id: String,
    pub sha256: String,
    pub size: i64,
    pub filename: String,
    #[serde(default = "file_kind")]
    pub kind: String,
}
fn file_kind() -> String {
    "file".into()
}
impl Spec {
    fn validate(&self) -> Result<()> {
        credentials::uuid(&self.request_id)?;
        if self.filename.is_empty()
            || self.filename.contains(['/', '\\', ':', '\0', '\r', '\n'])
            || !matches!(self.kind.as_str(), "file" | "image" | "record" | "video")
            || self.size <= 0
            || self.size > i64::from(i32::MAX)
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        {
            return Err("validation_error".into());
        }
        Ok(())
    }
    fn fingerprint(&self, who: &Principal) -> String {
        credentials::fingerprint(
            "client.upload",
            who.request_scope(),
            &json!({"sha256":self.sha256,"size":self.size,"filename":self.filename,"kind":self.kind}),
        )
    }
    fn id(&self, who: &Principal) -> String {
        // RFC 4122 URL namespace, identical to the original uuid5 receipt ID.
        let mut hash = Sha1::new();
        hash.update([
            0x6b, 0xa7, 0xb8, 0x11, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4,
            0x30, 0xc8,
        ]);
        hash.update(format!(
            "yuki:upload:{}:{}",
            who.request_scope(),
            self.request_id
        ));
        let mut b = hash.finalize();
        b[6] = (b[6] & 0x0f) | 0x50;
        b[8] = (b[8] & 0x3f) | 0x80;
        let h: String = b[..16].iter().map(|n| format!("{n:02x}")).collect();
        format!(
            "{}-{}-{}-{}-{}",
            &h[..8],
            &h[8..12],
            &h[12..16],
            &h[16..20],
            &h[20..]
        )
    }
    async fn replay(&self, db: &mut SqliteConnection, who: &Principal) -> Result<Option<Value>> {
        let Some(old) = super::receipts::before_mutation(db, who, &self.request_id).await? else {
            return Ok(None);
        };
        if old.get::<&str, _>("class") != "client.upload"
            || old.get::<&str, _>("digest") != self.fingerprint(who)
        {
            return Err("request_conflict".into());
        }
        let projected = project_receipt(db, who, &old).await?;
        let mut v = projected["result"].clone();
        if projected["found"] != true || v["media_id"].is_null() {
            return Err("not_found".into());
        }
        v["replay"] = json!(true);
        Ok(Some(v))
    }
}
pub(super) async fn upload(
    State(store): State<Store>,
    headers: HeaderMap,
    Query(spec): Query<Spec>,
    body: Body,
) -> Response {
    let result = async {
        spec.validate()?;
        let hash = http::bearer(&headers)?;
        let selection = Selection::headers(&headers)?;
        let issuer = {
            let mut tx = store.readers().begin().await?;
            let who =
                selection::authenticate(&mut tx, &hash, Purpose::Chat, &selection, true).await?;
            if let Some(v) = spec.replay(&mut tx, &who).await? {
                return Ok(v);
            }
            who.entry.ok_or("chat_entry_required")?
        };
        let file = upload_body::receive(body, spec.size, &spec.sha256).await?;
        super::write(
            &store,
            super::Request::Entry {
                token_hash: hash,
                selection,
                purpose: Purpose::Chat,
                command: super::Command::Upload(spec, issuer, file),
            },
        )
        .await
    }
    .await;
    http::reply(result, true)
}
pub(super) async fn register(
    db: &mut SqliteConnection,
    who: &Principal,
    spec: &Spec,
    entry: &str,
    file: &File,
) -> Result<Value> {
    if who.entry.as_deref() != Some(entry) {
        return Err("unauthenticated".into());
    }
    if let Some(old) = spec.replay(db, who).await? {
        return Ok(old);
    }
    let row:Option<i64>=sqlx::query_scalar("INSERT INTO media_objects(digest,size,bytes) VALUES(?1,?2,zeroblob(?2)) ON CONFLICT(digest) DO NOTHING RETURNING rowid").bind(&spec.sha256).bind(spec.size).fetch_optional(&mut *db).await?;
    if let Some(row) = row {
        if upload_body::copy(db, row, spec.size, upload_body::Transfer::Write(file)).await?
            != spec.sha256
        {
            return Err("original_content_unavailable".into());
        }
    } else {
        let (row, stored): (i64, i64) =
            sqlx::query_as("SELECT rowid,size FROM media_objects WHERE digest=?")
                .bind(&spec.sha256)
                .fetch_one(&mut *db)
                .await?;
        if stored != spec.size
            || upload_body::copy(db, row, stored, upload_body::Transfer::Verify).await?
                != spec.sha256
        {
            return Err("original_content_unavailable".into());
        }
    }
    let id = spec.id(who);
    let until = credentials::now()? + 86400;
    let result = json!({"media_id":id,"sha256":spec.sha256,"size":spec.size,"filename":spec.filename,"kind":spec.kind,"expires_at":until});
    sqlx::query(
        "INSERT INTO client_uploads(id,device,owner,entry,digest,expires) VALUES(?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(&who.device)
    .bind(&who.request_person)
    .bind(entry)
    .bind(&spec.sha256)
    .bind(until)
    .execute(&mut *db)
    .await?;
    sqlx::query("INSERT INTO personal_receipts VALUES(?,?,?,'client.upload',?,?,?,NULL)")
        .bind(&who.request_person)
        .bind(who.request_scope())
        .bind(&spec.request_id)
        .bind(&id)
        .bind(spec.fingerprint(who))
        .bind(result.to_string())
        .execute(db)
        .await?;
    let mut result = result;
    result["replay"] = json!(false);
    Ok(result)
}
pub(super) async fn project_receipt(
    db: &mut SqliteConnection,
    who: &Principal,
    row: &sqlx::sqlite::SqliteRow,
) -> Result<Value> {
    let result: Value = serde_json::from_str(row.get("result"))?;
    let Some(id) = result["media_id"].as_str() else {
        return Ok(json!({"found":false}));
    };
    let authorized:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM client_uploads u JOIN persons p ON p.id=u.owner AND p.enabled JOIN device_origins d ON u.device=d.device_id JOIN persons s ON s.id=d.original_person AND s.enabled WHERE u.id=?1 AND coalesce(p.current_root,p.id)=?2 AND coalesce(s.current_root,s.id)=?2)").bind(id).bind(&who.root).fetch_one(db).await?;
    if !authorized {
        return Ok(json!({"found":false}));
    }
    Ok(
        json!({"found":true,"status":"succeeded","operation_kind":"client.upload","target":id,"payload_hash":row.get::<&str,_>("digest"),"result":result,"problem":null,"problem_code":null}),
    )
}

pub(super) struct Part {
    pub id: String,
    pub name: String,
    pub kind: String,
    digest: String,
    mime: String,
}
pub(super) async fn parts(
    db: &mut SqliteConnection,
    who: &Principal,
    ids: &[String],
) -> Result<Vec<Part>> {
    let mut parts = Vec::new();
    let mut verified = std::collections::HashSet::new();
    for id in ids {
        let r=sqlx::query("SELECT u.id,u.digest,b.rowid AS rid,b.size,substr(b.bytes,1,512) prefix,r.result FROM client_uploads u JOIN media_objects b ON b.digest=u.digest JOIN persons p ON p.id=u.owner AND p.enabled JOIN device_origins d ON u.device=d.device_id JOIN persons s ON s.id=d.original_person AND s.enabled JOIN personal_receipts r ON r.owner=u.owner AND r.target=u.id AND r.class='client.upload' WHERE u.id=?1 AND coalesce(p.current_root,p.id)=?2 AND coalesce(s.current_root,s.id)=?2 AND u.expires>?3").bind(id).bind(&who.root).bind(credentials::now()?).fetch_optional(&mut *db).await?.ok_or("not_found")?;
        let receipt: Value = serde_json::from_str(r.get("result"))?;
        if receipt["sha256"] != r.get::<&str, _>("digest")
            || receipt["size"] != r.get::<i64, _>("size")
        {
            return Err("not_found".into());
        }
        let digest: String = r.get("digest");
        if verified.insert(digest.clone())
            && upload_body::copy(
                db,
                r.get("rid"),
                r.get("size"),
                upload_body::Transfer::Verify,
            )
            .await?
                != digest
        {
            return Err("not_found".into());
        }
        parts.push(Part {
            id: id.clone(),
            name: receipt["filename"].as_str().ok_or("not_found")?.into(),
            kind: receipt["kind"].as_str().ok_or("not_found")?.into(),
            digest,
            mime: crate::media::mime_bytes(r.get("prefix")).into(),
        });
    }
    Ok(parts)
}
pub(super) async fn attach(db: &mut SqliteConnection, event: i64, parts: &[Part]) -> Result<()> {
    let until = crate::media::handles::now()? + 86_400_000;
    for (i, part) in parts.iter().enumerate() {
        let slot = i64::try_from(i)?;
        let n=sqlx::query("UPDATE media_sources SET digest=?1,mime=?2,expires=?3,acquired_via='client.upload',state='cached' WHERE event=?4 AND slot=?5 AND segment=?5+1 AND origin=?6 AND state='unprepared'").bind(&part.digest).bind(&part.mime).bind(until).bind(event).bind(slot).bind(&part.id).execute(&mut *db).await?.rows_affected();
        if n != 1 {
            return Err("original_content_unavailable".into());
        }
        sqlx::query("INSERT INTO client_upload_parts(event,slot,media,name) VALUES(?,?,?,?)")
            .bind(event)
            .bind(slot)
            .bind(&part.id)
            .bind(&part.name)
            .execute(&mut *db)
            .await?;
    }
    Ok(())
}
pub(super) async fn metadata(
    db: &mut SqliteConnection,
    event: i64,
    items: &mut [Value],
) -> Result<()> {
    let rows=sqlx::query("SELECT u.slot,u.media,u.name,s.mime,json_extract(r.result,'$.size') size FROM client_upload_parts u JOIN media_sources s ON (s.event,s.slot)=(u.event,u.slot) JOIN personal_receipts r ON r.target=u.media AND r.class='client.upload' WHERE u.event=? AND s.state!='erased'").bind(event).fetch_all(db).await?;
    for row in rows {
        if let Some(item) = items
            .iter_mut()
            .find(|i| i["attachment_index"] == row.get::<i64, _>("slot"))
        {
            item["media_id"] = json!(row.get::<&str, _>("media"));
            item["size"] = json!(row.get::<Option<i64>, _>("size"));
            item["filename"] = json!(row.get::<&str, _>("name"));
            item["mime_type"] = json!(row.get::<Option<&str>, _>("mime"));
        }
    }
    Ok(())
}
#[derive(PartialEq)]
struct Source {
    row: i64,
    size: i64,
    digest: String,
    name: String,
    until: i64,
    media: String,
}
async fn source(
    tx: &mut SqliteConnection,
    h: &HeaderMap,
    entry: &str,
    event: i64,
    slot: i64,
) -> Result<Source> {
    let who = selection::authenticate(
        tx,
        &http::bearer(h)?,
        Purpose::Chat,
        &Selection::headers(h)?,
        false,
    )
    .await?;
    super::views::entry(tx, &who, entry).await?;
    let row=sqlx::query("SELECT b.rowid AS rid,b.size,b.digest,u.name,u.media,s.expires FROM client_upload_parts u JOIN events e ON e.id=u.event JOIN media_sources s ON (s.event,s.slot)=(u.event,u.slot) JOIN media_objects b ON b.digest=s.digest WHERE e.entry_id=? AND e.id=? AND u.slot=? AND s.state='cached' AND s.expires>? AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id)").bind(entry).bind(event).bind(slot).bind(crate::media::handles::now()?).fetch_optional(tx).await?.ok_or("not_found")?;
    Ok(Source {
        row: row.get("rid"),
        size: row.get("size"),
        digest: row.get("digest"),
        name: row.get("name"),
        until: row.get("expires"),
        media: row.get("media"),
    })
}
pub(super) async fn download(
    State(store): State<Store>,
    h: HeaderMap,
    axum::extract::Path((entry, event, slot)): axum::extract::Path<(String, i64, i64)>,
) -> Response {
    use axum::response::IntoResponse;
    use std::io::{Seek, SeekFrom};
    use tokio::io::AsyncReadExt;
    let result: Result<Response> = async {
        let mut file = upload_body::download_stage()?;
        let selected = {
            let mut tx = store.readers().begin().await?;
            let selected = source(&mut tx, &h, &entry, event, slot).await?;
            if upload_body::copy(
                &mut tx,
                selected.row,
                selected.size,
                upload_body::Transfer::Read(&file),
            )
            .await?
                != selected.digest
            {
                return Err("not_found".into());
            }
            selected
        };
        let mut tx = store.readers().begin().await?;
        if source(&mut tx, &h, &entry, event, slot).await? != selected {
            return Err("not_found".into());
        }
        file.seek(SeekFrom::Start(0))?;
        let stream =
            futures_util::stream::try_unfold(tokio::fs::File::from_std(file), |mut f| async move {
                let mut bytes = vec![0; 64 * 1024];
                let n = f.read(&mut bytes).await?;
                bytes.truncate(n);
                Ok::<_, std::io::Error>((n != 0).then_some((bytes, f)))
            });
        let until = chrono::DateTime::from_timestamp_millis(selected.until)
            .ok_or("invalid timestamp")?
            .to_rfc3339_opts(
                if selected.until % 1000 == 0 {
                    chrono::SecondsFormat::Secs
                } else {
                    chrono::SecondsFormat::Micros
                },
                false,
            );
        let filename = selected
            .name
            .bytes()
            .map(|c| {
                if c.is_ascii_alphanumeric() || b"-_.~".contains(&c) {
                    char::from(c).to_string()
                } else {
                    format!("%{c:02X}")
                }
            })
            .collect::<String>();
        Ok((
            [
                (
                    axum::http::header::CONTENT_TYPE,
                    "application/octet-stream".to_string(),
                ),
                (
                    axum::http::header::CONTENT_LENGTH,
                    selected.size.to_string(),
                ),
                (axum::http::header::ETAG, format!("\"{}\"", selected.digest)),
                (
                    axum::http::header::CONTENT_DISPOSITION,
                    format!("attachment; filename*=UTF-8''{filename}"),
                ),
                (
                    axum::http::HeaderName::from_static("x-yuki-expires-at"),
                    until,
                ),
            ],
            Body::from_stream(stream),
        )
            .into_response())
    }
    .await;
    result.unwrap_or_else(|e| http::reply(Err(e), true))
}
