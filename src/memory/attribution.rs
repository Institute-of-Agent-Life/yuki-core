//! Explicit group attribution is derived from the original admitted QQ event.
//! An author may version their own group-scoped claim, never write as its new subject.
use super::{
    evidence, facts, metadata,
    scope::{self, Reader},
    tools,
};
use crate::Result;
use serde_json::{Value, json};
use sqlx::SqliteConnection;

async fn reply_author(conn: &mut SqliteConnection, event: i64) -> Result<Option<(i64, String)>> {
    let matches: Vec<(i64, String)> = sqlx::query_as(
        "SELECT reply.id,reply.raw_actor FROM events source
         JOIN entries g ON g.id=source.entry_id AND g.scope='group' AND g.closed_at IS NULL
         JOIN json_each(source.envelope,'$.segments') part ON json_extract(part.value,'$.type')='reply'
         JOIN event_locators l ON l.receiver=source.bot_stage AND l.native_id=json_extract(part.value,'$.native_id')
         JOIN events reply ON reply.id=l.event_id AND reply.entry_id=source.entry_id AND reply.bot_stage=source.bot_stage AND reply.id<source.id
         JOIN assignments author ON author.id=reply.actor_stage AND author.kind='person' AND author.original_person=reply.raw_actor AND author.status='active'
         JOIN assignments bot ON bot.id=reply.bot_stage AND bot.status='active'
         JOIN persons p ON p.id=reply.raw_actor AND p.enabled
         JOIN persons root ON root.id=coalesce(p.current_root,p.id) AND root.enabled
         WHERE source.id=? AND json_extract(reply.envelope,'$.kind')='message'
         AND (SELECT count(*) FROM json_each(source.envelope,'$.segments') p WHERE json_extract(p.value,'$.type')='reply')=1
         AND (SELECT count(*) FROM event_locators candidates JOIN events e ON e.id=candidates.event_id WHERE candidates.receiver=source.bot_stage AND candidates.native_id=l.native_id AND e.entry_id=source.entry_id AND e.id<source.id)=1
         AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source IN(source.id,reply.id)) LIMIT 2",
    ).bind(event).fetch_all(conn).await?;
    Ok(match matches.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    })
}

async fn source_actor(
    db: &mut SqliteConnection,
    r: &Reader,
    event: i64,
    reference: &str,
    exec: &str,
) -> Result<(facts::Subject, &'static str)> {
    let (actor,group):(String,String)=sqlx::query_as(
        "SELECT e.raw_actor,g.space FROM events e JOIN entries g ON g.id=e.entry_id AND g.scope='group' AND g.closed_at IS NULL
         JOIN assignments speaker ON speaker.id=e.actor_stage AND speaker.kind='person' AND speaker.original_person=e.raw_actor AND speaker.status='active'
         JOIN assignments bot ON bot.id=e.bot_stage AND bot.status='active' WHERE e.id=? AND g.id=? AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id)")
         .bind(event).bind(&r.base.home).fetch_optional(&mut *db).await?.ok_or("original_group_required")?;
    let (person, authority) = match reference {
        "current_group" => {
            return Ok((
                facts::Subject {
                    kind: "group".into(),
                    person: None,
                    space: Some(group),
                    visibility: None,
                    entry: None,
                },
                "group_report",
            ));
        }
        "current_speaker" => (actor, "explicit"),
        "replied_message_author" => {
            let (reply, person) = reply_author(db, event)
                .await?
                .ok_or("unproved_reply_author")?;
            let root: String =
                sqlx::query_scalar("SELECT coalesce(current_root,id) FROM persons WHERE id=?")
                    .bind(&person)
                    .fetch_one(&mut *db)
                    .await?;
            sqlx::query("INSERT OR IGNORE INTO main_dependencies(execution,event) VALUES(?,?)")
                .bind(exec)
                .bind(reply)
                .execute(&mut *db)
                .await?;
            (
                person,
                if root == r.root {
                    "explicit"
                } else {
                    "third_party"
                },
            )
        }
        reference if reference == "mentioned_user" || reference.starts_with("member_") => {
            // Only admitted human Assignment IDs from this event qualify. Exclude the
            // original speaker (and their merged identities); never resolve a display label.
            let unresolved:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM events e JOIN json_each(e.mentions) m LEFT JOIN assignments s ON s.id=json_extract(m.value,'$.assignment') LEFT JOIN persons p ON p.id=s.original_person LEFT JOIN persons root ON root.id=coalesce(p.current_root,p.id) WHERE e.id=? AND (s.id IS NULL OR (s.id!=e.bot_stage AND (s.kind!='person' OR p.id IS NULL OR p.id IS NOT json_extract(m.value,'$.original') OR NOT p.enabled OR NOT root.enabled))))")
                .bind(event).fetch_one(&mut *db).await?;
            if unresolved {
                return Err("unproved_group_mention".into());
            }
            let people:Vec<String>=sqlx::query_scalar("SELECT DISTINCT p.id FROM events e JOIN json_each(e.mentions) m JOIN assignments s ON s.id=json_extract(m.value,'$.assignment') AND s.kind='person' JOIN persons p ON p.id=s.original_person AND p.id=json_extract(m.value,'$.original') JOIN persons root ON root.id=coalesce(p.current_root,p.id) WHERE e.id=? AND p.enabled AND root.enabled AND root.id!=? ORDER BY p.id").bind(event).bind(&r.root).fetch_all(&mut *db).await?;
            let person = if let Some(selected) = reference.strip_prefix("member_") {
                people
                    .into_iter()
                    .find(|p| p == selected)
                    .ok_or("unproved_group_subject")?
            } else {
                let [one] = people.as_slice() else {
                    return Err("mentioned_user_is_ambiguous".into());
                };
                one.clone()
            };
            (person, "third_party")
        }
        _ => return Err("unproved_group_subject".into()),
    };
    Ok((
        facts::Subject {
            kind: "person_group".into(),
            person: Some(person),
            space: Some(group),
            visibility: None,
            entry: None,
        },
        authority,
    ))
}

pub(super) async fn apply(
    db: &mut SqliteConnection,
    r: &Reader,
    exec: &str,
    op: &str,
    req: &Value,
    mut patch: metadata::Patch,
) -> Result<Value> {
    let moved = req["operation"] == "reassign";
    let reference = req["target"]["subject_ref"]
        .as_str()
        .ok_or("missing_subject")?;
    let group_report =
        req["target"]["scope_type"] == "group" && reference == "current_group" && !moved;
    if !group_report && req["target"]["scope_type"] != "person_group"
        || (moved && reference == "current_speaker")
        || !req["selector"].is_null()
        || (!moved && !req["fact_id"].is_null())
    {
        return Err("explicit_group_target_required".into());
    }
    let source = tools::original_event(db, exec, req).await?;
    let evidence::Source::Event { id: event, .. } = &source else {
        return Err("original_group_event_required".into());
    };
    let mut proof = evidence::resolve(db, r, &r.actor, &source).await?;
    let (subject, authority) = source_actor(db, r, *event, reference, exec).await?;
    if moved && authority == "explicit" {
        return Err("operation_not_allowed_for_target".into());
    }
    if reference == "replied_message_author" {
        proof.execution = Some(exec.into());
    }
    let old = if moved {
        if req["expected_fact_state"]
            .as_str()
            .is_some_and(|v| v != "active")
        {
            return Err("expected_fact_state_mismatch".into());
        }
        let id:String=sqlx::query_scalar("SELECT f.id FROM memory_facts f JOIN memory_attributions a ON a.fact=f.id AND a.authority='explicit' JOIN persons p ON p.id=f.person AND p.enabled WHERE f.number=? AND f.kind='person_group' AND f.space=? AND coalesce(p.current_root,p.id)=? AND f.status='active'")
            .bind(req["fact_id"].as_i64().ok_or("original fact ID required")?).bind(&subject.space).bind(&r.root).fetch_optional(&mut *db).await?.ok_or("original_group_fact_not_owned")?;
        let current = facts::read(db, r, Some(&id), "", 1, true, false).await?["items"][0].clone();
        let paid: Option<(i64, String)> = sqlx::query_as(
            "SELECT revision,proof FROM memory_paid_reads WHERE execution=? AND fact=?",
        )
        .bind(exec)
        .bind(&id)
        .fetch_optional(&mut *db)
        .await?;
        if !paid.is_some_and(|(rev, token)| current["revision"] == rev && current["proof"] == token)
            || current["status"] != "active"
        {
            return Err("original_read_unavailable_or_changed".into());
        }
        let m = &current["metadata"];
        patch.memory_key = patch
            .memory_key
            .or_else(|| current["fact_key"].as_str().map(str::to_owned));
        patch.category = patch
            .category
            .or_else(|| m["category"].as_str().map(str::to_owned));
        patch.kind = patch.kind.or_else(|| m["kind"].as_str().map(str::to_owned));
        patch.importance = patch.importance.or(m["importance"].as_i64());
        if patch.valid_from.is_none() && patch.valid_until.is_none() {
            patch.valid_from = m["valid_from"].as_str().map(str::to_owned);
            patch.valid_until = m["valid_until"].as_str().map(str::to_owned);
        }
        Some(current)
    } else {
        None
    };
    let body = metadata::normalize(
        req["new_content"]
            .as_str()
            .or_else(|| old.as_ref().and_then(|v| v["body"].as_str()))
            .ok_or("fact content required")?,
    );
    let key = patch.memory_key.as_deref().ok_or("memory_key required")?;
    if body.is_empty()
        || key.trim().is_empty()
        || patch
            .category
            .as_deref()
            .is_none_or(|c| c.trim().is_empty())
    {
        return Err("memory_content_key_and_category_required".into());
    }
    let id = facts::insert_fact(db, exec, &subject, "subject", key).await?;
    sqlx::query(
        "INSERT INTO memory_versions(fact,revision,body,confidence,execution) VALUES(?,1,?,?,?)",
    )
    .bind(&id)
    .bind(&body)
    .bind(req["confidence"].as_f64().unwrap_or(0.9))
    .bind(exec)
    .execute(&mut *db)
    .await?;
    metadata::apply(db, &id, &patch).await?;
    metadata::temporal(db, &id, exec, &patch, true).await?;
    evidence::attach(db, &id, proof).await?;
    let prior = old.as_ref().and_then(|v| v["fact"].as_str());
    if let (Some(former), Some(old)) = (prior, old.as_ref()) {
        let sources = old["evidence"]
            .as_array()
            .ok_or("missing_source_evidence")?;
        let retained: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM memory_evidence WHERE fact=? AND quote IS NOT NULL",
        )
        .bind(former)
        .fetch_one(&mut *db)
        .await?;
        if sources.is_empty() || i64::try_from(sources.len())? != retained {
            return Err("missing_source_evidence".into());
        }
        for row in sources {
            // Do not copy private out-of-group evidence into a third-party assertion.
            if row["entry"] != r.base.home || row["kind"] != "event" {
                return Err("source_evidence_outside_group".into());
            }
            sqlx::query("INSERT OR IGNORE INTO memory_evidence(fact,kind,source_key,source_version,entry,author,origin_execution,event,quote) SELECT ?,kind,source_key,source_version,entry,author,origin_execution,event,quote FROM memory_evidence WHERE fact=? AND id=? AND quote IS NOT NULL").bind(&id).bind(former).bind(row["id"].as_i64()).execute(&mut *db).await?;
        }
        let rev = old["revision"]
            .as_i64()
            .ok_or("old revision missing")?
            .checked_add(1)
            .ok_or("revision overflow")?;
        sqlx::query("UPDATE memory_facts SET status='retracted',revision=? WHERE id=? AND revision=? AND status='active' RETURNING id").bind(rev).bind(former).bind(rev-1).fetch_one(&mut *db).await?;
        sqlx::query("INSERT INTO memory_versions(fact,revision,body,confidence,execution) VALUES(?,?,?,?,?)").bind(former).bind(rev).bind(old["body"].as_str()).bind(old["confidence"].as_f64()).bind(exec).execute(&mut *db).await?;
        tools::bind_sources(db, exec, former).await?;
    }
    if group_report {
        sqlx::query(
            "UPDATE memory_evidence SET relation='group_statement' WHERE fact=? AND event=?",
        )
        .bind(&id)
        .bind(event)
        .execute(&mut *db)
        .await?;
    } else {
        sqlx::query("UPDATE memory_evidence SET attribution=? WHERE fact=?")
            .bind(authority)
            .bind(&id)
            .execute(&mut *db)
            .await?;
        sqlx::query("INSERT INTO memory_attributions(fact,authority,supersedes) VALUES(?,?,?)")
            .bind(&id)
            .bind(authority)
            .bind(prior)
            .execute(&mut *db)
            .await?;
    }
    sqlx::query("INSERT INTO memory_operations VALUES(?,?,?,?,1,'committed')")
        .bind(exec)
        .bind(op)
        .bind(scope::hash(req))
        .bind(&id)
        .execute(&mut *db)
        .await?;
    tools::bind_sources(db, exec, &id).await?;
    let number: i64 = sqlx::query_scalar("SELECT number FROM memory_facts WHERE id=?")
        .bind(&id)
        .fetch_one(&mut *db)
        .await?;
    Ok(
        json!({"fact":id,"fact_id":number,"old_fact_id":old.as_ref().map(|v|v["fact_id"].clone()),"new_fact_id":number,"revision":1,"outcome":"committed","applied_operation":req["operation"],"reason_code":if moved{"memory_reassigned"}else if group_report{"independent_claim"}else{"created"},"authority":authority}),
    )
}
