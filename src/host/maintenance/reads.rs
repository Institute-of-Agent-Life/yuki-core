use super::{Change, scope::Granted};
use crate::{Result, memory, privacy::source};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub(super) async fn original(
    db: &mut SqliteConnection,
    c: &Change,
    g: &Granted,
    event: i64,
) -> Result<memory::extract::source::Event> {
    let item = memory::extract::source::load_reviewed(db, event, "UTC")
        .await?
        .ok_or("original maintenance source unavailable")?;
    if !g.reader.admits_entry(&item.entry) {
        return Err("source outside installed maintenance entry".into());
    }
    source::acquire(db, &c.plugin, &format!("maintenance:{event}"), event).await?;
    Ok(item)
}
pub(super) async fn history(db: &mut SqliteConnection, c: &Change, g: &Granted) -> Result<Value> {
    if let Some(event) = c.params["event"].as_i64() {
        let item = original(db, c, g, event).await?;
        if c.params["version"]
            .as_str()
            .is_some_and(|v| v != item.version)
        {
            return Err("source version changed".into());
        }
        let chars: Vec<char> = item.content.chars().collect();
        let start = c.params["offset"].as_u64().unwrap_or(0) as usize;
        if start > chars.len() {
            return Err("source offset outside original text".into());
        }
        let end = chars.len().min(start + 4096);
        return Ok(
            json!({"items":[{"event":event,"entry":item.entry,"actor":item.actor,"version":item.version,"text":chars[start..end].iter().collect::<String>(),"offset":start,"next_offset":if end<chars.len(){Some(end)}else{None}}],"next_event":event}),
        );
    }
    let after = c.params["after"].as_i64().unwrap_or(0);
    let ids:Vec<i64>=sqlx::query_scalar("SELECT e.id FROM events e JOIN persons p ON p.id=e.raw_actor JOIN persons r ON r.id=coalesce(p.current_root,p.id) WHERE e.entry_id=? AND e.id>? AND p.enabled AND r.enabled AND json_extract(e.envelope,'$.kind')='message' AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id) ORDER BY e.id LIMIT 8")
        .bind(&g.policy.entry).bind(after).fetch_all(&mut *db).await?;
    let mut items = vec![];
    for id in &ids {
        let item = original(db, c, g, *id).await?;
        let text: String = item.content.chars().take(512).collect();
        let more = item.content.chars().count() > 512;
        items.push(json!({"event":id,"entry":item.entry,"actor":item.actor,"version":item.version,"text":text,"offset":0,"next_offset":if more{Some(512)}else{None}}));
    }
    Ok(json!({"items":items,"next_event":ids.last().copied().unwrap_or(after)}))
}
pub(super) async fn facts_full(
    db: &mut SqliteConnection,
    c: &Change,
    g: &Granted,
    evidence: bool,
) -> Result<Value> {
    let result = memory::facts::read(
        db,
        &g.reader,
        c.params["fact"].as_str(),
        c.params["after"].as_str().unwrap_or(""),
        8,
        evidence,
        true,
    )
    .await?;
    let mut result = result;
    let evidence_granted = g.policy.methods.contains("maintenance.evidence");
    for item in result["items"].as_array_mut().ok_or("missing fact items")? {
        // The installed maintenance contract has no metadata field. Keep its
        // declared projection; the opaque proof still binds the full fact.
        let projection = item.as_object_mut().ok_or("invalid fact item")?;
        projection.remove("metadata");
        projection.remove("conflict_state");
        if !evidence_granted {
            let object = item.as_object_mut().ok_or("invalid fact item")?;
            object.remove("proof");
            object.remove("readable_evidence");
        }
        let fact = item["fact"]
            .as_str()
            .ok_or("missing original fact")?
            .to_owned();
        item["source_version"] = json!(super::fact_sources::version(db, &fact).await?);
        let sources = super::fact_sources::source_ids(db, &fact).await?;
        item["source_count"] = json!(sources.len());
        item["source_ids"] = json!(sources.iter().take(128).collect::<Vec<_>>());
        item["sources_complete"] = json!(sources.len() <= 128);
        for event in sources {
            source::acquire(db, &c.plugin, &format!("maintenance-fact:{fact}"), event).await?;
        }
    }
    Ok(result)
}
pub(super) async fn audit(db: &mut SqliteConnection, c: &Change, g: &Granted) -> Result<Value> {
    let page = memory::scope::visible(
        db,
        &g.reader,
        c.params["fact"].as_str(),
        c.params["after"].as_str().unwrap_or(""),
        8,
    )
    .await?;
    let mut items = vec![];
    let mut versions = vec![];
    for id in page {
        let row=sqlx::query("SELECT f.revision,state.status,(SELECT count(*) FROM memory_evidence e WHERE e.fact=f.id) AS total,(SELECT count(*) FROM memory_evidence e WHERE e.fact=f.id AND e.quote IS NOT NULL) AS retained,(SELECT count(*) FROM fact_vectors v WHERE v.fact=f.id AND v.revision=f.revision) AS vectors FROM memory_facts f JOIN memory_fact_states state ON state.id=f.id WHERE f.id=?")
            .bind(&id).fetch_one(&mut *db).await?;
        let evidence:Vec<String>=sqlx::query_scalar("SELECT json_array(e.kind,e.source_key,e.source_version,e.quote,source.envelope,(SELECT json_group_array(json_array(segment,text)) FROM (SELECT segment,text FROM history_transcripts WHERE source=e.event ORDER BY segment))) FROM memory_evidence e LEFT JOIN events source ON source.id=e.event WHERE e.fact=? ORDER BY e.id")
            .bind(&id).fetch_all(&mut *db).await?;
        let body:Option<String>=sqlx::query_scalar("SELECT v.body FROM memory_versions v JOIN memory_facts f ON f.id=v.fact AND f.revision=v.revision WHERE f.id=?")
            .bind(&id).fetch_one(&mut *db).await?;
        let profiles:Vec<String>=sqlx::query_scalar("SELECT DISTINCT profile FROM fact_vectors WHERE fact=? AND revision=? ORDER BY profile")
            .bind(&id).bind(row.get::<i64,_>("revision")).fetch_all(&mut *db).await?;
        versions.push(json!([id, body, evidence, profiles]));
        items.push(json!({"fact":id,"revision":row.get::<i64,_>("revision"),"status":row.get::<String,_>("status"),"evidence_total":row.get::<i64,_>("total"),"evidence_retained":row.get::<i64,_>("retained"),"vector_profiles":profiles}));
    }
    let routes=g.embedding_routes.iter().filter(|(name,_)|g.policy.embedding_routes.contains(*name))
        .map(|(name,profile)|json!({"route":name,"profile":profile.identity(),"dimensions":profile.dimensions})).collect::<Vec<_>>();
    Ok(
        json!({"fingerprint":memory::scope::hash(&json!([g.revision,c.params["scope"],c.params["purpose"],c.params["fact"],c.params["after"],items,versions])),"items":items,"embedding_routes":routes,"paid_calls":0}),
    )
}

pub(super) async fn apply_audit(
    db: &mut SqliteConnection,
    c: &Change,
    g: &Granted,
) -> Result<Value> {
    if g.policy.write_subject.is_none() {
        return Err("index maintenance requires explicit subject authority".into());
    }
    let operation = c.params["operation"]
        .as_str()
        .ok_or("original audit operation required")?;
    let digest = memory::scope::hash(&c.params);
    if let Some((original, result)) = sqlx::query_as::<_, (String, String)>(
        "SELECT digest,result FROM maintenance_audit_apply WHERE plugin=? AND operation=?",
    )
    .bind(&c.plugin)
    .bind(operation)
    .fetch_optional(&mut *db)
    .await?
    {
        if original != digest {
            return Err("original audit operation changed".into());
        }
        return Ok(serde_json::from_str(&result)?);
    }
    let current = audit(db, c, g).await?;
    if current["fingerprint"] != c.params["fingerprint"] {
        return Err("original source audit changed".into());
    }
    let visible = current["items"].as_array().ok_or("audit page required")?;
    let approved: std::collections::BTreeSet<String> =
        serde_json::from_value(c.params["approved"].clone())?;
    for id in &approved {
        if !visible.iter().any(|item| item["fact"] == *id) {
            return Err("fact outside original audit page".into());
        }
        memory::scope::writable(db, &g.reader, id).await?;
        sqlx::query("DELETE FROM memory_retrieval_fts WHERE rowid=(SELECT number FROM memory_facts WHERE id=?)").bind(id).execute(&mut *db).await?;
        sqlx::query("INSERT INTO memory_retrieval_fts(rowid,content,memory_key,category) SELECT number,content,fact_key,category FROM retrieval_documents WHERE id=?").bind(id).execute(&mut *db).await?;
    }
    let result = json!({"operation":operation,"fingerprint":c.params["fingerprint"],"rebuilt":approved,"paid_calls":0});
    sqlx::query("INSERT INTO maintenance_audit_apply VALUES(?,?,?,?)")
        .bind(&c.plugin)
        .bind(operation)
        .bind(digest)
        .bind(result.to_string())
        .execute(db)
        .await?;
    Ok(result)
}

pub(super) async fn facts(
    db: &mut SqliteConnection,
    c: &Change,
    g: &Granted,
    evidence: bool,
) -> Result<Value> {
    let mut result = facts_full(db, c, g, evidence).await?;
    for item in result["items"]
        .as_array_mut()
        .ok_or("fact items required")?
    {
        let raw = serde_json::to_vec(item)?;
        let id = item["fact"].as_str().ok_or("fact ID required")?.to_owned();
        let descriptor =
            super::content::descriptor(if evidence { "evidence" } else { "fact" }, &id, &raw);
        for field in ["body", "fact_key"] {
            if let Some(text) = item[field].as_str() {
                let complete = text.chars().count() <= 512;
                item[field] = json!(text.chars().take(512).collect::<String>());
                item[format!("{field}_complete")] = json!(complete);
            }
        }
        if let Some(proofs) = item.get_mut("evidence").and_then(Value::as_array_mut) {
            let count = proofs.len();
            proofs.truncate(4);
            for proof in proofs.iter_mut() {
                if let Some(quote) = proof["quote"].as_str() {
                    let complete = quote.chars().count() <= 512;
                    proof["quote"] = json!(quote.chars().take(512).collect::<String>());
                    proof["quote_complete"] = json!(complete);
                }
            }
            item["evidence_count"] = json!(count);
        }
        item["content"] = descriptor;
        item["inline"] = json!(true);
        if serde_json::to_vec(item)?.len() > 6000 {
            *item = json!({"fact":id,"revision":item["revision"],"source_version":item["source_version"],"source_count":item["source_count"],"source_ids":item["source_ids"],"sources_complete":item["sources_complete"],"content":item["content"],"inline":false});
        }
    }
    Ok(result)
}
