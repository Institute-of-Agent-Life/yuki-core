use super::{Config, GOLD, source};
use crate::{
    Result,
    identity::new_id,
    memory::{evidence, facts, scope},
    model::{
        attempt,
        chat::{self, Task},
    },
    runtime::execution,
};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

async fn context(
    conn: &mut SqliteConnection,
    batch: &str,
    first: &source::Event,
    zone: &str,
) -> Result<Vec<Value>> {
    let history=sqlx::query("SELECT h.source_event,h.outbound FROM history_items h WHERE h.entry=? AND h.visible AND h.id<(SELECT id FROM history_items WHERE source_event=?) ORDER BY h.id DESC LIMIT 8")
        .bind(&first.entry).bind(first.id).fetch_all(&mut *conn).await?;
    let mut values = Vec::new();
    for row in history.iter().rev() {
        if let Some(event) = row.try_get::<Option<i64>, _>("source_event")? {
            if let Some(item) = source::load(conn, event, zone).await?
                && !item.content.trim().is_empty()
            {
                bind_source(conn, batch, &item, "context").await?;
                values.push(json!({"speaker_role":"member","sender_label":item.label,"content":item.content}));
            }
        } else if let Some(id) = row.try_get::<Option<i64>, _>("outbound")? {
            let Some(out)=sqlx::query("SELECT namespace,body FROM social_outbound WHERE id=? AND NOT recalled AND body!='{}'").bind(id).fetch_optional(&mut *conn).await? else{continue};
            let raw: &str = out.try_get("body")?;
            let text = crate::history::outbound_text(raw)?;
            if text.trim().is_empty() {
                continue;
            }
            let version = scope::hash(&serde_json::from_str(raw)?);
            sqlx::query("INSERT INTO extraction_outbound VALUES(?,?,?)")
                .bind(batch)
                .bind(id)
                .bind(version)
                .execute(&mut *conn)
                .await?;
            sqlx::query("INSERT OR IGNORE INTO main_dependencies SELECT (SELECT execution FROM extraction_batches WHERE id=?1),e.id FROM events e WHERE e.id IN(SELECT source FROM executions WHERE id=?2 UNION SELECT event FROM main_dependencies WHERE execution=?2)").bind(batch).bind(out.get::<String,_>("namespace")).execute(&mut *conn).await?;
            values.push(json!({"speaker_role":"bot","sender_label":"Yuki","content":text}));
        }
    }
    Ok(values)
}
async fn bind_source(
    conn: &mut SqliteConnection,
    batch: &str,
    e: &source::Event,
    role: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO extraction_sources(batch,event,role,version,subjects) VALUES(?,?,?,?,?)",
    )
    .bind(batch)
    .bind(e.id)
    .bind(role)
    .bind(&e.version)
    .bind(serde_json::to_string(&e.aliases)?)
    .execute(&mut *conn)
    .await?;
    sqlx::query("INSERT OR IGNORE INTO main_dependencies SELECT execution,?2 FROM extraction_batches WHERE id=?1").bind(batch).bind(e.id).execute(conn).await?;
    Ok(())
}
pub async fn tick(
    conn: &mut SqliteConnection,
    cfg: &Config,
    now: i64,
    foreground: bool,
) -> Result<Value> {
    if let Some(r)=sqlx::query("SELECT j.id,j.execution,j.logical,j.contract,j.task,p.state,p.response,r.request_json FROM extraction_batches j LEFT JOIN model_requests r ON r.id=j.logical LEFT JOIN model_attempts p ON p.logical=j.logical
        WHERE j.state='prepared' ORDER BY j.rowid,p.ordinal DESC LIMIT 1").fetch_optional(&mut *conn).await?{
        let id:String=r.try_get("id")?;
        let logical:String=r.try_get("logical")?;
        let Some(payment)=r.try_get::<Option<String>,_>("state")? else{
            if !super::can_dispatch(conn,r.get("execution")).await?{
                hold(conn,&id,"original_extraction_scope_unavailable").await?;
                return Ok(Value::Null);
            }
            if r.get::<String,_>("contract")!=cfg.profile.contract(){
                hold(conn,&id,"extraction_profile_changed").await?;return Ok(Value::Null);
            }
            return Ok(json!({"logical":logical}));
        };
        if !matches!(payment.as_str(),"paired"|"response"){
            hold(conn,&id,&format!("paid_{payment}")).await?;return Ok(Value::Null);
        }
        let frozen:Value=serde_json::from_str(r.get("task"))?;
        let profile:crate::runtime::main::Config=serde_json::from_value(frozen["profile"].clone())?;
        let task:Task=serde_json::from_value(frozen["task"].clone())?;
        let output=profile.protocol.decode(r.get("response"),Some(&task),&profile,&serde_json::from_str(r.get("request_json"))?)?;
        commit(conn,&id,profile.timezone.as_deref().unwrap_or("Asia/Shanghai"),&output.result).await?;
        return Ok(Value::Null);
    }
    let ready = ready_owner(conn, cfg, now, foreground).await?;
    let Some(ready) = ready else {
        return Ok(Value::Null);
    };
    let owner: String = ready.try_get("owner")?;
    let reason = if ready.get::<i64, _>("count") >= i64::from(cfg.trigger_count) {
        "event_count"
    } else if ready.get::<i64, _>("chars") >= i64::from(cfg.max_characters) {
        "characters"
    } else {
        "age"
    };
    let ids:Vec<i64>=sqlx::query_scalar("SELECT event FROM extraction_events WHERE owner=? AND state='queued' ORDER BY event LIMIT ?").bind(&owner).bind(cfg.max_events).fetch_all(&mut *conn).await?;
    let zone = cfg.profile.timezone.as_deref().unwrap_or("Asia/Shanghai");
    let mut selected = Vec::new();
    let mut characters = 0;
    for event in ids {
        let item = source::load(conn, event, zone).await?;
        let Some(item) = item else {
            sqlx::query("UPDATE extraction_events SET state='erased' WHERE event=?")
                .bind(event)
                .execute(&mut *conn)
                .await?;
            continue;
        };
        if item.content.trim().is_empty() {
            sqlx::query("UPDATE extraction_events SET state='blank' WHERE event=?")
                .bind(event)
                .execute(&mut *conn)
                .await?;
            continue;
        }
        characters += item.content.chars().count();
        if !selected.is_empty() && characters > cfg.max_characters as usize {
            break;
        }
        selected.push(item);
    }
    let Some(first) = selected.first() else {
        return Ok(Value::Null);
    };
    let batch = new_id(conn).await?;
    let exec = new_id(conn).await?;
    let origin = format!("memory-extract:{batch}");
    let logical = format!("{origin}:batch");
    let profile = &cfg.profile;
    sqlx::query("INSERT INTO main_origins(ref,kind,conversation) VALUES(?,'memory_extract',?)")
        .bind(&origin)
        .bind(&first.entry)
        .execute(&mut *conn)
        .await?;
    sqlx::query("INSERT INTO executions(id,origin,state) VALUES(?,?,'running')")
        .bind(&exec)
        .bind(&origin)
        .execute(&mut *conn)
        .await?;
    let task = cfg.task();
    sqlx::query("INSERT INTO extraction_batches(id,execution,owner,logical,contract,task,trigger_reason,state) VALUES(?,?,?,?,?,?,?,'prepared')")
        .bind(&batch).bind(&exec).bind(owner).bind(&logical).bind(profile.contract()).bind(json!({"profile":profile,"task":task}).to_string()).bind(reason).execute(&mut *conn).await?;
    let nearby = context(conn, &batch, first, zone).await?;
    for e in &selected {
        bind_source(conn, &batch, e, "primary").await?;
        sqlx::query("UPDATE extraction_events SET batch=?,state='reserved' WHERE event=? AND state='queued'").bind(&batch).bind(e.id).execute(&mut *conn).await?;
    }
    let input = json!({"events":selected.iter().map(source::Event::payload).collect::<Vec<_>>(),"conversation_context":nearby});
    let mut layout = json!({"model":profile.model,"max_tokens":profile.max_output,"stream":false,"messages":[{"role":"system","content":GOLD["instruction"]},{"role":"user","content":input.to_string()}]});
    chat::configure(&mut layout, Some(&task), vec![]);
    let mut wire = profile.protocol.request(layout, profile, Some(&task))?;
    if profile.protocol.is_gemini() {
        wire["generationConfig"]["temperature"] = json!(0.1);
    } else if !profile.protocol.is_messages() || wire.get("thinking").is_none() {
        wire["temperature"] = json!(0.1);
    }
    let raw = wire.to_string();
    execution::root(
        conn,
        &exec,
        profile.root_slots,
        profile.max_input_units,
        profile.root_tool_calls,
    )
    .await?;
    execution::activate(conn, &exec, &origin, &profile.contract()).await?;
    attempt::apply(
        conn,
        &attempt::Action::Reserve(attempt::Reservation {
            activation: origin,
            logical: logical.clone(),
            input: i64::try_from(raw.len())?,
            output: profile.max_output,
            safe_rate_limit_once: false,
            native_tools: false,
            request: raw,
        }),
    )
    .await?;
    Ok(json!({"logical":logical}))
}
async fn hold(conn: &mut SqliteConnection, id: &str, reason: &str) -> Result<()> {
    sqlx::query(
        "UPDATE extraction_batches SET state='held',diagnostic=? WHERE id=? AND state='prepared'",
    )
    .bind(reason)
    .bind(id)
    .execute(&mut *conn)
    .await?;
    sqlx::query("UPDATE executions SET state='held' WHERE id=(SELECT execution FROM extraction_batches WHERE id=?)").bind(id).execute(conn).await?;
    Ok(())
}
async fn commit(conn: &mut SqliteConnection, id: &str, zone: &str, result: &Value) -> Result<()> {
    let row = sqlx::query(
        "SELECT execution,owner FROM extraction_batches WHERE id=? AND state='prepared'",
    )
    .bind(id)
    .fetch_one(&mut *conn)
    .await?;
    let exec: String = row.try_get("execution")?;
    let mut primary = std::collections::BTreeMap::new();
    let rows = sqlx::query(
        "SELECT event,role,version FROM extraction_sources WHERE batch=? ORDER BY event",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    for r in rows {
        let e = source::load(conn, r.get("event"), zone).await?;
        let Some(e) = e.filter(|e| e.version == r.get::<String, _>("version")) else {
            hold(conn, id, "source_version_changed").await?;
            return Ok(());
        };
        if r.get::<String, _>("role") == "primary" {
            primary.insert(e.id, e);
        }
    }
    let rows=sqlx::query("SELECT s.version,o.body,o.recalled FROM extraction_outbound s JOIN social_outbound o ON o.id=s.outbound WHERE s.batch=?").bind(id).fetch_all(&mut *conn).await?;
    for r in rows {
        if r.get::<bool, _>("recalled")
            || scope::hash(&serde_json::from_str(r.get("body"))?) != r.get::<String, _>("version")
        {
            hold(conn, id, "context_outbound_changed").await?;
            return Ok(());
        }
    }
    for (ordinal, item) in result["claims"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let event = item["source_event_id"].as_i64();
        let claim = &item["claim"];
        let mut fact = None;
        let reason = if let Some(e) = event.and_then(|n| primary.get(&n)) {
            let quote = claim["evidence_quote"].as_str().unwrap_or("").trim();
            if quote.is_empty() || !e.content.contains(quote) {
                "not_in_original_event"
            } else if let Some((kind, person, space)) = source::subject(conn, e, claim).await? {
                let proof = evidence::event_proof(conn, e.id, quote).await?;
                if proof.author != e.actor || proof.entry != e.entry {
                    return Err("original author or entry changed".into());
                }
                let subject = facts::Subject {
                    kind,
                    person,
                    space,
                    visibility: None,
                    entry: None,
                };
                let body = claim["content"].as_str().unwrap_or("").trim();
                let key = claim["memory_key"].as_str().unwrap_or("").trim();
                if body.is_empty() || key.is_empty() {
                    "blank_claim"
                } else {
                    let fid = facts::insert_fact(conn, &exec, &subject, "subject", key).await?;
                    evidence::attach(conn, &fid, proof).await?;
                    sqlx::query("INSERT INTO memory_versions(fact,revision,body,confidence,execution) VALUES(?,1,?,?,?)").bind(&fid).bind(body).bind(claim["confidence"].as_f64().unwrap_or(0.8)).bind(&exec).execute(&mut *conn).await?;
                    sqlx::query("INSERT INTO extract_fact_details VALUES(?,?,?,?,?,?,?)")
                        .bind(&fid)
                        .bind(claim["category"].as_str())
                        .bind(claim["kind"].as_str().unwrap_or("fact"))
                        .bind(claim["importance"].as_i64().unwrap_or(3))
                        .bind(claim["temporal_mode"].as_str().unwrap_or("persistent"))
                        .bind(claim["valid_from"].as_str())
                        .bind(claim["valid_until"].as_str())
                        .execute(&mut *conn)
                        .await?;
                    fact = Some(fid);
                    "created"
                }
            } else {
                "subject_not_in_original_scope"
            }
        } else {
            "source_not_in_batch"
        };
        sqlx::query("INSERT INTO extraction_results VALUES(?,?,?,?,?)")
            .bind(id)
            .bind(i64::try_from(ordinal)?)
            .bind(event.filter(|n| primary.contains_key(n)))
            .bind(fact)
            .bind(reason)
            .execute(&mut *conn)
            .await?;
    }
    sqlx::query("UPDATE extraction_events SET state='done' WHERE batch=? AND state='reserved'")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    let last = *primary.keys().last().ok_or("extraction source missing")?;
    sqlx::query("INSERT INTO extraction_watermarks VALUES(?,?) ON CONFLICT(owner) DO UPDATE SET event=max(extraction_watermarks.event,excluded.event)").bind(row.get::<String,_>("owner")).bind(last).execute(&mut *conn).await?;
    sqlx::query("UPDATE extraction_batches SET state='committed' WHERE id=?")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE executions SET state='completed' WHERE id=?")
        .bind(exec)
        .execute(conn)
        .await?;
    Ok(())
}

const READY:&str="SELECT q.owner,count(*) AS count,sum(q.characters) AS chars,min(q.arrived) AS first FROM extraction_events q WHERE q.state='queued' AND NOT EXISTS(SELECT 1 FROM extraction_batches b WHERE b.owner=q.owner AND b.state IN('prepared','held')) GROUP BY q.owner
          HAVING count(*)>=?1 OR sum(q.characters)>=?2 OR min(q.arrived)<=?3 ORDER BY min(q.arrived),min(q.event) LIMIT 1";
const BUSY: &str = "SELECT EXISTS(SELECT 1 FROM main_turns t JOIN executions x ON x.id=t.execution WHERE x.state IN('pending','running') AND NOT x.cancelled) OR EXISTS(SELECT 1 FROM accepted_sources s WHERE NOT EXISTS(SELECT 1 FROM main_inputs i WHERE i.event=s.event_id) AND NOT EXISTS(SELECT 1 FROM executions x WHERE x.source=s.event_id AND x.state NOT IN('pending','running')) AND NOT EXISTS(SELECT 1 FROM invalidated_sources f WHERE f.source=s.event_id))";
async fn ready_owner(
    conn: &mut SqliteConnection,
    cfg: &Config,
    now: i64,
    foreground: bool,
) -> Result<Option<sqlx::sqlite::SqliteRow>> {
    if foreground
        && sqlx::query_scalar::<_, bool>(BUSY)
            .fetch_one(&mut *conn)
            .await?
    {
        return Ok(None);
    }
    Ok(sqlx::query(READY)
        .bind(cfg.trigger_count)
        .bind(cfg.max_characters)
        .bind(now - i64::from(cfg.max_wait_seconds) * 1000)
        .fetch_optional(conn)
        .await?)
}
pub(super) async fn needed(
    conn: &mut SqliteConnection,
    cfg: &Config,
    now: i64,
    foreground: bool,
) -> Result<bool> {
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM extraction_batches WHERE state='prepared')",
    )
    .fetch_one(&mut *conn)
    .await?
    {
        return Ok(true);
    }
    Ok(ready_owner(conn, cfg, now, foreground).await?.is_some())
}
