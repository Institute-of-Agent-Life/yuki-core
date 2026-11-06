//! Target-local original selectors. Similarity supplies candidates, never write authority.
use super::scope::Reader;
use crate::Result;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

#[derive(Debug)]
pub(super) struct Rejected {
    reason: &'static str,
    pub facts: Vec<String>,
    candidates: Vec<Value>,
}
impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason)
    }
}
impl std::error::Error for Rejected {}
impl Rejected {
    pub fn payload(&self) -> Value {
        json!({"error":self.reason,"reason_code":self.reason,"candidates":self.candidates})
    }
}
struct Candidate {
    id: String,
    number: i64,
    exact: bool,
    key_exact: bool,
    body_exact: bool,
    updated: String,
    value: Value,
}
fn literal_contains(haystack: &str, needle: &str) -> bool {
    // The original SQLite LIKE comparison folds ASCII only. Percent and
    // underscore are literal because the original query uses autoescape.
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}
fn field(selector: &Value, key: &str) -> Result<Option<String>> {
    match selector.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(super::metadata::normalize(text))),
        _ => Err("invalid_memory_selector".into()),
    }
}
pub(super) async fn locate(db: &mut SqliteConnection, r: &Reader, args: &Value) -> Result<String> {
    let selector = &args["selector"];
    if !args["fact_id"].is_null() || args["operation"] == "create" {
        return Err("conflicting_memory_selector".into());
    }
    if args["target"]["scope_type"] != "person"
        || args["target"]["subject_ref"] != "current_speaker"
    {
        return Err("memory_selector_target_required".into());
    }
    let object = selector.as_object().ok_or("invalid_memory_selector")?;
    if object
        .keys()
        .any(|key| !["memory_key", "old_content", "category"].contains(&key.as_str()))
    {
        return Err("invalid_memory_selector".into());
    }
    let key = field(selector, "memory_key")?.filter(|s| !s.is_empty());
    let content = field(selector, "old_content")?.filter(|s| !s.is_empty());
    let category = field(selector, "category")?;
    if key.is_none() && content.is_none() {
        return Err("invalid_memory_selector".into());
    }
    let folded_key = key
        .as_ref()
        .map(|s| crate::history::normalize::casefold(s.chars()));
    let state = if args["operation"] == "restore" {
        "retracted"
    } else {
        "active"
    };
    let mut after = 0;
    let mut best: Vec<Candidate> = vec![];
    loop {
        // Resolve the exact canonical owner and state in SQL before reading a
        // body. Page the authorized set so the lexical scan has bounded memory.
        let rows=sqlx::query("SELECT f.id,f.number,f.fact_key,coalesce(v.body,(SELECT original.body FROM memory_versions original WHERE original.fact=f.id AND f.status='retracted' AND original.body IS NOT NULL AND original.revision<=f.revision ORDER BY original.revision DESC LIMIT 1)) AS body,v.created_at,d.category,coalesce(d.kind,'fact') AS memory_kind FROM memory_facts f JOIN persons p ON p.id=f.person JOIN persons owner ON owner.id=coalesce(p.current_root,p.id) JOIN memory_versions v ON v.fact=f.id AND v.revision=f.revision LEFT JOIN extract_fact_details d ON d.fact=f.id WHERE f.kind='person' AND owner.id=? AND p.enabled AND owner.enabled AND f.status=? AND f.number>? AND (f.status='retracted' OR v.body IS NOT NULL) AND (d.valid_until IS NULL OR julianday(d.valid_until)>julianday('now')) ORDER BY f.number LIMIT 128")
            .bind(&r.root).bind(state).bind(after).fetch_all(&mut *db).await?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            let number: i64 = row.get("number");
            after = number;
            let actual_key: String = row.get("fact_key");
            let Some(body) = row.get::<Option<String>, _>("body") else {
                continue;
            };
            let actual_category: Option<String> = row.get("category");
            if category
                .as_ref()
                .is_some_and(|v| Some(v) != actual_category.as_ref())
            {
                continue;
            }
            let normalized =
                crate::history::normalize::casefold(super::metadata::normalize(&body).chars());
            let lexical = key
                .as_ref()
                .is_some_and(|v| literal_contains(&actual_key, v))
                || folded_key
                    .as_ref()
                    .is_some_and(|v| literal_contains(&normalized, v))
                || content
                    .as_ref()
                    .is_some_and(|v| literal_contains(&normalized, v));
            if !lexical {
                continue;
            }
            let key_matches = key.as_ref().is_none_or(|v| *v == actual_key);
            let body_matches = content.as_ref().is_none_or(|v| *v == normalized);
            let exact = key_matches && body_matches;
            best.push(Candidate {
                id:row.get("id"),number,exact,
                key_exact:if key.is_some() { key_matches } else { exact },
                body_exact:if content.is_some() { body_matches } else { exact },
                updated:row.get("created_at"),
                value:json!({"fact_id":number,"memory_ref":format!("M{number}"),"key":actual_key,"category":actual_category,"kind":row.get::<String,_>("memory_kind"),"content":body,"status":if state=="retracted" {"invalidated"} else {"active"}}),
            });
            best.sort_by(|a, b| {
                b.exact
                    .cmp(&a.exact)
                    .then(b.key_exact.cmp(&a.key_exact))
                    .then(b.body_exact.cmp(&a.body_exact))
                    .then(b.updated.cmp(&a.updated))
                    .then(a.number.cmp(&b.number))
            });
            best.truncate(4);
        }
    }
    if best.iter().filter(|c| c.exact).count() == 1 {
        return Ok(best.into_iter().find(|c| c.exact).unwrap().id);
    }
    let reason = if best.is_empty() {
        "memory_candidate_not_found"
    } else {
        "memory_candidate_ambiguous"
    };
    best.truncate(3);
    Err(Rejected {
        reason,
        facts: best.iter().map(|c| c.id.clone()).collect(),
        candidates: best.into_iter().map(|c| c.value).collect(),
    }
    .into())
}
