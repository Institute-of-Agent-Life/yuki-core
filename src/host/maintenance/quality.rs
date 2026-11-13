use super::scope::Granted;
use crate::{
    Result,
    memory::{self, embedding::profile},
};
use serde::Serialize;
use sqlx::{Row, SqliteConnection};

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Lexical {
    Current,
    Missing,
    Mismatch,
    NotIndexed,
    Unexpected,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Vector {
    Current,
    Missing,
    InvalidShape,
    Stale,
    NotIndexed,
}

#[derive(Serialize)]
pub(super) struct Quality {
    lexical: Lexical,
    evidence_missing: bool,
    event_evidence_checked: usize,
    invalid_event_evidence: i64,
    vectors: Vec<VectorCoverage>,
}

#[derive(Serialize)]
struct VectorCoverage {
    route: String,
    profile: String,
    coverage: Vector,
    attempt: Option<String>,
}

pub(super) async fn inspect(
    db: &mut SqliteConnection,
    scope: &Granted,
    fact: &str,
    revision: i64,
    evidence_total: i64,
) -> Result<(Quality, String)> {
    let row = sqlx::query(
        "SELECT d.content,d.fact_key,d.category,d.kind AS embedding_kind,
                fts.rowid AS indexed,fts.content AS indexed_content,
                fts.memory_key AS indexed_key,fts.category AS indexed_category
         FROM memory_facts f LEFT JOIN retrieval_documents d ON d.id=f.id
         LEFT JOIN memory_retrieval_fts fts ON fts.rowid=f.number WHERE f.id=?",
    )
    .bind(fact)
    .fetch_one(&mut *db)
    .await?;
    let body: Option<&str> = row.try_get("content")?;
    let key: Option<&str> = row.try_get("fact_key")?;
    let category: Option<&str> = row.try_get("category")?;
    let indexed_content: Option<&str> = row.try_get("indexed_content")?;
    let indexed_key: Option<&str> = row.try_get("indexed_key")?;
    let indexed_category: Option<&str> = row.try_get("indexed_category")?;
    let indexed = row.try_get::<Option<i64>, _>("indexed")?.is_some();
    let lexical = match (body, indexed) {
        (None, false) => Lexical::NotIndexed,
        (None, true) => Lexical::Unexpected,
        (Some(_), false) => Lexical::Missing,
        (Some(_), true)
            if (body, key, category) == (indexed_content, indexed_key, indexed_category) =>
        {
            Lexical::Current
        }
        (Some(_), true) => Lexical::Mismatch,
    };
    let lexical_version = memory::scope::hash(&serde_json::json!([
        indexed,
        indexed_content,
        indexed_key,
        indexed_category
    ]));
    let mut vectors = Vec::new();
    for (route, configured) in scope
        .embedding_routes
        .iter()
        .filter(|(route, _)| scope.policy.embedding_routes.contains(*route))
    {
        let identity = configured.identity();
        let coverage = if let Some(body) = body {
            let expected = profile::digest(
                profile::document(
                    row.try_get("embedding_kind")?,
                    category.ok_or("retrieval category missing")?,
                    key.ok_or("retrieval key missing")?,
                    body,
                    configured.max_characters,
                )
                .as_bytes(),
            );
            let stored: Option<(i64, i64, i64, String)> = sqlx::query_as(
                "SELECT revision,dimension,length(vector),content_hash FROM fact_vectors
                 WHERE fact=?1 AND profile=?2 AND purpose='document'
                 ORDER BY (revision=?3 AND content_hash=?4 AND dimension=?5
                           AND length(vector)=4*?5) DESC,rowid DESC LIMIT 1",
            )
            .bind(fact)
            .bind(&identity)
            .bind(revision)
            .bind(&expected)
            .bind(i64::try_from(configured.dimensions)?)
            .fetch_optional(&mut *db)
            .await?;
            let dim = i64::try_from(configured.dimensions)?;
            match stored {
                None => Vector::Missing,
                Some((_, actual, bytes, _)) if actual != dim || bytes != 4 * dim => {
                    Vector::InvalidShape
                }
                Some((rev, _, _, hash)) if rev != revision || hash != expected => Vector::Stale,
                Some(_) => Vector::Current,
            }
        } else {
            Vector::NotIndexed
        };
        let attempt = sqlx::query_scalar(
            "SELECT i.state FROM embedding_items i JOIN embedding_requests r ON r.logical=i.logical
             WHERE i.fact=? AND i.revision=? AND r.profile=? ORDER BY r.rowid DESC LIMIT 1",
        )
        .bind(fact)
        .bind(revision)
        .bind(&identity)
        .fetch_optional(&mut *db)
        .await?;
        vectors.push(VectorCoverage {
            route: route.clone(),
            profile: identity,
            coverage,
            attempt,
        });
    }
    let originals: Vec<(i64, String, String, String, String)> = sqlx::query_as(
        "SELECT event,entry,author,source_version,quote FROM memory_evidence
         WHERE fact=? AND kind='event' AND quote IS NOT NULL
           AND entry IN(SELECT value FROM json_each(?)) ORDER BY id",
    )
    .bind(fact)
    .bind(serde_json::to_string(&scope.reader.base.entries)?)
    .fetch_all(&mut *db)
    .await?;
    let event_evidence_checked = originals.len();
    let mut invalid_event_evidence = 0;
    for (event, entry, author, version, quote) in originals {
        match memory::evidence::event_proof(db, event, &quote).await {
            Ok(proof)
                if proof.entry == entry && proof.author == author && proof.version == version => {}
            Err(e)
                if e.downcast_ref::<sqlx::Error>()
                    .is_some_and(|e| !matches!(e, sqlx::Error::RowNotFound))
                    || e.downcast_ref::<serde_json::Error>().is_some() =>
            {
                return Err(e);
            }
            _ => invalid_event_evidence += 1,
        }
    }
    Ok((
        Quality {
            lexical,
            evidence_missing: evidence_total == 0,
            event_evidence_checked,
            invalid_event_evidence,
            vectors,
        },
        lexical_version,
    ))
}
