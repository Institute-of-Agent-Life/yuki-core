use super::{Change, scope};
use crate::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub(super) fn summary(row: &sqlx::sqlite::SqliteRow) -> Value {
    json!({"session":row.get::<String,_>("id"),"state":row.get::<String,_>("state"),"epoch":row.get::<i64,_>("epoch"),"next_sequence":row.get::<i64,_>("next_sequence"),"persistence":row.get::<String,_>("persistence"),"context_profile":row.get::<String,_>("context_profile"),"created_at":row.get::<i64,_>("created_at"),"updated_at":row.get::<i64,_>("updated_at")})
}
pub(super) fn active(row: &sqlx::sqlite::SqliteRow) -> Result<()> {
    if row.get::<&str, _>("state") != "active" {
        return Err("isolated session is closed, expired or erased".into());
    }
    Ok(())
}
pub(super) fn epoch(c: &Change, row: &sqlx::sqlite::SqliteRow) -> Result<()> {
    active(row)?;
    if c.params["epoch"] != row.get::<i64, _>("epoch") {
        return Err("original session epoch changed".into());
    }
    Ok(())
}
pub(super) async fn page(
    db: &mut SqliteConnection,
    c: &Change,
    s: &sqlx::sqlite::SqliteRow,
) -> Result<Value> {
    active(s)?;
    let id: &str = s.get("id");
    scope::retain(db, &c.plugin, id).await?;
    let after = c.params["after"].as_i64().unwrap_or(0);
    let rows=sqlx::query("SELECT sequence,role,body,digest,sender FROM plugin_session_messages WHERE plugin=? AND session=? AND epoch=? AND sequence>? AND body IS NOT NULL ORDER BY sequence LIMIT 8")
        .bind(&c.plugin).bind(id).bind(s.get::<i64,_>("epoch")).bind(after).fetch_all(&mut *db).await?;
    let items=rows.iter().map(|r|{
        let body:&str=r.get("body");
        json!({"sequence":r.get::<i64,_>("sequence"),"role":r.get::<String,_>("role"),"sender":r.get::<Option<String>,_>("sender"),"text":body.chars().take(512).collect::<String>(),"complete":body.chars().count()<=512,"sha256":r.get::<String,_>("digest"),"size":body.len()})
    }).collect::<Vec<_>>();
    let next = rows
        .last()
        .map(|r| r.get::<i64, _>("sequence"))
        .unwrap_or(after);
    let sources = sources_summary(db, &c.plugin, id).await?;
    Ok(json!({"session":summary(s),"items":items,"next":next,"sources":sources}))
}
pub(super) async fn read(
    db: &mut SqliteConnection,
    c: &Change,
    s: &sqlx::sqlite::SqliteRow,
) -> Result<Value> {
    active(s)?;
    scope::retain(db, &c.plugin, s.get("id")).await?;
    let (body,digest):(String,String)=sqlx::query_as("SELECT body,digest FROM plugin_session_messages WHERE plugin=? AND session=? AND epoch=? AND sequence=? AND body IS NOT NULL")
        .bind(&c.plugin).bind(s.get::<&str,_>("id")).bind(s.get::<i64,_>("epoch")).bind(c.params["sequence"].as_i64()).fetch_one(&mut *db).await?;
    if c.params["sha256"] != digest {
        return Err("original session content changed".into());
    }
    let offset = c.params["offset"].as_u64().unwrap_or(0) as usize;
    if offset > body.len() {
        return Err("session content offset outside bytes".into());
    }
    let end = body.len().min(offset.saturating_add(12288));
    let sources = sources_summary(db, &c.plugin, s.get("id")).await?;
    Ok(
        json!({"session":s.get::<&str,_>("id"),"sequence":c.params["sequence"],"sha256":digest,"size":body.len(),"offset":offset,"next_offset":if end<body.len(){Some(end)}else{None},"bytes":STANDARD.encode(&body.as_bytes()[offset..end]),"sources":sources}),
    )
}

pub(super) async fn sources_summary(
    db: &mut SqliteConnection,
    plugin: &str,
    id: &str,
) -> Result<Value> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT source FROM plugin_session_sources WHERE plugin=? AND session=? ORDER BY source",
    )
    .bind(plugin)
    .bind(id)
    .fetch_all(db)
    .await?;
    Ok(
        json!({"ids":ids.iter().take(128).collect::<Vec<_>>(),"count":ids.len(),"complete":ids.len()<=128,"version":crate::memory::scope::hash(&json!(ids))}),
    )
}
pub(super) async fn sources(
    db: &mut SqliteConnection,
    c: &Change,
    s: &sqlx::sqlite::SqliteRow,
) -> Result<Value> {
    active(s)?;
    scope::retain(db, &c.plugin, s.get("id")).await?;
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT source FROM plugin_session_sources WHERE plugin=? AND session=? ORDER BY source",
    )
    .bind(&c.plugin)
    .bind(s.get::<&str, _>("id"))
    .fetch_all(db)
    .await?;
    let version = crate::memory::scope::hash(&json!(ids));
    if c.params["version"] != version {
        return Err("original session dependency set changed".into());
    }
    let after = c.params["after"].as_i64().unwrap_or(0);
    let remaining = ids
        .iter()
        .copied()
        .filter(|id| *id > after)
        .collect::<Vec<_>>();
    let page = remaining.iter().take(128).copied().collect::<Vec<_>>();
    Ok(
        json!({"session":s.get::<&str,_>("id"),"ids":page,"count":ids.len(),"version":version,"next":if remaining.len()>128{page.last().copied()}else{None}}),
    )
}
