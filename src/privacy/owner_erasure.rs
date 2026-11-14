use super::source;
use crate::Result;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::collections::BTreeMap;

pub async fn erase(
    conn: &mut SqliteConnection,
    key: &str,
    person: &str,
    raw: &str,
) -> Result<Value> {
    let mut sources: Vec<i64> = serde_json::from_str(raw)?;
    sources.sort_unstable();
    sources.dedup();
    if sources.is_empty() {
        return Err("erase requires actual sources".into());
    }
    if sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM person_forgets f JOIN source_erasure_requests r USING(watermark) WHERE r.erase_key=?)").bind(key).fetch_one(&mut *conn).await?{
        return Err("erasure key belongs to a Person closure".into());
    }
    erase_sources(conn, key, person, sources, None).await
}

pub(super) async fn erase_sources(
    conn: &mut SqliteConnection,
    key: &str,
    person: &str,
    sources: Vec<i64>,
    family: Option<&str>,
) -> Result<Value> {
    // Authorize the caller's original roots before following stored derived references.
    if family.is_none() {
        for event in &sources {
            let actual: Option<String> =
                sqlx::query_scalar("SELECT raw_actor FROM events WHERE id=?")
                    .bind(event)
                    .fetch_one(&mut *conn)
                    .await?;
            if actual.as_deref() != Some(person) {
                return Err("source is not owned by this original person".into());
            }
        }
    }
    let sources: Vec<i64> = sqlx::query_scalar(
        "WITH RECURSIVE erased(id) AS(SELECT value FROM json_each(?) UNION
         SELECT p.event FROM erased e JOIN notification_sources d ON d.source=e.id
         JOIN notification_publications p ON p.id=d.publication)
         SELECT id FROM erased ORDER BY id",
    )
    .bind(serde_json::to_string(&sources)?)
    .fetch_all(&mut *conn)
    .await?;
    let canonical = serde_json::to_string(&sources)?;
    if let Some(old) = sqlx::query(
        "SELECT subject,sources,watermark FROM source_erasure_requests WHERE erase_key=?",
    )
    .bind(key)
    .fetch_optional(&mut *conn)
    .await?
    {
        if old.try_get::<String, _>("subject")? != person
            || old.try_get::<String, _>("sources")? != canonical
        {
            return Err("erasure key conflicts with original subject or sources".into());
        }
        return Ok(
            json!({"key":key,"watermark":old.try_get::<i64,_>("watermark")?,"sources":sources}),
        );
    }
    let watermark:i64=sqlx::query_scalar("INSERT INTO source_erasure_requests(erase_key,subject,sources) VALUES(?,?,?) RETURNING watermark")
        .bind(key).bind(person).bind(&canonical).fetch_one(&mut *conn).await?;
    if let Some(members) = family {
        super::forget::capture(conn, watermark, person, members, &canonical).await?;
    }
    let mut obligations: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    for event in &sources {
        if source::is_erased(conn, *event).await? {
            continue;
        }
        sqlx::query("INSERT INTO invalidated_sources(source,subject,watermark) VALUES(?,?,?)")
            .bind(event)
            .bind(person)
            .bind(watermark)
            .execute(&mut *conn)
            .await?;
        let owners: Vec<String> = sqlx::query_scalar(
            "SELECT owner FROM source_holds WHERE source=?1
             UNION SELECT owner FROM maintenance_quality_dependencies WHERE source=?1 ORDER BY owner",
        )
        .bind(event)
        .fetch_all(&mut *conn)
        .await?;
        for owner in owners {
            obligations.entry(owner).or_default().push(*event);
        }
        source::scrub(conn, *event).await?;
    }
    // A participation reader can have a copy of an event authored by somebody else.
    if let Some(members) = family {
        let copies: Vec<(String, i64)> = sqlx::query_as(
            "SELECT DISTINCT h.owner,h.source FROM participation_held_sources h JOIN event_person_mentions m ON m.event=h.source
             WHERE m.person IN(SELECT value FROM json_each(?))",
        )
        .bind(members)
        .fetch_all(&mut *conn)
        .await?;
        for (owner, event) in copies {
            obligations.entry(owner).or_default().push(event);
        }
    }
    for (owner, mut refs) in obligations {
        refs.sort_unstable();
        refs.dedup();
        sqlx::query("INSERT INTO owner_erasures(watermark,owner,sources) VALUES(?,?,?)")
            .bind(watermark)
            .bind(owner)
            .bind(serde_json::to_string(&refs)?)
            .execute(&mut *conn)
            .await?;
    }
    Ok(json!({"key":key,"watermark":watermark,"sources":sources}))
}

pub async fn pending(conn: &mut SqliteConnection, owner: &str) -> Result<Value> {
    let rows=sqlx::query("SELECT r.erase_key,d.watermark,d.sources FROM owner_erasures d JOIN source_erasure_requests r
        ON r.watermark=d.watermark WHERE d.owner=? AND d.proof IS NULL ORDER BY d.watermark").bind(owner).fetch_all(&mut *conn).await?;
    let mut items = Vec::new();
    for row in rows {
        let sources: &str = row.try_get("sources")?;
        let mut item = json!({"key":row.try_get::<String,_>("erase_key")?,"watermark":row.try_get::<i64,_>("watermark")?,
        "sources":serde_json::from_str::<Vec<i64>>(sources)?});
        let refs: Vec<String> = sqlx::query_scalar(
            "SELECT reference FROM maintenance_quality_dependencies
              WHERE owner=? AND source IN(SELECT value FROM json_each(?)) ORDER BY reference",
        )
        .bind(owner)
        .bind(sources)
        .fetch_all(&mut *conn)
        .await?;
        if !refs.is_empty() {
            item["erasure_refs"] = json!(refs);
            let visible: Vec<i64> = sqlx::query_scalar(
                "SELECT DISTINCT source FROM source_holds WHERE owner=? AND source IN(SELECT value FROM json_each(?)) ORDER BY source",
            )
            .bind(owner)
            .bind(sources)
            .fetch_all(&mut *conn)
            .await?;
            item["callback_sources"] = json!(visible);
        }
        items.push(item);
    }
    Ok(json!({"owner":owner,"obligations":items}))
}

pub async fn ack(
    conn: &mut SqliteConnection,
    owner: &str,
    key: &str,
    watermark: i64,
    proof: &str,
) -> Result<Value> {
    if proof.trim().is_empty() {
        return Err("owner cleanup receipt required".into());
    }
    let row=sqlx::query("SELECT d.proof,o.enabled FROM owner_erasures d JOIN source_owners o ON o.id=d.owner
        JOIN source_erasure_requests r ON r.watermark=d.watermark WHERE d.owner=? AND r.erase_key=? AND d.watermark=?")
        .bind(owner).bind(key).bind(watermark).fetch_one(&mut *conn).await?;
    if !row.try_get::<bool, _>("enabled")? {
        return Err("disabled owner cannot settle an outstanding obligation".into());
    }
    if let Some(old) = row.try_get::<Option<String>, _>("proof")? {
        if old != proof {
            return Err("acknowledgment conflicts with retained proof".into());
        }
    } else {
        sqlx::query("UPDATE owner_erasures SET proof=? WHERE owner=? AND watermark=?")
            .bind(proof)
            .bind(owner)
            .bind(watermark)
            .execute(conn)
            .await?;
    }
    Ok(json!({"owner":owner,"key":key,"watermark":watermark,"acknowledged":true}))
}
