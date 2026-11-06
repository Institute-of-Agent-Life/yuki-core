use super::{
    credentials::{self, Principal},
    work,
};
use crate::{
    Result,
    history::query,
    identity::new_id,
    memory::{facts, scope::Reader},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
fn home(w: &Principal) -> Result<&str> {
    w.entry
        .as_deref()
        .ok_or_else(|| "chat_entry_required".into())
}
async fn reader(conn: &mut SqliteConnection, w: &Principal) -> Result<Reader> {
    Ok(Reader {
        archived_evidence: false,
        write_subject: None,
        base: query::for_person(conn, &w.request_person, home(w)?, false).await?,
        root: w.root.clone(),
        actor: w.request_person.clone(),
        delegated: false,
        admin: false,
    })
}
const FACTS:&str="SELECT f.id,f.number,f.kind,v.created_at,d.kind AS detail_kind FROM memory_facts f JOIN memory_versions v ON v.fact=f.id AND v.revision=f.revision LEFT JOIN persons p ON p.id=f.person LEFT JOIN extract_fact_details d ON d.fact=f.id
 WHERE f.status='active' AND (d.valid_until IS NULL OR julianday(d.valid_until)>julianday('now')) AND
 ((f.kind='person' AND p.enabled AND coalesce(p.current_root,p.id)=?1) OR (f.kind='self' AND (f.visibility='global' OR (f.visibility='private' AND f.visibility_entry IN(SELECT e.id FROM entries e JOIN assignments s ON s.id=e.actor_stage JOIN persons q ON q.id=s.original_person WHERE q.enabled AND coalesce(q.current_root,q.id)=?1)))))";
async fn project(
    conn: &mut SqliteConnection,
    w: &Principal,
    row: &sqlx::sqlite::SqliteRow,
    detail: bool,
) -> Result<Value> {
    let r = reader(conn, w).await?;
    let core = facts::read(conn, &r, Some(row.get("id")), "", 1, detail, false).await?;
    let fact = &core["items"][0];
    let mut v = json!({"fact_id":fact["fact_id"],"kind":row.get::<Option<&str>,_>("detail_kind").unwrap_or("fact"),"key":fact["fact_key"],"text":fact["body"],"confidence":fact["confidence"].as_f64().unwrap_or(1.),"revision":fact["proof"],"updated_at":row.get::<&str,_>("created_at"),"evidence_count":fact["readable_evidence"],"scope":fact["kind"],"writable":row.get::<&str,_>("kind")=="person"});
    if detail {
        v["evidence"]=json!(fact["evidence"].as_array().into_iter().flatten().filter_map(|e|{
        if e["kind"]!="event"{return None}
        Some(json!({"conversation_id":e["entry"],"event_id":e["key"].as_str()?.parse::<i64>().ok()?,"excerpt":e["quote"]}))
    }).collect::<Vec<_>>())
    }
    Ok(v)
}
pub async fn fact(conn: &mut SqliteConnection, w: &Principal, num: i64) -> Result<Value> {
    let row = sqlx::query(&format!("{FACTS} AND f.number=?2"))
        .bind(&w.root)
        .bind(num)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or("not_found")?;
    project(conn, w, &row, true).await
}
pub async fn list(
    conn: &mut SqliteConnection,
    w: &Principal,
    about: &str,
    before: Option<&str>,
    limit: i64,
) -> Result<Value> {
    if !matches!(about, "me" | "self") || !(1..=100).contains(&limit) {
        return Err("validation_error".into());
    }
    let position = match before {
        None => i64::MAX,
        Some(raw) => {
            let (scope, n): (String, i64) = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(raw)?)?;
            if scope != about || n < 1 {
                return Err("invalid_cursor".into());
            }
            n
        }
    };
    let rows = sqlx::query(&format!(
        "{FACTS} AND f.kind=?2 AND f.number<?3 ORDER BY f.number DESC LIMIT ?4"
    ))
    .bind(&w.root)
    .bind(if about == "me" { "person" } else { "self" })
    .bind(position)
    .bind(limit + 1)
    .fetch_all(&mut *conn)
    .await?;
    let mut items = Vec::new();
    for row in rows.iter().take(limit as usize) {
        items.push(project(conn, w, row, false).await?);
    }
    let next = if rows.len() > limit as usize {
        items
            .last()
            .map(|v| URL_SAFE_NO_PAD.encode(json!([about, v["fact_id"]]).to_string()))
    } else {
        None
    };
    Ok(json!({"items":items,"next_cursor":next}))
}
pub async fn correct(
    conn: &mut SqliteConnection,
    w: &Principal,
    num: i64,
    body: &Value,
) -> Result<Value> {
    let op = body["request_id"].as_str().ok_or("invalid_request_id")?;
    credentials::uuid(op)?;
    let class = "client.memory_correction";
    let fp = credentials::fingerprint(
        class,
        &num.to_string(),
        &json!({"action":body["action"],"text":body["text"],"expected_revision":body["expected_revision"]}),
    );
    // A replay still requires the original subject, including after a legitimate retraction.
    let (id,status):(String,String)=sqlx::query_as("SELECT f.id,f.status FROM memory_facts f JOIN persons p ON p.id=f.person AND p.enabled WHERE f.kind='person' AND f.number=? AND coalesce(p.current_root,p.id)=? AND f.status!='erased'").bind(num).bind(&w.root).fetch_optional(&mut *conn).await?.ok_or("not_found")?;
    if let Some(v) = prior(conn, w, op, &fp).await? {
        return Ok(v);
    }
    if status != "active" {
        return Err("precondition_failed".into());
    }
    let original = fact(conn, w, num).await?;
    if original["revision"] != body["expected_revision"] {
        return Err("precondition_failed".into());
    }
    let rev: i64 = sqlx::query_scalar("SELECT revision FROM memory_facts WHERE id=?")
        .bind(&id)
        .fetch_one(&mut *conn)
        .await?;
    let proof = original["revision"]
        .as_str()
        .ok_or("missing revision")?
        .to_owned();
    let edit = match body["action"].as_str() {
        Some("correct") => facts::Edit::Correct {
            metadata: None,
            fact: id.clone(),
            revision: rev,
            proof,
            body: body["text"].as_str().ok_or("validation_error")?.into(),
            confidence: None,
            evidence: vec![],
        },
        Some("retract") => facts::Edit::Retract {
            fact: id.clone(),
            revision: rev,
            proof,
        },
        _ => return Err("validation_error".into()),
    };
    let event = work::action_event(conn, w, op, class).await?;
    let exec = new_id(conn).await?;
    sqlx::query("INSERT INTO accepted_sources VALUES(?)")
        .bind(event)
        .execute(&mut *conn)
        .await?;
    sqlx::query("INSERT INTO executions(id,source,original_actor,state) VALUES(?,?,?,'completed')")
        .bind(&exec)
        .bind(event)
        .bind(&w.request_person)
        .execute(&mut *conn)
        .await?;
    let r = reader(conn, w).await?;
    facts::edit(conn, &r, &exec, &w.request_person, op, &edit, false).await?;
    let correct = body["action"] == "correct";
    let fresh = if correct {
        fact(conn, w, num).await?["revision"].clone()
    } else {
        Value::Null
    };
    let result = json!({"event_id":event,"old_fact_id":num,"new_fact_id":if correct{Some(num)}else{None},"applied_operation":body["action"],"revision":fresh,"replay":false});
    record(conn, w, op, (class, &num.to_string(), &fp), &result, event).await?;
    Ok(result)
}
async fn prior(
    conn: &mut SqliteConnection,
    w: &Principal,
    op: &str,
    fp: &str,
) -> Result<Option<Value>> {
    if let Some(row) = super::receipts::before_mutation(conn, w, op).await? {
        let old: &str = row.get("digest");
        let raw: &str = row.get("result");
        if old != fp {
            return Err("request_conflict".into());
        }
        let mut v: Value = serde_json::from_str(raw)?;
        if v.as_object().is_none_or(|m| m.is_empty()) {
            return Err("source_erased".into());
        }
        v["replay"] = json!(true);
        return Ok(Some(v));
    }
    Ok(None)
}
async fn record(
    conn: &mut SqliteConnection,
    w: &Principal,
    op: &str,
    binding: (&str, &str, &str),
    result: &Value,
    event: i64,
) -> Result<()> {
    let (class, target, fp) = binding;
    sqlx::query("INSERT INTO personal_receipts VALUES(?,?,?,?,?,?,?,?)")
        .bind(&w.request_person)
        .bind(w.request_scope())
        .bind(op)
        .bind(class)
        .bind(target)
        .bind(fp)
        .bind(result.to_string())
        .bind(event)
        .execute(conn)
        .await?;
    Ok(())
}
const SCHEDULE:&str="SELECT s.*,v.name,v.goal,v.trigger_json,v.zone FROM schedules s JOIN persons p ON p.id=s.owner AND p.enabled JOIN schedule_versions v ON v.schedule=s.id AND v.version=s.version
 WHERE coalesce(p.current_root,p.id)=?1 AND v.goal IS NOT NULL AND NOT EXISTS(SELECT 1 FROM schedule_sources ss JOIN invalidated_sources i ON i.source=ss.event WHERE ss.schedule=s.id AND ss.version=s.version)";
fn automation(r: &sqlx::sqlite::SqliteRow) -> Result<Value> {
    let id: i64 = r.try_get("id")?;
    let text: &str = r.try_get("trigger_json")?;
    let state: &str = r.try_get("status")?;
    let due = r.try_get::<Option<i64>, _>("next_due")?;
    let revision = credentials::fingerprint(
        "automation",
        &id.to_string(),
        &json!([
            r.get::<i64, _>("version"),
            state,
            text,
            due,
            r.get::<i64, _>("run_count")
        ]),
    );
    Ok(
        json!({"automation_id":id,"conversation_id":r.get::<&str,_>("entry"),"title":r.get::<Option<&str>,_>("name").unwrap_or(r.get("goal")),"schedule_text":text,"state":state,"next_run_at":due.map(|v|credentials::stamp(v/1000)).transpose()?,"revision":revision}),
    )
}
pub async fn schedules(conn: &mut SqliteConnection, w: &Principal) -> Result<Value> {
    let rows = sqlx::query(&format!("{SCHEDULE} ORDER BY s.id DESC"))
        .bind(&w.root)
        .fetch_all(conn)
        .await?;
    Ok(json!({"items":rows.iter().map(automation).collect::<Result<Vec<_>>>()?}))
}
pub async fn change_schedule(
    conn: &mut SqliteConnection,
    w: &Principal,
    id: i64,
    action: &str,
    body: &Value,
) -> Result<Value> {
    let op = body["request_id"].as_str().ok_or("invalid_request_id")?;
    credentials::uuid(op)?;
    let row = sqlx::query(&format!("{SCHEDULE} AND s.id=?2"))
        .bind(&w.root)
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or("not_found")?;
    let fp = credentials::fingerprint(
        "client.automation",
        &id.to_string(),
        &json!([action, body["expected_revision"]]),
    );
    if let Some(v) = prior(conn, w, op, &fp).await? {
        return Ok(v);
    }
    if automation(&row)?["revision"] != body["expected_revision"] {
        return Err("precondition_failed".into());
    }
    let event = work::action_event(conn, w, op, "client.automation").await?;
    let changed =
        crate::work::schedule::state_change(conn, id, action, credentials::now()? * 1000).await?;
    if changed["changed"] != true {
        return Err("precondition_failed".into());
    }
    let row = sqlx::query(&format!("{SCHEDULE} AND s.id=?2"))
        .bind(&w.root)
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    let mut v = automation(&row)?;
    v["replay"] = json!(false);
    record(
        conn,
        w,
        op,
        ("client.automation", &id.to_string(), &fp),
        &v,
        event,
    )
    .await?;
    Ok(v)
}
