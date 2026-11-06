use super::{
    embedding,
    rank::{self, Hit},
    scope::{self, Reader},
};
use crate::{Result, store::Store};
use chrono::DateTime;
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::collections::BTreeMap;

#[derive(clap::ValueEnum, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Vec1,
    Exact,
}
#[derive(clap::Args, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub execution: String,
    pub actor: String,
    pub operation: String,
    pub text: String,
    #[arg(long)]
    pub start: Option<String>,
    #[arg(long)]
    pub end: Option<String>,
    #[arg(long, default_value_t = 10)]
    pub limit: usize,
    #[arg(long, default_value_t = 50)]
    pub candidates: usize,
    #[arg(long, value_enum, default_value = "exact")]
    pub backend: Backend,
    #[arg(long)]
    pub explain: bool,
}
fn date(raw: &Option<String>) -> Result<Option<i64>> {
    raw.as_deref()
        .map(|v| Ok(DateTime::parse_from_rfc3339(v)?.timestamp_millis()))
        .transpose()
}
const ELIGIBLE:&str="SELECT d.*,s.conflict_state FROM retrieval_documents d JOIN memory_fact_states s ON s.id=d.id JOIN json_each(?1) a ON d.id=a.value WHERE
 (d.valid_until IS NULL OR julianday(d.valid_until)>julianday('now')) AND
 (?2 IS NULL OR unixepoch(d.valid_from,'subsec')*1000>=?2) AND (?3 IS NULL OR unixepoch(d.valid_from,'subsec')*1000<?3)";
struct Index {
    profile: String,
    dimension: usize,
    max_chars: usize,
}
struct Flags {
    eligible: i64,
    covered: i64,
    stale: i64,
    provider_failed: bool,
    truncated: bool,
    lexical_missing: i64,
    approximate: bool,
}
async fn authorized(
    conn: &mut SqliteConnection,
    q: &Query,
    target: Option<&super::search_tool::Target>,
) -> Result<String> {
    let reader = Reader::at(conn, &q.execution, &q.actor, false, false).await?;
    let ids = scope::visible(conn, &reader, None, "", i64::MAX).await?;
    let ids = serde_json::to_string(&ids)?;
    match target {
        Some(t) => t.select(conn, &reader, &q.execution, &ids).await,
        None => Ok(ids),
    }
}
pub async fn retrieve(store: &Store, q: &Query) -> Result<Value> {
    scoped(store, q, None).await
}
pub(super) async fn scoped(
    store: &Store,
    q: &Query,
    target: Option<&super::search_tool::Target>,
) -> Result<Value> {
    let text = rank::normalize(&q.text);
    if text.is_empty()
        || text.chars().count() > 8192
        || q.operation.trim().is_empty()
        || !(1..=200).contains(&q.candidates)
        || q.limit == 0
        || q.limit > q.candidates
    {
        return Err("invalid retrieval bounds".into());
    }
    let (start, end) = (date(&q.start)?, date(&q.end)?);
    if matches!((start,end),(Some(a),Some(b)) if a>=b) {
        return Err("invalid half-open dates".into());
    }
    let index = store.vectors.as_ref().map(|e| Index {
        profile: e.profile.identity(),
        dimension: e.profile.dimensions,
        max_chars: e.profile.max_characters,
    });
    let mut vector = None;
    let mut failed = false;
    if index.is_some() {
        // Release this snapshot before leaving the process. An empty eligible pool needs no query purchase.
        let mut tx = store.readers().begin().await?;
        let ids = authorized(&mut tx, q, target).await?;
        let any: bool = sqlx::query_scalar(&format!("SELECT EXISTS({ELIGIBLE})"))
            .bind(ids)
            .bind(start)
            .bind(end)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        if any {
            if q.backend == Backend::Vec1 && !embedding::open_reference(store).await? {
                return Ok(json!({"status":"index_building","backend":"vec1","retryable":true}));
            }
            let receipt =
                embedding::query(store, &q.execution, &q.actor, &q.operation, &text).await;
            vector = receipt
                .ok()
                .and_then(|v| serde_json::from_value::<Vec<f32>>(v["vector"].clone()).ok());
            failed = vector.is_none()
        }
    }
    // All actual selection, scoring and returned bodies now share a fresh authorization snapshot.
    let mut tx = store.readers().begin().await?;
    let ids = authorized(&mut tx, q, target).await?;
    let eligible = format!("WITH eligible AS MATERIALIZED ({ELIGIBLE})");
    let (count,lexical_missing):(i64,i64)=sqlx::query_as(&format!("{eligible} SELECT count(*),count(*)-count(f.rowid) FROM eligible d LEFT JOIN memory_retrieval_fts f ON f.rowid=d.number")).bind(&ids).bind(start).bind(end).fetch_one(&mut *tx).await?;
    let mut flags = Flags {
        eligible: count,
        covered: 0,
        stale: 0,
        provider_failed: failed,
        truncated: false,
        lexical_missing,
        approximate: false,
    };
    let mut hits = BTreeMap::new();
    let (words, short) = rank::terms(&text, 12);
    let b = Binding {
        eligible: &eligible,
        ids: &ids,
        start,
        end,
    };
    if !words.is_empty() {
        let expression = words
            .iter()
            .map(|v| format!("\"{v}\""))
            .collect::<Vec<_>>()
            .join(" OR ");
        let sql = format!(
            "{eligible} SELECT d.*,bm25(memory_retrieval_fts,1.,4.,2.) AS fts FROM memory_retrieval_fts JOIN eligible d ON d.number=memory_retrieval_fts.rowid WHERE memory_retrieval_fts MATCH ?4 ORDER BY fts,d.number LIMIT ?5"
        );
        let rows = sqlx::query(&sql)
            .bind(&ids)
            .bind(start)
            .bind(end)
            .bind(expression)
            .bind(i64::try_from(q.candidates + 1)?)
            .fetch_all(&mut *tx)
            .await?;
        flags.truncated |= rows.len() > q.candidates;
        for r in rows.iter().take(q.candidates) {
            let score = -r.get::<f64, _>("fts");
            let mut h = hit(r)?;
            h.lexical = Some(score);
            hits.insert(h.number, h);
        }
    }
    if !short.is_empty() {
        // SQLite's built-in lower() only folds ASCII. The v4 short-query branch
        // searches normalized fact content and treats all query terms literally.
        let sql = format!("{eligible} SELECT d.* FROM eligible d ORDER BY d.number");
        let mut rows = sqlx::query(&sql)
            .bind(&ids)
            .bind(start)
            .bind(end)
            .fetch(&mut *tx);
        let mut found = 0;
        while let Some(r) = rows.try_next().await? {
            let h = hit(&r)?;
            let fields = [
                rank::normalize(&h.text),
                rank::normalize(&h.key),
                rank::normalize(&h.category),
            ];
            if !short
                .iter()
                .any(|term| fields.iter().any(|field| field.contains(term)))
            {
                continue;
            }
            found += 1;
            if found > q.candidates {
                flags.truncated = true;
                break;
            }
            hits.entry(h.number).or_insert(Hit {
                lexical: Some(-1000.),
                ..h
            });
        }
        if hits.len() > q.candidates {
            flags.truncated = true;
            let mut scores: Vec<_> = hits
                .values()
                .map(|h| (h.lexical.unwrap_or_default(), h.number))
                .collect();
            scores.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            for (_, id) in scores.into_iter().skip(q.candidates) {
                hits.remove(&id);
            }
        }
    }
    if count > 0
        && let Some(index) = &index
    {
        let route = Semantic {
            index,
            query: vector.as_deref(),
            limit: q.candidates,
        };
        if q.backend == Backend::Vec1 {
            native(&mut tx, &b, &route, &mut hits, &mut flags).await?;
        } else {
            semantic(
                &mut tx,
                &b,
                index,
                vector.as_deref(),
                q.candidates,
                &mut hits,
                &mut flags,
            )
            .await?;
        }
    }
    flags.truncated |= hits.len() > q.limit;
    let items = rank::global(hits, &text, q.limit);
    let mut result = json!({"items":items,"mode":if index.is_some(){"hybrid"}else{"lexical-only"},"authorized":flags.eligible,"covered":if index.is_some(){Some(flags.covered)}else{None},"stale":flags.stale>0,"stale_count":flags.stale,"lexical_missing":flags.lexical_missing,"backend":if index.is_none(){"lexical"}else if q.backend==Backend::Vec1{"vec1"}else{"exact"},"approximate":flags.approximate,"truncated":flags.truncated,"partial_failure":flags.lexical_missing>0||index.is_some()&&(flags.provider_failed||flags.covered<flags.eligible),"provider_failure":flags.provider_failed});
    if q.explain
        && count > 0
        && let Some(index) = &index
    {
        let sql = if q.backend == Backend::Exact {
            scan_sql(&eligible)
        } else {
            metadata_sql(&eligible, &super::index::table(&index.profile)?)
        };
        let rows = sqlx::query(&format!("EXPLAIN QUERY PLAN {sql}"))
            .bind(&ids)
            .bind(start)
            .bind(end)
            .bind(&index.profile)
            .fetch_all(&mut *tx)
            .await?;
        let mut plans: Vec<String> = rows.iter().map(|r| r.get("detail")).collect();
        if q.backend == Backend::Vec1 {
            let name = super::index::table(&index.profile)?;
            let explain = format!("EXPLAIN QUERY PLAN {}", super::index::nearest_sql(&name));
            let rows = sqlx::query(&explain)
                .bind(Vec::<u8>::new())
                .bind("{}")
                .bind("[]")
                .fetch_all(&mut *tx)
                .await?;
            plans.extend(rows.iter().map(|r| r.get::<String, _>("detail")));
        }
        result["semantic_eqp"] = json!(plans);
    }
    Ok(result)
}
fn hit(r: &sqlx::sqlite::SqliteRow) -> Result<Hit> {
    Ok(Hit {
        number: r.try_get("number")?,
        revision: r.try_get("revision")?,
        id: r.try_get("id")?,
        text: r.try_get("content")?,
        conflict: r.try_get("conflict_state")?,
        importance: r.try_get("importance")?,
        key: r.try_get("fact_key")?,
        category: r.try_get("category")?,
        lexical: None,
        semantic: None,
    })
}
struct Binding<'a> {
    eligible: &'a str,
    ids: &'a str,
    start: Option<i64>,
    end: Option<i64>,
}
async fn semantic(
    conn: &mut SqliteConnection,
    b: &Binding<'_>,
    index: &Index,
    q: Option<&[f32]>,
    limit: usize,
    hits: &mut BTreeMap<i64, Hit>,
    flags: &mut Flags,
) -> Result<()> {
    use embedding::profile::{digest, document};
    let eligible = b.eligible;
    let query = scan_sql(eligible);
    let mut top = Vec::new();
    let mut rows = sqlx::query(&query)
        .bind(b.ids)
        .bind(b.start)
        .bind(b.end)
        .bind(&index.profile)
        .fetch(&mut *conn);
    while let Some(r) = rows.try_next().await? {
        let Some(vector) = r.get::<Option<&[u8]>, _>("vector") else {
            continue;
        };
        let projection = document(
            r.get("kind"),
            r.get("category"),
            r.get("fact_key"),
            r.get("content"),
            index.max_chars,
        );
        if r.get::<i64, _>("indexed_revision") != r.get::<i64, _>("revision")
            || r.get::<i64, _>("dimension") != i64::try_from(index.dimension)?
            || r.get::<&str, _>("content_hash") != digest(projection.as_bytes())
        {
            flags.stale += 1;
            continue;
        }
        if let Some(q) = q {
            match rank::cosine(vector, q) {
                Ok(score) => {
                    let mut h = hit(&r)?;
                    h.semantic = Some(score);
                    top.push((score, h));
                }
                Err(_) => {
                    flags.stale += 1;
                    continue;
                }
            }
        }
        flags.covered += 1;
        if top.len() >= 2 * limit {
            keep_top(&mut top, limit);
            flags.truncated = true;
        }
    }
    flags.truncated |= top.len() > limit;
    keep_top(&mut top, limit);
    for (score, h) in top {
        hits.entry(h.number)
            .and_modify(|h| h.semantic = Some(score))
            .or_insert(h);
    }
    Ok(())
}

fn keep_top(top: &mut Vec<(f64, Hit)>, limit: usize) {
    top.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.number.cmp(&b.1.number)));
    top.truncate(limit);
}

struct Semantic<'a> {
    index: &'a Index,
    query: Option<&'a [f32]>,
    limit: usize,
}
async fn native(
    conn: &mut SqliteConnection,
    b: &Binding<'_>,
    route: &Semantic<'_>,
    hits: &mut BTreeMap<i64, Hit>,
    flags: &mut Flags,
) -> Result<()> {
    use embedding::profile::{digest, document};
    let idx = route.index;
    let table = super::index::table(&idx.profile)?;
    // Version and content checks happen before Vec1's candidate limit. Never select
    // global nearest neighbors and then remove forbidden or stale items.
    let sql = metadata_sql(b.eligible, &table);
    let mut verified = Vec::new();
    {
        let mut rows = sqlx::query(&sql)
            .bind(b.ids)
            .bind(b.start)
            .bind(b.end)
            .bind(&idx.profile)
            .fetch(&mut *conn);
        while let Some(r) = rows.try_next().await? {
            let Some(hash) = r.try_get::<Option<&str>, _>("current_hash")? else {
                continue;
            };
            let current = digest(
                document(
                    r.get("kind"),
                    r.get("category"),
                    r.get("fact_key"),
                    r.get("content"),
                    idx.max_chars,
                )
                .as_bytes(),
            );
            if hash != current
                || r.get::<i64, _>("revision") != r.get::<i64, _>("vector_revision")
                || r.get::<i64, _>("dim") != idx.dimension as i64
                || r.get::<i64, _>("bytes") != 4 * idx.dimension as i64
            {
                flags.stale += 1;
                continue;
            }
            if !r.get::<bool, _>("present") || r.get::<Option<i64>, _>("indexed").is_none() {
                continue;
            }
            if r.get::<i64, _>("indexed") != r.get::<i64, _>("revision")
                || r.get::<Option<&str>, _>("indexed_hash") != Some(hash)
            {
                flags.stale += 1;
                continue;
            }
            verified.push(r.get("number"));
        }
    }
    flags.covered = i64::try_from(verified.len())?;
    let Some(query) = route.query else {
        return Ok(());
    };
    flags.approximate = verified.len() > route.limit;
    flags.truncated |= flags.approximate;
    let ids = super::index::nearest(conn, &idx.profile, &verified, query, route.limit).await?;
    let selected=sqlx::query("SELECT d.*,s.conflict_state,v.vector FROM json_each(?1) selected JOIN retrieval_documents d ON d.number=selected.value JOIN memory_fact_states s ON s.id=d.id JOIN retrieval_index_members m ON m.fact=d.id AND m.profile=?2 JOIN fact_vectors v ON v.fact=d.id AND v.profile=m.profile AND v.content_hash=m.content_hash AND v.revision=d.revision")
        .bind(serde_json::to_string(&ids)?).bind(&idx.profile).fetch_all(conn).await?;
    for row in selected {
        let mut h = hit(&row)?;
        let Ok(score) = rank::cosine(row.get("vector"), query) else {
            flags.stale += 1;
            flags.covered -= 1;
            continue;
        };
        h.semantic = Some(score);
        hits.entry(h.number)
            .and_modify(|old| old.semantic = Some(score))
            .or_insert(h);
    }
    Ok(())
}

fn scan_sql(eligible: &str) -> String {
    format!(
        "{eligible} SELECT d.*,v.vector,v.dimension,v.content_hash,v.revision AS indexed_revision FROM eligible d LEFT JOIN fact_vectors v ON v.fact=d.id AND v.purpose='document' AND v.profile=?4 ORDER BY d.number"
    )
}

fn metadata_sql(eligible: &str, table: &str) -> String {
    format!(
        "{} SELECT d.*,m.revision AS indexed,m.content_hash AS indexed_hash,v.content_hash AS current_hash,v.dimension AS dim,length(v.vector) AS bytes,v.revision AS vector_revision,EXISTS(SELECT 1 FROM {table} n WHERE n.rowid=d.number) AS present FROM eligible d LEFT JOIN retrieval_index_members m ON m.fact=d.id AND m.profile=?4 LEFT JOIN fact_vectors v ON v.rowid=(SELECT max(rowid) FROM fact_vectors v2 WHERE v2.fact=d.id AND v2.profile=?4 AND v2.purpose='document') ORDER BY d.number",
        eligible
    )
}
