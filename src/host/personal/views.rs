use super::{
    credentials::{self, Principal},
    work,
};
use crate::Result;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
async fn entry(conn: &mut SqliteConnection, who: &Principal, id: &str) -> Result<Value> {
    let row=sqlx::query("SELECT e.id,e.actor_stage,a.platform,p.name,e.closed_at,coalesce(w.generation,1) AS generation,coalesce((SELECT max(id) FROM events WHERE entry_id=e.id AND json_extract(envelope,'$.kind')='message'),0) AS latest FROM entries e JOIN assignments s ON s.id=e.actor_stage JOIN accounts a ON a.id=s.account_id JOIN persons p ON p.id=s.original_person AND p.enabled LEFT JOIN main_windows w ON w.conversation=e.id WHERE e.id=? AND e.scope='private' AND coalesce(p.current_root,p.id)=? AND s.status='active' AND e.closed_at IS NULL").bind(id).bind(&who.root).fetch_optional(conn).await?.ok_or("not_found")?;
    Ok(
        json!({"conversation_id":row.get::<&str,_>("id"),"binding_id":row.get::<&str,_>("actor_stage"),"platform":row.get::<&str,_>("platform"),"display_name":row.get::<&str,_>("name"),"status":"active","generation":row.get::<i64,_>("generation"),"revision":row.get::<i64,_>("generation"),"latest_event_id":row.get::<i64,_>("latest")}),
    )
}
pub async fn me(conn: &mut SqliteConnection, who: &Principal) -> Result<Value> {
    let own = who.entry.as_deref().ok_or("chat_entry_required")?;
    let view = entry(conn, who, own).await?;
    let rows=sqlx::query("SELECT a.id,a.status,c.platform,p.name,p.id AS owner FROM assignments a JOIN accounts c ON c.id=a.account_id JOIN persons p ON p.id=a.original_person AND p.enabled WHERE coalesce(p.current_root,p.id)=? ORDER BY a.id").bind(&who.root).fetch_all(&mut *conn).await?;
    let bindings:Vec<_>=rows.iter().map(|r|json!({"binding_id":r.get::<&str,_>("id"),"platform":r.get::<&str,_>("platform"),"display_name":r.get::<&str,_>("name"),"status":r.get::<&str,_>("status"),"source_person_id":r.get::<&str,_>("owner")})).collect();
    let rows=sqlx::query("SELECT d.id,d.purpose,d.label,d.created,d.revoked FROM personal_devices d JOIN device_origins x ON x.device_id=d.id JOIN persons p ON p.id=x.original_person AND p.enabled WHERE coalesce(p.current_root,p.id)=? ORDER BY d.created,d.id").bind(&who.root).fetch_all(&mut *conn).await?;
    let mut devices = Vec::new();
    for r in rows {
        devices.push(json!({"device_id":r.get::<&str,_>("id"),"purpose":r.get::<&str,_>("purpose"),"label":r.get::<&str,_>("label"),"created_at":credentials::stamp(r.get("created"))?,"revoked_at":r.get::<Option<i64>,_>("revoked").map(credentials::stamp).transpose()?}));
    }
    let bot: String = sqlx::query_scalar("SELECT bot_stage FROM entries WHERE id=?")
        .bind(own)
        .fetch_one(conn)
        .await?;
    Ok(
        json!({"person_id":who.root,"source_person_id":who.owner,"request_person_id":who.owner,"request_conversation_id":own,"request_scope":"entry","platform":"app","presence_id":bot,"thread_id":null,"binding_id":view["binding_id"],"display_name":view["display_name"],"conversation_id":own,"is_superuser":who.admin,"bindings":bindings,"devices":devices}),
    )
}
pub async fn works(
    conn: &mut SqliteConnection,
    who: &Principal,
    before: Option<&str>,
    limit: i64,
) -> Result<Value> {
    if !(1..=100).contains(&limit) {
        return Err("invalid_page_size".into());
    }
    let cursor = before
        .map(|v| -> Result<(f64, String)> {
            Ok(serde_json::from_slice(&URL_SAFE_NO_PAD.decode(v)?)?)
        })
        .transpose()?;
    if cursor
        .as_ref()
        .is_some_and(|(t, k)| !t.is_finite() || k.is_empty())
    {
        return Err("invalid_cursor".into());
    }
    let rows:Vec<(String,i64)>=sqlx::query_as("SELECT w.id,w.updated FROM work_nodes w JOIN persons p ON p.id=w.principal AND p.enabled JOIN executions x ON x.id=w.owner_execution JOIN entries e ON e.id=w.entry AND e.closed_at IS NULL WHERE coalesce(p.current_root,p.id)=?1 AND w.goal IS NOT NULL AND NOT EXISTS(SELECT 1 FROM work_sources s JOIN invalidated_sources i ON i.source=s.event WHERE s.work=w.id) AND (?2 IS NULL OR (w.updated,w.id)<(?2,?3)) ORDER BY w.updated DESC,w.id DESC LIMIT ?4")
        .bind(&who.root).bind(cursor.as_ref().map(|(t,_)|(*t*1000.).round() as i64)).bind(cursor.as_ref().map(|(_,k)|k)).bind(limit+1).fetch_all(&mut *conn).await?;
    let mut out = Vec::new();
    for (id, _) in rows.iter().take(limit as usize) {
        out.push(work::project(conn, who, id).await?);
    }
    let next = if rows.len() > limit as usize {
        rows.get(limit as usize - 1)
            .map(|(id, at)| URL_SAFE_NO_PAD.encode(json!([*at as f64 / 1000., id]).to_string()))
    } else {
        None
    };
    Ok(json!({"works":out,"next_before":next}))
}
pub async fn home(conn: &mut SqliteConnection, who: &Principal) -> Result<Value> {
    let main = entry(
        conn,
        who,
        who.entry.as_deref().ok_or("chat_entry_required")?,
    )
    .await?;
    let ids:Vec<String>=sqlx::query_scalar("SELECT e.id FROM entries e JOIN assignments s ON s.id=e.actor_stage AND s.status='active' JOIN persons p ON p.id=s.original_person AND p.enabled WHERE e.scope='private' AND e.closed_at IS NULL AND coalesce(p.current_root,p.id)=? ORDER BY e.id").bind(&who.root).fetch_all(&mut *conn).await?;
    let mut items = Vec::new();
    for id in ids {
        items.push(entry(conn, who, &id).await?);
    }
    let w = works(conn, who, None, 20).await?;
    Ok(
        json!({"main":main,"entries":items,"works":w["works"],"snapshot_at":credentials::stamp(credentials::now()?)?}),
    )
}
pub async fn events(
    conn: &mut SqliteConnection,
    who: &Principal,
    id: &str,
    after: i64,
    before: Option<i64>,
    limit: i64,
) -> Result<Value> {
    let view = entry(conn, who, id).await?;
    if !(1..=100).contains(&limit) || after < 0 {
        return Err("invalid_page_size".into());
    }
    let mut rows=sqlx::query("SELECT e.*,p.name FROM events e LEFT JOIN persons p ON p.id=e.raw_actor WHERE e.entry_id=?1 AND e.id>?2 AND (?3 IS NULL OR e.id<?3) AND json_extract(e.envelope,'$.kind')='message' AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id) ORDER BY CASE WHEN ?3 IS NULL THEN e.id ELSE -e.id END LIMIT ?4").bind(id).bind(after).bind(before).bind(limit+1).fetch_all(&mut *conn).await?;
    let more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    if before.is_some() {
        rows.reverse();
    }
    let mut items = Vec::new();
    for r in rows {
        let event: i64 = r.get("id");
        let text = crate::runtime::main::event_text(r.get("envelope"))?;
        let ts = |ms: i64| {
            chrono::DateTime::from_timestamp_millis(ms)
                .ok_or("invalid event clock")
                .map(|v| v.to_rfc3339())
        };
        let observed = ts(r.get("observed_at"))?;
        let attachments:Vec<_>=crate::media::display(conn,event).await?.into_iter().map(|a|json!({"attachment_index":a["attachment_index"],"segment_index":a["segment_index"],"kind":a["type"]})).collect();
        let transcripts: Vec<(i64, String)> = sqlx::query_as(
            "SELECT segment,text FROM history_transcripts WHERE source=? ORDER BY segment",
        )
        .bind(event)
        .fetch_all(&mut *conn)
        .await?;
        let asr:Vec<_>=transcripts.iter().map(|(segment,text)|json!({"segment_index":segment,"text":text,"source":"transcript"})).collect();
        items.push(json!({"id":event,"event_id":event,"conversation_id":id,"event_kind":"message","origin":view["platform"],"direction":"incoming","text":text,"content":text,"binding_id":view["binding_id"],"platform":view["platform"],"author_kind":if r.get::<Option<String>,_>("raw_actor").is_some(){"person"}else{"bot"},"author_person_id":r.get::<Option<String>,_>("raw_actor"),"sender_display_name":r.get::<Option<String>,_>("name"),"attachments":attachments,"asr":asr,"observed_at":observed,"created_at":observed,"occurred_at":r.get::<Option<i64>,_>("occurred_at").map(ts).transpose()?,"work_ids":[]}));
    }
    Ok(
        json!({"items":items,"revision":view["revision"],"generation":view["generation"],"latest_event_id":view["latest_event_id"],"next_after":items.last().and_then(|x|x["event_id"].as_i64()).unwrap_or(after),"next_before":items.first().and_then(|x|x["event_id"].as_i64()),"has_more":more}),
    )
}
