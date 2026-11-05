use crate::Result;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub async fn put(
    conn: &mut SqliteConnection,
    target: &str,
    exec: &str,
    key: &str,
    kind: &str,
    body: &str,
    reply_to: Option<i64>,
) -> Result<i64> {
    if let Some(parent) = reply_to {
        let actual:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM work_mail m JOIN work_runs r ON r.execution=? JOIN work_runs sender ON sender.execution=m.origin_execution WHERE m.id=? AND m.kind='question' AND m.body IS NOT NULL AND m.work=r.work AND sender.work=?)").bind(exec).bind(parent).bind(target).fetch_one(&mut *conn).await?;
        if !actual {
            return Err("reply does not reference this private inbox".into());
        }
    }
    if let Some((id,old_kind,old,reply))=sqlx::query_as::<_,(i64,String,Option<String>,Option<i64>)>("SELECT id,kind,body,reply_to FROM work_mail WHERE work=? AND origin_execution=? AND source_key=?")
        .bind(target).bind(exec).bind(key).fetch_optional(&mut *conn).await?{
        if old.as_deref()!=Some(body)||old_kind!=kind||reply!=reply_to{return Err("private message key conflicts with original".into());}return Ok(id)
    }
    Ok(sqlx::query_scalar("INSERT INTO work_mail(work,origin_execution,source_key,kind,body,reply_to) VALUES(?,?,?,?,?,?) RETURNING id")
        .bind(target).bind(exec).bind(key).bind(kind).bind(body).bind(reply_to).fetch_one(conn).await?)
}

/// This runs in the writer transaction that reserves the next logical request. A refused
/// budget, revoked source, or failed wire assembly rolls the cursor and its input back together.
pub async fn consume(conn: &mut SqliteConnection, work: &str, exec: &str) -> Result<Vec<Value>> {
    let rows=sqlx::query("SELECT m.* FROM work_mail m WHERE m.work=? AND m.id>coalesce((SELECT watermark FROM work_consumption WHERE work=m.work),0) ORDER BY m.id")
        .bind(work).fetch_all(&mut *conn).await?;
    let mut items = Vec::new();
    let mut watermark = None;
    for r in rows {
        let id: i64 = r.try_get("id")?;
        watermark = Some(id);
        let Some(body) = r.try_get::<Option<String>, _>("body")? else {
            continue;
        };
        sqlx::query("INSERT OR IGNORE INTO main_dependencies(execution,event) SELECT ?1,id FROM events WHERE id IN(SELECT source FROM executions WHERE id=?2 UNION SELECT event FROM main_dependencies WHERE execution=?2) AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=events.id)")
            .bind(exec).bind(r.try_get::<String,_>("origin_execution")?).execute(&mut *conn).await?;
        sqlx::query("INSERT OR IGNORE INTO main_dependencies SELECT ?,m.source_event FROM work_wait_mail m WHERE m.mail=? AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=m.source_event)").bind(exec).bind(id).execute(&mut *conn).await?;
        items.push(json!({"id":id,"kind":r.try_get::<String,_>("kind")?,"text":body,"reply_to":r.try_get::<Option<i64>,_>("reply_to")?}));
    }
    if let Some(id) = watermark {
        sqlx::query("INSERT INTO work_consumption VALUES(?,?) ON CONFLICT(work) DO UPDATE SET watermark=max(watermark,excluded.watermark)").bind(work).bind(id).execute(conn).await?;
    }
    Ok(items)
}
