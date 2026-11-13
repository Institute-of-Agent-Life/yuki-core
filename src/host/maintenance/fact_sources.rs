use super::scope::Granted;
use crate::{Result, memory};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
use std::collections::BTreeSet;

pub(super) struct Dependencies {
    pub all: BTreeSet<i64>,
    pub primary: BTreeSet<i64>,
}
pub(super) async fn source_ids(db: &mut SqliteConnection, fact: &str) -> Result<Vec<i64>> {
    Ok(sqlx::query_scalar("SELECT event FROM memory_evidence WHERE fact=?1 AND event IS NOT NULL
      UNION SELECT x.source FROM executions x JOIN memory_evidence e ON e.origin_execution=x.id WHERE e.fact=?1 AND x.source IS NOT NULL
      UNION SELECT d.event FROM main_dependencies d JOIN memory_evidence e ON e.origin_execution=d.execution WHERE e.fact=?1
      UNION SELECT x.source FROM executions x JOIN memory_versions v ON v.execution=x.id JOIN memory_facts f ON f.id=v.fact AND f.revision=v.revision WHERE f.id=?1 AND x.source IS NOT NULL
      UNION SELECT d.event FROM main_dependencies d JOIN memory_versions v ON v.execution=d.execution JOIN memory_facts f ON f.id=v.fact AND f.revision=v.revision WHERE f.id=?1 ORDER BY 1")
      .bind(fact).fetch_all(db).await?)
}
pub(super) async fn version(db: &mut SqliteConnection, fact: &str) -> Result<String> {
    let data:String=sqlx::query_scalar("SELECT json_array(f.kind,f.person,f.space,f.visibility,f.visibility_entry,f.fact_key,f.revision,state.status,state.conflict_state,v.body,v.confidence,d.category,d.kind,d.importance,d.temporal_mode,d.valid_from,d.valid_until) FROM memory_facts f JOIN memory_fact_states state ON state.id=f.id JOIN memory_versions v ON v.fact=f.id AND v.revision=f.revision LEFT JOIN extract_fact_details d ON d.fact=f.id WHERE f.id=?")
        .bind(fact).fetch_one(&mut *db).await?;
    let evidence:Vec<String>=sqlx::query_scalar("SELECT json_array(kind,source_key,source_version,quote,entry,author,origin_execution,event) FROM memory_evidence WHERE fact=? ORDER BY id")
        .bind(fact).fetch_all(&mut *db).await?;
    let mut sources = vec![];
    for source in source_ids(db, fact).await? {
        let value:String=sqlx::query_scalar("SELECT json_array(e.id,e.envelope,(SELECT json_group_array(json_array(segment,text)) FROM (SELECT segment,text FROM history_transcripts WHERE source=e.id ORDER BY segment))) FROM events e WHERE e.id=?")
            .bind(source).fetch_one(&mut *db).await?;
        sources.push(value);
    }
    let receipts:Vec<String>=sqlx::query_scalar("SELECT json_array(e.kind,e.source_key,o.state,o.result,t.result) FROM memory_evidence e LEFT JOIN operations o ON e.kind='tool' AND o.namespace=json_extract(CASE WHEN e.kind='tool' THEN e.source_key ELSE '[]' END,'$[0]') AND o.operation_id=json_extract(CASE WHEN e.kind='tool' THEN e.source_key ELSE '[]' END,'$[1]') LEFT JOIN main_turns t ON e.kind='run' AND t.execution=e.origin_execution WHERE e.fact=? AND e.origin_execution IS NOT NULL ORDER BY e.id")
        .bind(fact).fetch_all(db).await?;
    Ok(memory::scope::hash(&json!([
        data, evidence, sources, receipts
    ])))
}
pub(super) async fn validate(
    db: &mut SqliteConnection,
    g: &Granted,
    refs: &Value,
    preview: Option<&str>,
) -> Result<Dependencies> {
    let mut all = BTreeSet::new();
    let mut primary = BTreeSet::new();
    let mut seen = BTreeSet::new();
    for reference in refs.as_array().into_iter().flatten() {
        let fact = reference["fact"]
            .as_str()
            .ok_or("original fact reference required")?;
        if !seen.insert(fact)
            || memory::scope::visible(db, &g.reader, Some(fact), "", 1)
                .await?
                .is_empty()
        {
            return Err("input fact outside original scope or repeated".into());
        }
        let revision: i64 = sqlx::query_scalar("SELECT revision FROM memory_facts WHERE id=?")
            .bind(fact)
            .fetch_one(&mut *db)
            .await?;
        let current = version(db, fact).await?;
        let transition: Option<(i64, String)> = if let Some(preview) = preview {
            sqlx::query_as("SELECT revision,source_version FROM maintenance_fact_transitions WHERE preview=? AND fact=?").bind(preview).bind(fact).fetch_optional(&mut *db).await?
        } else {
            None
        };
        let original = reference["revision"] == revision && reference["source_version"] == current;
        if !original
            && transition
                .as_ref()
                .is_none_or(|(expected, hash)| *expected != revision || hash != &current)
        {
            return Err("original input fact or evidence changed".into());
        }
        let sources = source_ids(db, fact).await?;
        if reference["role"] == "primary" {
            primary.extend(sources.iter().copied());
        }
        all.extend(sources);
    }
    Ok(Dependencies { all, primary })
}
pub(super) async fn retain(db: &mut SqliteConnection, plugin: &str, refs: &Value) -> Result<()> {
    for reference in refs.as_array().into_iter().flatten() {
        let fact = reference["fact"].as_str().ok_or("fact required")?;
        for source in source_ids(db, fact).await? {
            crate::privacy::source::acquire(
                db,
                plugin,
                &format!("maintenance-fact:{fact}"),
                source,
            )
            .await?;
        }
    }
    Ok(())
}

pub(super) async fn retain_quality(
    db: &mut SqliteConnection,
    plugin: &str,
    fact: &str,
) -> Result<Vec<String>> {
    // Keep a single association for this owner and event. A later erasure still
    // identifies old audit reports even if the fact has since been corrected.
    let mut refs = Vec::new();
    let sources = source_ids(db, fact).await?;
    if sources.len() > 128 {
        return Err("retained quality report exceeds source limit".into());
    }
    for event in sources {
        refs.push(crate::privacy::source::retain_quality(db, plugin, event).await?);
    }
    refs.sort_unstable();
    Ok(refs)
}
