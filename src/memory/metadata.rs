use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Patch {
    pub memory_key: Option<String>,
    pub category: Option<String>,
    pub kind: Option<String>,
    pub importance: Option<i64>,
}

// Original normalize_memory_text: control runs and Unicode whitespace become a single space.
fn text(value: &str) -> Result<String> {
    let value = value
        .split(|c: char| c.is_whitespace() || c <= '\u{1f}' || c == '\u{7f}')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if value.is_empty() {
        return Err("empty memory metadata text".into());
    }
    Ok(value)
}
pub(super) async fn read(db: &mut SqliteConnection, fact: &str) -> Result<Value> {
    let row = sqlx::query("SELECT d.* FROM extract_fact_details d WHERE fact=?")
        .bind(fact)
        .fetch_optional(db)
        .await?;
    Ok(match row {
        None => {
            json!({"category":null,"kind":"fact","importance":3,"temporal_mode":null,"valid_from":null,"valid_until":null})
        }
        Some(r) => {
            json!({"category":r.get::<Option<String>,_>("category"),"kind":r.get::<Option<String>,_>("kind").unwrap_or_else(||"fact".into()),"importance":r.get::<Option<i64>,_>("importance").unwrap_or(3),"temporal_mode":r.get::<Option<String>,_>("temporal_mode"),"valid_from":r.get::<Option<String>,_>("valid_from"),"valid_until":r.get::<Option<String>,_>("valid_until")})
        }
    })
}
pub(super) async fn apply(db: &mut SqliteConnection, fact: &str, patch: &Patch) -> Result<()> {
    let key = patch.memory_key.as_deref().map(text).transpose()?;
    let category = patch.category.as_deref().map(text).transpose()?;
    if patch
        .kind
        .as_deref()
        .is_some_and(|v| !matches!(v, "fact" | "preference" | "episode"))
    {
        return Err("invalid memory kind".into());
    }
    if patch.importance.is_some_and(|v| !(1..=5).contains(&v)) {
        return Err("invalid memory importance".into());
    }
    sqlx::query("INSERT INTO extract_fact_details(fact,category,kind,importance) VALUES(?,?,?,?) ON CONFLICT(fact) DO UPDATE SET category=coalesce(excluded.category,category),kind=coalesce(excluded.kind,kind),importance=coalesce(excluded.importance,importance)")
        .bind(fact).bind(category).bind(&patch.kind).bind(patch.importance).execute(&mut *db).await?;
    if let Some(key) = key {
        sqlx::query("UPDATE memory_facts SET fact_key=? WHERE id=?")
            .bind(key)
            .bind(fact)
            .execute(db)
            .await?;
    }
    Ok(())
}
