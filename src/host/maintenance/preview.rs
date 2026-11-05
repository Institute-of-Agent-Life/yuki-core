use super::{Change, reads, scope::Granted};
use crate::{
    Result,
    identity::new_id,
    memory::{self, evidence::Source, facts::Edit},
};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::collections::{BTreeMap, BTreeSet};

async fn selected(
    db: &mut SqliteConnection,
    c: &Change,
    g: &Granted,
    refs: &Value,
) -> Result<BTreeMap<i64, memory::extract::source::Event>> {
    let mut events = BTreeMap::new();
    for reference in refs
        .as_array()
        .ok_or("explicit source selection required")?
    {
        let id = reference["event"]
            .as_i64()
            .ok_or("original event ID required")?;
        let item = reads::original(db, c, g, id).await?;
        if reference["version"] != item.version || events.contains_key(&id) {
            return Err("original source version changed or repeated".into());
        }
        events.insert(id, item);
    }
    Ok(events)
}
fn sources(edit: &Edit) -> &[Source] {
    match edit {
        Edit::Add { evidence, .. } | Edit::Correct { evidence, .. } => evidence,
        Edit::Retract { .. } => &[],
    }
}
async fn proofs(
    db: &mut SqliteConnection,
    g: &Granted,
    edit: &Edit,
    primary: &BTreeSet<i64>,
    tools: &Value,
) -> Result<Vec<(Source, String)>> {
    let mut proofs = vec![];
    for source in sources(edit) {
        if let Source::Tool {
            execution,
            operation,
            ..
        } = source
        {
            let receipt: i64 = sqlx::query_scalar(
                "SELECT rowid FROM operations WHERE namespace=? AND operation_id=?",
            )
            .bind(execution)
            .bind(operation)
            .fetch_one(&mut *db)
            .await?;
            if !tools
                .as_array()
                .into_iter()
                .flatten()
                .any(|r| r["receipt"] == receipt && r["role"] == "primary")
            {
                return Err("tool evidence must be in the frozen primary receipt map".into());
            }
        }
        let proof = memory::evidence::resolve(db, &g.reader, &g.policy.actor, source).await?;
        if let Some(event) = proof.event {
            if !primary.contains(&event) {
                return Err("context is not an approved primary source".into());
            }
        } else if let Some(execution) = &proof.execution {
            let dependencies = super::tools::sources(db, execution).await?;
            if dependencies.is_empty() || dependencies.iter().any(|id| !primary.contains(id)) {
                return Err("tool evidence outside approved primary sources".into());
            }
        }
        if let Edit::Add { subject, .. } = edit
            && subject.kind == "self"
            && subject.visibility.as_deref() == Some("global")
        {
            let private: bool =
                sqlx::query_scalar("SELECT scope='private' FROM entries WHERE id=?")
                    .bind(&proof.entry)
                    .fetch_one(&mut *db)
                    .await?;
            if private {
                return Err("private evidence cannot create global SELF".into());
            }
        }
        proofs.push((source.clone(), proof.version));
    }
    Ok(proofs)
}
async fn extraction(
    db: &mut SqliteConnection,
    events: &BTreeMap<i64, memory::extract::source::Event>,
    claim: &Value,
) -> Result<Edit> {
    let source = claim["source_event_id"]
        .as_i64()
        .and_then(|id| events.get(&id))
        .ok_or("claim has no original selected source")?;
    let claim = &claim["claim"];
    let (kind, person, space) = memory::extract::source::subject(db, source, claim)
        .await?
        .ok_or("claim subject outside original extraction aliases")?;
    let quote = claim["evidence_quote"]
        .as_str()
        .ok_or("original extraction quote required")?;
    if quote.trim().is_empty() || !source.content.contains(quote) {
        return Err("quote absent from original extraction source".into());
    }
    Ok(Edit::Add {
        subject: memory::facts::Subject {
            kind,
            person,
            space,
            visibility: None,
            entry: None,
        },
        fact_key: claim["memory_key"]
            .as_str()
            .ok_or("fact key required")?
            .into(),
        body: claim["content"]
            .as_str()
            .ok_or("claim body required")?
            .into(),
        confidence: claim["confidence"].as_f64(),
        evidence: vec![Source::Event {
            id: source.id,
            quote: quote.into(),
        }],
    })
}
pub(super) async fn prepare(db: &mut SqliteConnection, c: &Change, g: &Granted) -> Result<Value> {
    if !g.policy.methods.contains("maintenance.evidence") || g.policy.write_subject.is_none() {
        return Err("review requires explicit evidence and subject authority".into());
    }
    let request = c
        .paid_request
        .as_deref()
        .or_else(|| c.params["request"].as_str())
        .ok_or("paid request required")?;
    let key = c.params["key"].as_str().ok_or("preview key required")?;
    let digest = memory::scope::hash(&c.params);
    if let Some(row)=sqlx::query("SELECT id,digest,state,actions FROM maintenance_previews WHERE plugin=? AND scope=? AND purpose=? AND original_key=?")
        .bind(&c.plugin).bind(c.params["scope"].as_str()).bind(c.params["purpose"].as_str()).bind(key).fetch_optional(&mut *db).await? {
        if row.get::<String,_>("digest")!=digest || row.get::<String,_>("state")=="erased"{return Err("original preview changed or erased".into());}
        selected(db,c,g,&c.params["sources"]).await?;
        super::fact_sources::validate(db,g,&c.params["facts"],Some(row.get("id"))).await?;
        super::tools::original(db,g,row.get("id")).await?;
        return view(db,row.get("id"),true).await;
    }
    let receipt =
        super::super::aux_model::structured_receipt(db, &c.plugin, &c.generation, request).await?;
    if receipt["purpose"] != c.params["purpose"]
        || !g
            .policy
            .routes
            .contains(receipt["route"].as_str().ok_or("original route missing")?)
    {
        return Err("paid receipt outside installed maintenance purpose or route".into());
    }
    let paid_scope:Option<(String,String)>=sqlx::query_as("WITH RECURSIVE family(id) AS(SELECT ?1 UNION SELECT o.parent FROM family f JOIN executions x ON x.id=f.id JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL) SELECT r.scope,r.params FROM family f JOIN maintenance_roots r ON r.execution=f.id LIMIT 1")
        .bind(receipt["execution"].as_str()).fetch_optional(&mut *db).await?;
    if let Some((scope, params)) = paid_scope {
        let original: Value = serde_json::from_str(&params)?;
        if Some(scope.as_str()) != c.params["scope"].as_str()
            || ["sources", "facts", "tools"].into_iter().any(|field| {
                original[field].as_array().map(Vec::as_slice).unwrap_or(&[])
                    != c.params[field].as_array().map(Vec::as_slice).unwrap_or(&[])
            })
        {
            return Err(
                "paid result belongs to another original maintenance scope or source selection"
                    .into(),
            );
        }
    }
    let events = selected(db, c, g, &c.params["sources"]).await?;
    let facts = super::fact_sources::validate(db, g, &c.params["facts"], None).await?;
    super::fact_sources::retain(db, &c.plugin, &c.params["facts"]).await?;
    let mut primary: BTreeSet<i64> = c.params["sources"]
        .as_array()
        .ok_or("sources required")?
        .iter()
        .filter(|v| v["role"] == "primary")
        .filter_map(|v| v["event"].as_i64())
        .collect();
    primary.extend(facts.primary);
    let tools = super::tools::validate(db, g, &c.params["tools"]).await?;
    primary.extend(tools.primary);
    let primary_fact = c.params["facts"]
        .as_array()
        .is_some_and(|refs| refs.iter().any(|r| r["role"] == "primary"));
    if primary.is_empty() && !primary_fact {
        return Err("original primary sources required".into());
    }
    let exec = receipt["execution"]
        .as_str()
        .ok_or("paid execution required")?;
    let paid_sources:BTreeSet<i64>=sqlx::query_scalar("SELECT source FROM executions WHERE id=?1 AND source IS NOT NULL UNION SELECT event FROM main_dependencies WHERE execution=?1")
        .bind(exec).fetch_all(&mut *db).await?.into_iter().collect();
    let expected_sources: BTreeSet<i64> = events
        .keys()
        .copied()
        .chain(facts.all.iter().copied())
        .chain(tools.all.iter().copied())
        .collect();
    if paid_sources != expected_sources {
        return Err("preview must freeze every original paid source".into());
    }
    let extraction_format = c.params["format"] == "extraction";
    if extraction_format {
        let contract: Value =
            serde_json::from_str(include_str!("../../../fixtures/extract/v4-batch.json"))?;
        jsonschema::validator_for(&contract["schema"])?
            .validate(&receipt["result"])
            .map_err(|error| error.to_string())?;
    }
    let proposals = receipt["result"][if extraction_format {
        "claims"
    } else {
        "actions"
    }]
    .as_array()
    .filter(|a| a.len() <= 128)
    .ok_or("bounded paid action array required")?;
    if !proposals.is_empty()
        && g.policy.write_subject.as_ref().is_some_and(|subject| {
            subject.kind == "self" && subject.visibility.as_deref() == Some("global")
        })
    {
        if c.params["facts"]
            .as_array()
            .is_some_and(|refs| !refs.is_empty())
            && facts.all.is_empty()
        {
            return Err("global SELF requires proven source visibility".into());
        }
        for id in &expected_sources {
            let private: bool =
                sqlx::query_scalar("SELECT d.scope='private' FROM events e JOIN entries d ON d.id=e.entry_id WHERE e.id=?")
                    .bind(id)
                    .fetch_one(&mut *db)
                    .await?;
            if private {
                return Err("private paid context cannot produce global SELF".into());
            }
        }
    }
    let id = new_id(db).await?;
    let mut actions = vec![];
    let mut bindings = vec![];
    for (ordinal, value) in proposals.iter().enumerate() {
        let edit = if extraction_format {
            extraction(db, &events, value).await?
        } else {
            serde_json::from_value(value.clone())?
        };
        bindings.extend(proofs(db, g, &edit, &primary, &c.params["tools"]).await?);
        // Reuse the common writer validation in a rolled-back dry run.
        sqlx::query("SAVEPOINT maintenance_preview")
            .execute(&mut *db)
            .await?;
        let check = memory::facts::edit(
            db,
            &g.reader,
            exec,
            &g.policy.actor,
            &format!("preview:{id}:{ordinal}"),
            &edit,
            true,
        )
        .await;
        sqlx::query("ROLLBACK TO maintenance_preview")
            .execute(&mut *db)
            .await?;
        sqlx::query("RELEASE maintenance_preview")
            .execute(&mut *db)
            .await?;
        check?;
        actions.push(serde_json::to_value(edit)?);
    }
    sqlx::query("INSERT INTO maintenance_previews(id,plugin,revision,scope,purpose,original_key,digest,request,execution,actions) VALUES(?,?,?,?,?,?,?,?,?,?)")
        .bind(&id).bind(&c.plugin).bind(&g.revision).bind(c.params["scope"].as_str()).bind(c.params["purpose"].as_str()).bind(key).bind(digest).bind(request).bind(exec).bind(json!(actions).to_string()).execute(&mut *db).await?;
    for reference in c.params["sources"]
        .as_array()
        .ok_or("source list required")?
    {
        sqlx::query("INSERT INTO maintenance_sources VALUES(?,?,?,?)")
            .bind(&id)
            .bind(reference["event"].as_i64())
            .bind(reference["role"].as_str())
            .bind(reference["version"].as_str())
            .execute(&mut *db)
            .await?;
    }
    for reference in c.params["facts"].as_array().into_iter().flatten() {
        sqlx::query("INSERT INTO maintenance_fact_inputs VALUES(?,?,?,?,?)")
            .bind(&id)
            .bind(reference["fact"].as_str())
            .bind(reference["revision"].as_i64())
            .bind(reference["source_version"].as_str())
            .bind(reference["role"].as_str())
            .execute(&mut *db)
            .await?;
        for event in
            super::fact_sources::source_ids(db, reference["fact"].as_str().ok_or("fact required")?)
                .await?
        {
            let version = memory::extract::source::load_reviewed(db, event, "UTC")
                .await?
                .map(|event| event.version)
                .unwrap_or_default();
            sqlx::query("INSERT OR IGNORE INTO maintenance_sources VALUES(?,?,'fact',?)")
                .bind(&id)
                .bind(event)
                .bind(version)
                .execute(&mut *db)
                .await?;
        }
    }
    for reference in c.params["tools"].as_array().into_iter().flatten() {
        let receipt = reference["receipt"].as_i64().ok_or("receipt required")?;
        let value = super::tools::full(db, g, receipt).await?;
        super::tools::retain(db, &c.plugin, &value).await?;
        sqlx::query("INSERT INTO maintenance_tool_inputs VALUES(?,?,?,?)")
            .bind(&id)
            .bind(receipt)
            .bind(reference["version"].as_str())
            .bind(reference["role"].as_str())
            .execute(&mut *db)
            .await?;
        for event in
            super::tools::sources(db, value["execution"].as_str().ok_or("execution required")?)
                .await?
        {
            sqlx::query("INSERT OR IGNORE INTO maintenance_sources VALUES(?,?,'tool','')")
                .bind(&id)
                .bind(event)
                .execute(&mut *db)
                .await?;
        }
    }
    for (ordinal, (source, version)) in bindings.iter().enumerate() {
        sqlx::query("INSERT INTO maintenance_evidence VALUES(?,?,?,?)")
            .bind(&id)
            .bind(ordinal as i64)
            .bind(serde_json::to_string(source)?)
            .bind(version)
            .execute(&mut *db)
            .await?;
    }
    if extraction_format {
        for (ordinal, claim) in proposals.iter().enumerate() {
            sqlx::query("INSERT INTO maintenance_extraction_details VALUES(?,?,?)")
                .bind(&id)
                .bind(ordinal as i64)
                .bind(claim["claim"].to_string())
                .execute(&mut *db)
                .await?;
        }
    }
    view(db, &id, false).await
}
pub(super) async fn apply(db: &mut SqliteConnection, c: &Change, g: &Granted) -> Result<Value> {
    if !g.policy.methods.contains("maintenance.evidence") || g.policy.write_subject.is_none() {
        return Err("apply requires explicit evidence and subject authority".into());
    }
    let preview = c.params["preview"]
        .as_str()
        .ok_or("original preview required")?;
    let operation = c.params["operation"]
        .as_str()
        .ok_or("original apply key required")?;
    let digest = memory::scope::hash(&c.params);
    if let Some((original, result)) = sqlx::query_as::<_, (String, String)>(
        "SELECT digest,result FROM maintenance_apply WHERE plugin=? AND operation=?",
    )
    .bind(&c.plugin)
    .bind(operation)
    .fetch_optional(&mut *db)
    .await?
    {
        if original != digest {
            return Err("original apply arguments conflict".into());
        }
        return Ok(serde_json::from_str(&result)?);
    }
    let row=sqlx::query("SELECT * FROM maintenance_previews WHERE id=? AND plugin=? AND revision=? AND scope=? AND purpose=? AND state='ready'")
        .bind(preview).bind(&c.plugin).bind(&g.revision).bind(c.params["scope"].as_str()).bind(c.params["purpose"].as_str()).fetch_one(&mut *db).await?;
    let refs: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT event,role,version FROM maintenance_sources WHERE preview=? AND role IN('primary','context') ORDER BY event",
    )
    .bind(preview)
    .fetch_all(&mut *db)
    .await?;
    let references = refs
        .iter()
        .map(|(event, role, version)| json!({"event":event,"role":role,"version":version}))
        .collect::<Vec<_>>();
    selected(db, c, g, &json!(references)).await?;
    let fact_inputs:Vec<(String,i64,String,String)>=sqlx::query_as("SELECT fact,revision,source_version,role FROM maintenance_fact_inputs WHERE preview=? ORDER BY fact").bind(preview).fetch_all(&mut *db).await?;
    let input_refs=fact_inputs.iter().map(|(fact,revision,version,role)|json!({"fact":fact,"revision":revision,"source_version":version,"role":role})).collect::<Vec<_>>();
    super::fact_sources::validate(db, g, &json!(input_refs), Some(preview)).await?;
    super::tools::original(db, g, preview).await?;
    let bound: Vec<(String, String)> = sqlx::query_as(
        "SELECT source,version FROM maintenance_evidence WHERE preview=? ORDER BY ordinal",
    )
    .bind(preview)
    .fetch_all(&mut *db)
    .await?;
    for (source, version) in bound {
        let proof = memory::evidence::resolve(
            db,
            &g.reader,
            &g.policy.actor,
            &serde_json::from_str(&source)?,
        )
        .await?;
        if proof.version != version {
            return Err("original tool or event proof changed".into());
        }
    }
    let actions: Vec<Edit> = serde_json::from_str(row.get("actions"))?;
    let approved: BTreeSet<usize> = serde_json::from_value(c.params["approved"].clone())?;
    let mut results = vec![];
    for ordinal in approved {
        let action = actions
            .get(ordinal)
            .ok_or("approval outside original preview")?;
        let result = memory::facts::edit(
            db,
            &g.reader,
            row.get("execution"),
            &g.policy.actor,
            &format!("preview:{preview}:{ordinal}"),
            action,
            true,
        )
        .await?;
        if let Some(raw)=sqlx::query_scalar::<_,String>("SELECT body FROM maintenance_extraction_details WHERE preview=? AND ordinal=? AND body IS NOT NULL")
            .bind(preview).bind(ordinal as i64).fetch_optional(&mut *db).await? {
            memory::extract::details(db,result["fact"].as_str().ok_or("missing applied fact")?,&serde_json::from_str(&raw)?).await?;
        }
        results.push(json!({"ordinal":ordinal,"receipt":result}));
    }
    for result in &results {
        let fact = result["receipt"]["fact"]
            .as_str()
            .ok_or("missing applied fact")?;
        if fact_inputs.iter().any(|input| input.0 == fact) {
            let version = super::fact_sources::version(db, fact).await?;
            sqlx::query("INSERT INTO maintenance_fact_transitions VALUES(?,?,?,?) ON CONFLICT(preview,fact) DO UPDATE SET revision=excluded.revision,source_version=excluded.source_version")
                .bind(preview).bind(fact).bind(result["receipt"]["revision"].as_i64()).bind(version).execute(&mut *db).await?;
        }
    }
    let result = json!({"preview":preview,"operation":operation,"applied":results});
    sqlx::query("INSERT INTO maintenance_apply VALUES(?,?,?,?,?)")
        .bind(&c.plugin)
        .bind(operation)
        .bind(preview)
        .bind(digest)
        .bind(result.to_string())
        .execute(db)
        .await?;
    Ok(result)
}

pub(super) async fn raw_view(db: &mut SqliteConnection, id: &str, original: bool) -> Result<Value> {
    let actions: String =
        sqlx::query_scalar("SELECT actions FROM maintenance_previews WHERE id=? AND state='ready'")
            .bind(id)
            .fetch_one(&mut *db)
            .await?;
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT ordinal,body FROM maintenance_extraction_details WHERE preview=? ORDER BY ordinal",
    )
    .bind(id)
    .fetch_all(db)
    .await?;
    let details = rows
        .into_iter()
        .map(|(ordinal, raw)| {
            Ok(json!({"ordinal":ordinal,"claim":serde_json::from_str::<Value>(&raw)?}))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"preview":id,"actions":serde_json::from_str::<Value>(&actions)?,"extraction_details":details,"original":original}),
    )
}

async fn view(db: &mut SqliteConnection, id: &str, original: bool) -> Result<Value> {
    let mut value = raw_view(db, id, original).await?;
    // The immutable content excludes replay metadata, keeping every page on one digest.
    let payload =
        json!({"actions":value["actions"],"extraction_details":value["extraction_details"]});
    let bytes = serde_json::to_vec(&payload)?;
    value["content"] = super::content::descriptor("preview", id, &bytes);
    value["action_count"] = json!(value["actions"].as_array().ok_or("actions required")?.len());
    value["inline"] = json!(bytes.len() <= 32768);
    if bytes.len() > 32768 {
        value["actions"] = Value::Null;
        value["extraction_details"] = Value::Null;
    }
    Ok(value)
}
