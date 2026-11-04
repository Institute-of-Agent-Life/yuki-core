use super::credentials::Principal;
use crate::{Result, work::personal};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection, sqlite::SqliteRow};

const SELECT: &str = "SELECT a.*,EXISTS(SELECT 1 FROM entries e JOIN assignments s ON s.id=e.actor_stage JOIN persons p ON p.id=s.original_person AND p.enabled WHERE e.id=a.entry AND e.scope='private' AND e.closed_at IS NULL AND s.status='active' AND coalesce(p.current_root,p.id)=?1) AS private_visible FROM personal_artifact_references a";
async fn visible(db: &mut SqliteConnection, who: &Principal, row: &SqliteRow) -> Result<bool> {
    if row.get::<bool, _>("private_visible") {
        return Ok(true);
    }
    let Some(work) = row.get::<Option<String>, _>("work") else {
        return Ok(false);
    };
    match personal::owner(db, &who.root, &work).await {
        Ok(_) => Ok(true),
        Err(err)
            if matches!(
                err.to_string().as_str(),
                "not_found" | "original_work_unavailable"
            ) =>
        {
            Ok(false)
        }
        Err(err) => Err(err),
    }
}
fn item(r: &SqliteRow) -> Result<Value> {
    let ms: Option<i64> = r.try_get("observed_at")?;
    let time =
        chrono::DateTime::from_timestamp_millis(ms.ok_or("missing original delivery timestamp")?)
            .ok_or("invalid delivery timestamp")?;
    Ok(
        json!({"ref":{"event_id":r.get::<i64,_>("event"),"part_index":r.get::<i64,_>("part_index")},"artifact_id":r.get::<String,_>("id"),"version":r.get::<String,_>("digest"),"name":r.get::<String,_>("filename"),"size":r.get::<i64,_>("size"),"media_type":r.get::<String,_>("mime"),"conversation_id":r.get::<String,_>("entry"),"work_id":r.get::<Option<String>,_>("work"),"published_path":null,"created_at":time.to_rfc3339()}),
    )
}
fn category(name: &str, mime: &str) -> &'static str {
    if mime.starts_with("image/") {
        return "image";
    }
    match std::path::Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "py" | "js" | "ts" | "tsx" | "jsx" | "go" | "sh" | "json" | "yaml" | "html" | "css"
        | "sql" | "java" | "c" | "cpp" => "code",
        _ => "document",
    }
}
pub(super) async fn list(
    db: &mut SqliteConnection,
    who: &Principal,
    before: Option<&str>,
    limit: usize,
    kind: &str,
) -> Result<Value> {
    if !(1..=100).contains(&limit) || !matches!(kind, "all" | "code" | "image" | "document") {
        return Err("invalid_page".into());
    }
    let mut pos = before
        .map(|s| -> Result<(i64, i64)> { Ok(serde_json::from_slice(&URL_SAFE_NO_PAD.decode(s)?)?) })
        .transpose()?
        .unwrap_or((i64::MAX, i64::MAX));
    let mut items = Vec::new();
    let mut last = None;
    loop {
        let sql = format!(
            "{SELECT} WHERE (a.event,a.part_index)<(?2,?3) ORDER BY a.event DESC,a.part_index DESC LIMIT 80"
        );
        let page = sqlx::query(&sql)
            .bind(&who.root)
            .bind(pos.0)
            .bind(pos.1)
            .fetch_all(&mut *db)
            .await?;
        for r in &page {
            pos = (r.get("event"), r.get("part_index"));
            if (kind != "all" && category(r.get("filename"), r.get("mime")) != kind)
                || !visible(db, who, r).await?
            {
                continue;
            }
            if items.len() == limit {
                return Ok(json!({"items":items,"next_cursor":last}));
            }
            items.push(item(r)?);
            last = Some(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&pos)?));
        }
        if page.len() < 80 {
            return Ok(json!({"items":items,"next_cursor":null}));
        }
    }
}
pub(super) async fn download(
    db: &mut SqliteConnection,
    who: &Principal,
    id: &str,
    version: &str,
) -> Result<(String, Vec<u8>)> {
    let sql = format!("{SELECT} WHERE a.id=?2 AND a.digest=?3 ORDER BY a.event DESC");
    let rows = sqlx::query(&sql)
        .bind(&who.root)
        .bind(id)
        .bind(version)
        .fetch_all(&mut *db)
        .await?;
    for r in rows {
        if visible(db, who, &r).await? {
            let bytes = sqlx::query_scalar("SELECT bytes FROM media_objects WHERE digest=?")
                .bind(version)
                .fetch_one(db)
                .await?;
            return Ok((r.get("filename"), bytes));
        }
    }
    Err("not_found".into())
}
pub(super) async fn parts(db: &mut SqliteConnection, event: i64) -> Result<Vec<Value>> {
    let rows =
        sqlx::query("SELECT * FROM personal_artifact_references WHERE event=? ORDER BY part_index")
            .bind(event)
            .fetch_all(db)
            .await?;
    Ok(rows.iter().map(|r|json!({"attachment_index":0,"segment_index":r.get::<i64,_>("part_index"),"artifact_id":r.get::<String,_>("id"),"version":r.get::<String,_>("digest"),"filename":r.get::<String,_>("filename"),"name":r.get::<String,_>("filename"),"mime_type":r.get::<String,_>("mime"),"size":r.get::<i64,_>("size"),"kind":r.get::<String,_>("kind")})).collect())
}
