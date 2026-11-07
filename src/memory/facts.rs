use super::{
    evidence::{self, Source},
    scope::{self, Reader},
};
use crate::{Result, identity::new_id};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    pub kind: String,
    pub person: Option<String>,
    pub space: Option<String>,
    pub visibility: Option<String>,
    pub entry: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    Add {
        subject: Subject,
        fact_key: String,
        body: String,
        confidence: Option<f64>,
        evidence: Vec<Source>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<super::metadata::Patch>,
    },
    Correct {
        fact: String,
        revision: i64,
        proof: String,
        body: String,
        confidence: Option<f64>,
        #[serde(default)]
        evidence: Vec<Source>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metadata: Option<super::metadata::Patch>,
    },
    Retract {
        fact: String,
        revision: i64,
        proof: String,
    },
    Restore {
        fact: String,
        revision: i64,
        proof: String,
        evidence: Vec<Source>,
    },
    Metadata {
        fact: String,
        revision: i64,
        proof: String,
        fields: super::metadata::Patch,
        body: Option<String>,
        confidence: Option<f64>,
        evidence: Vec<Source>,
    },
}
async fn summary(
    conn: &mut SqliteConnection,
    r: &Reader,
    id: &str,
    actor: &str,
    home_only: bool,
    detail: bool,
) -> Result<Value> {
    let row=sqlx::query("SELECT f.*,state.status AS current_status,state.conflict_state,(SELECT authority FROM memory_attributions a WHERE a.fact=f.id) AS attribution,v.body,v.confidence FROM memory_facts f JOIN memory_fact_states state ON state.id=f.id JOIN memory_versions v ON v.fact=f.id AND v.revision=f.revision WHERE f.id=?").bind(id).fetch_one(&mut *conn).await?;
    let revision: i64 = row.try_get("revision")?;
    let evidence = evidence::readable(conn, r, actor, id, home_only).await?;
    let metadata = super::metadata::read(conn, id).await?;
    let binding = scope::hash(&json!([
        id,
        revision,
        &evidence,
        &metadata,
        row.try_get::<String, _>("fact_key")?
    ]));
    let mut result = json!({"fact":id,"fact_id":row.try_get::<i64,_>("number")?,"revision":revision,"status":row.try_get::<String,_>("current_status")?,"conflict_state":row.try_get::<String,_>("conflict_state")?,"kind":row.try_get::<String,_>("kind")?,
       "person":row.try_get::<Option<String>,_>("person")?,"space":row.try_get::<Option<String>,_>("space")?,"visibility":row.try_get::<String,_>("visibility")?,
       "fact_key":row.try_get::<String,_>("fact_key")?,"body":row.try_get::<Option<String>,_>("body")?,"confidence":row.try_get::<Option<f64>,_>("confidence")?,
       "readable_evidence":evidence.len(),"proof":binding,"metadata":metadata});
    if let Some(authority) = row.try_get::<Option<String>, _>("attribution")? {
        result["authority"] = json!(authority);
    }
    if evidence.iter().any(|e| e["relation"] == "group_statement") {
        result["authority"] = json!("group_report");
        result["source_type"] = json!("automatic");
    }
    if detail {
        result["evidence"] = json!(evidence);
    }
    Ok(result)
}

pub async fn read(
    conn: &mut SqliteConnection,
    r: &Reader,
    id: Option<&str>,
    cursor: &str,
    limit: i64,
    detail: bool,
    home_only: bool,
) -> Result<Value> {
    if !(1..=50).contains(&limit) {
        return Err("fact page size out of range".into());
    }
    let ids = scope::visible(conn, r, id, cursor, limit + 1).await?;
    if id.is_some() && ids.is_empty() {
        return Err("fact unavailable".into());
    }
    let mut items = Vec::new();
    for id in ids.iter().take(limit as usize) {
        items.push(summary(conn, r, id, &r.actor, home_only, detail).await?);
    }
    Ok(
        json!({"items":items,"next_cursor":if ids.len()>limit as usize{ids.get(limit as usize-1)}else{None}}),
    )
}
async fn verify_subject(conn: &mut SqliteConnection, r: &Reader, s: &Subject) -> Result<String> {
    let visibility = s.visibility.as_deref().unwrap_or("subject");
    if let Some(granted) = &r.write_subject {
        if !scope::same_subject(conn, granted, s).await? {
            return Err("fact outside installed maintenance subject".into());
        }
    } else if !r.admin {
        let owner: Option<String> = sqlx::query_scalar(
            "SELECT coalesce(current_root,id) FROM persons WHERE id=? AND enabled",
        )
        .bind(&s.person)
        .fetch_optional(&mut *conn)
        .await?;
        if s.kind != "person" || owner.as_deref() != Some(&r.root) {
            return Err("explicit fact creation requires own subject".into());
        }
    }
    if let Some(person) = &s.person {
        sqlx::query_scalar::<_,String>("SELECT p.id FROM persons p JOIN persons root ON root.id=coalesce(p.current_root,p.id) WHERE p.id=? AND p.enabled AND root.enabled").bind(person).fetch_one(&mut *conn).await?;
    }
    if let Some(space) = &s.space {
        let present:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM entries WHERE scope='group' AND space=? AND id IN(SELECT value FROM json_each(?)))").bind(space).bind(serde_json::to_string(&r.base.entries)?).fetch_one(&mut *conn).await?;
        if !present {
            return Err("group outside original scope".into());
        }
    }
    if matches!(visibility, "private" | "group") {
        let entry = s.entry.as_deref().ok_or("SELF visibility entry required")?;
        if entry != r.base.home || !r.admits_entry(entry) {
            return Err("SELF visibility outside current entry".into());
        }
        let kind: String = sqlx::query_scalar("SELECT scope FROM entries WHERE id=?")
            .bind(entry)
            .fetch_one(conn)
            .await?;
        if kind != visibility {
            return Err("SELF visibility entry type differs".into());
        }
    }
    Ok(visibility.into())
}
async fn sources(
    conn: &mut SqliteConnection,
    r: &Reader,
    actor: &str,
    fact: &str,
    items: &[Source],
) -> Result<()> {
    for item in items {
        let proof = evidence::resolve(conn, r, actor, item).await?;
        evidence::attach(conn, fact, proof).await?;
    }
    Ok(())
}
pub(super) async fn insert_fact(
    conn: &mut SqliteConnection,
    exec: &str,
    subject: &Subject,
    visibility: &str,
    key: &str,
) -> Result<String> {
    let id = new_id(conn).await?;
    sqlx::query("INSERT INTO memory_facts(id,kind,person,space,visibility,visibility_entry,fact_key,created_execution,status) VALUES(?,?,?,?,?,?,?,?,'active')").bind(&id).bind(&subject.kind).bind(&subject.person).bind(&subject.space).bind(visibility).bind(&subject.entry).bind(key).bind(exec).execute(conn).await?;
    Ok(id)
}
pub async fn edit(
    conn: &mut SqliteConnection,
    r: &Reader,
    exec: &str,
    actor: &str,
    key: &str,
    edit: &Edit,
    home_only: bool,
) -> Result<Value> {
    if key.trim().is_empty() || key.len() > 256 {
        return Err("original memory operation key required".into());
    }
    let request = scope::hash(&json!([r.admin, &r.base.home, edit]));
    // Replay only the original immutable receipt; both current source and fact access still apply.
    if let Some(row) =
        sqlx::query("SELECT * FROM memory_operations WHERE execution=? AND original_key=?")
            .bind(exec)
            .bind(key)
            .fetch_optional(&mut *conn)
            .await?
    {
        let fact: String = row.try_get("fact")?;
        if row.try_get::<String, _>("request_hash")? != request {
            return Err("original operation arguments conflict".into());
        }
        if scope::visible(conn, r, Some(&fact), "", 1)
            .await?
            .is_empty()
        {
            return Err("fact unavailable".into());
        }
        return Ok(
            json!({"fact":fact,"revision":row.try_get::<i64,_>("revision")?,"outcome":row.try_get::<String,_>("outcome")?}),
        );
    }
    let (fact, revision, status, body, confidence) = match edit {
        Edit::Add {
            subject,
            fact_key,
            body,
            confidence,
            evidence,
            ..
        } => {
            if body.trim().is_empty() || fact_key.trim().is_empty() || evidence.is_empty() {
                return Err("fact text, key and original evidence required".into());
            }
            let visibility = verify_subject(conn, r, subject).await?;
            let id = insert_fact(conn, exec, subject, &visibility, fact_key).await?;
            sources(conn, r, actor, &id, evidence).await?;
            (id, 1, "active", Some(body.clone()), *confidence)
        }
        Edit::Correct {
            fact,
            revision,
            proof,
            ..
        }
        | Edit::Retract {
            fact,
            revision,
            proof,
        }
        | Edit::Restore {
            fact,
            revision,
            proof,
            ..
        }
        | Edit::Metadata {
            fact,
            revision,
            proof,
            ..
        } => {
            if matches!(edit, Edit::Restore { .. }) {
                scope::writable_state(conn, r, fact, "retracted").await?;
            } else {
                scope::writable(conn, r, fact).await?;
            }
            let current = summary(conn, r, fact, actor, home_only, false).await?;
            if current["revision"] != *revision || current["proof"] != proof.as_str() {
                return Err("fact revision or readable evidence changed".into());
            }
            let next = revision.checked_add(1).ok_or("revision overflow")?;
            let (status, body, confidence) = if let Edit::Correct {
                body,
                confidence,
                evidence,
                ..
            } = edit
            {
                if body.trim().is_empty() {
                    return Err("empty correction".into());
                }
                sources(conn, r, actor, fact, evidence).await?;
                (
                    "active",
                    Some(body.clone()),
                    confidence.or(current["confidence"].as_f64()),
                )
            } else if let Edit::Metadata {
                body,
                confidence,
                evidence,
                ..
            } = edit
            {
                if evidence.is_empty() || body.as_ref().is_some_and(|s| s.trim().is_empty()) {
                    return Err(
                        "metadata update requires original evidence and nonempty content".into(),
                    );
                }
                sources(conn, r, actor, fact, evidence).await?;
                (
                    "active",
                    Some(
                        body.clone()
                            .or_else(|| current["body"].as_str().map(str::to_owned))
                            .ok_or("fact content unavailable")?,
                    ),
                    confidence.or(current["confidence"].as_f64()),
                )
            } else if let Edit::Restore { evidence, .. } = edit {
                if evidence.is_empty() {
                    return Err("restoration requires original evidence".into());
                }
                let expired: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM extract_fact_details WHERE fact=? AND valid_until IS NOT NULL AND (julianday(valid_until) IS NULL OR julianday(valid_until)<=julianday('now')))")
                    .bind(fact).fetch_one(&mut *conn).await?;
                if expired {
                    return Err("expired fact cannot be restored".into());
                }
                let retained: String = sqlx::query_scalar("SELECT body FROM memory_versions WHERE fact=? AND revision<? AND body IS NOT NULL ORDER BY revision DESC LIMIT 1")
                    .bind(fact).bind(revision).fetch_optional(&mut *conn).await?
                    .ok_or("original fact content unavailable")?;
                sources(conn, r, actor, fact, evidence).await?;
                ("active", Some(retained), current["confidence"].as_f64())
            } else {
                ("retracted", None, current["confidence"].as_f64())
            };
            sqlx::query("UPDATE memory_facts SET revision=?,status=? WHERE id=? AND revision=? RETURNING id").bind(next).bind(status).bind(fact).bind(revision).fetch_one(&mut *conn).await?;
            if !matches!(edit, Edit::Metadata { .. }) {
                sqlx::query("DELETE FROM memory_conflict_flags WHERE fact=? AND role='original'")
                    .bind(fact)
                    .execute(&mut *conn)
                    .await?;
            }
            (fact.to_owned(), next, status, body, confidence)
        }
    };
    sqlx::query(
        "INSERT INTO memory_versions(fact,revision,body,confidence,execution) VALUES(?,?,?,?,?)",
    )
    .bind(&fact)
    .bind(revision)
    .bind(body)
    .bind(confidence)
    .bind(exec)
    .execute(&mut *conn)
    .await?;
    let metadata = match edit {
        Edit::Add { metadata, .. } | Edit::Correct { metadata, .. } => metadata.as_ref(),
        Edit::Metadata { fields, .. } => Some(fields),
        _ => None,
    };
    if let Some(fields) = metadata {
        super::metadata::apply(conn, &fact, fields).await?;
    }
    // Existing typed writes without a metadata patch leave the extraction
    // projection to its original writer. The model adapter supplies a patch
    // for create/correct, including its explicit empty temporal range.
    let reset = metadata.is_some() && matches!(edit, Edit::Add { .. } | Edit::Correct { .. });
    if reset || metadata.is_some() {
        super::metadata::temporal(
            conn,
            &fact,
            exec,
            metadata.unwrap_or(&Default::default()),
            reset,
        )
        .await?;
    }
    sqlx::query("INSERT INTO memory_operations VALUES(?,?,?,?,?,?)")
        .bind(exec)
        .bind(key)
        .bind(request)
        .bind(&fact)
        .bind(revision)
        .bind(status)
        .execute(conn)
        .await?;
    Ok(json!({"fact":fact,"revision":revision,"outcome":status}))
}
