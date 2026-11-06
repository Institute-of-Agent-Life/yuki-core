//! Original paid Memory merge. The caller owns the memory_tool savepoint.
use super::{
    facts,
    scope::{self, Reader},
    tools,
};
use crate::Result;
use serde_json::{Value, json};
use sqlx::SqliteConnection;

async fn resolve(
    db: &mut SqliteConnection,
    r: &Reader,
    req: &Value,
    id: &str,
    selector: &str,
) -> Result<String> {
    match (req[id].as_i64(), req.get(selector).filter(|v| !v.is_null())) {
        (Some(n), None) => sqlx::query_scalar("SELECT id FROM memory_facts WHERE number=?")
            .bind(n)
            .fetch_optional(db)
            .await?
            .ok_or_else(|| "fact unavailable".into()),
        (None, Some(v)) => {
            super::selectors::locate(
                db,
                r,
                &json!({"selector":v,"target":req["target"],"operation":"merge"}),
            )
            .await
        }
        _ => Err("exactly one original fact ID or selector is required per side".into()),
    }
}

pub(super) async fn apply(
    db: &mut SqliteConnection,
    r: &Reader,
    exec: &str,
    op: &str,
    req: &Value,
) -> Result<Value> {
    if req["expected_fact_state"]
        .as_str()
        .is_some_and(|s| s != "active")
    {
        return Err("expected_fact_state_mismatch".into());
    }
    let src = resolve(db, r, req, "fact_id", "selector").await?;
    let dst = resolve(db, r, req, "merge_fact_id", "merge_selector").await?;
    if src == dst {
        return Err("cannot merge a memory fact into itself".into());
    }
    let origin = tools::paid_active_person(db, r, exec, &src).await?;
    let target = tools::paid_active_person(db, r, exec, &dst).await?;
    let person = |v: &Value| facts::Subject {
        kind: "person".into(),
        person: v["person"].as_str().map(str::to_owned),
        space: None,
        visibility: None,
        entry: None,
    };
    if !scope::same_subject(db, &person(&origin), &person(&target)).await?
        || !scope::same_subject(
            db,
            &person(&target),
            &facts::Subject {
                kind: "person".into(),
                person: Some(r.base.subject.clone()),
                space: None,
                entry: None,
                visibility: None,
            },
        )
        .await?
    {
        return Err("merge_target_mismatch".into());
    }
    let evidence = origin["evidence"]
        .as_array()
        .ok_or("original evidence unavailable")?;
    if evidence.is_empty() {
        return Err("original evidence unavailable".into());
    }
    for e in evidence {
        sqlx::query("INSERT OR IGNORE INTO memory_evidence(fact,kind,source_key,source_version,entry,author,origin_execution,event,quote)
            SELECT ?,kind,source_key,source_version,entry,author,origin_execution,event,quote FROM memory_evidence WHERE fact=? AND id=? AND quote IS NOT NULL")
            .bind(&dst).bind(&src).bind(e["id"].as_i64()).execute(&mut *db).await?;
    }
    let source = tools::original_event(db, exec, req).await?;
    let proof = super::evidence::resolve(db, r, &r.actor, &source).await?;
    super::evidence::attach(db, &dst, proof).await?;
    let src_rev = origin["revision"]
        .as_i64()
        .ok_or("source revision missing")?
        .checked_add(1)
        .ok_or("revision overflow")?;
    let dst_rev = target["revision"]
        .as_i64()
        .ok_or("destination revision missing")?
        .checked_add(1)
        .ok_or("revision overflow")?;
    for (id, rev, status, body, confidence) in [
        (
            &src,
            src_rev,
            "retracted",
            None,
            origin["confidence"].as_f64(),
        ),
        (
            &dst,
            dst_rev,
            "active",
            Some(target["body"].as_str().ok_or("target body unavailable")?),
            target["confidence"].as_f64(),
        ),
    ] {
        sqlx::query("UPDATE memory_facts SET status=?,revision=? WHERE id=? AND revision=? AND status='active' RETURNING id")
            .bind(status).bind(rev).bind(id).bind(rev-1).fetch_one(&mut *db).await?;
        sqlx::query("INSERT INTO memory_versions(fact,revision,body,confidence,execution) VALUES(?,?,?,?,?)")
            .bind(id).bind(rev).bind(body).bind(confidence).bind(exec).execute(&mut *db).await?;
    }
    sqlx::query("INSERT INTO memory_equivalences(source,target,source_revision,target_revision,execution,operation) VALUES(?,?,?,?,?,?)")
        .bind(&src).bind(&dst).bind(src_rev).bind(dst_rev).bind(exec).bind(op).execute(&mut *db).await?;
    sqlx::query("INSERT INTO memory_operations(execution,original_key,request_hash,fact,revision,outcome) VALUES(?,?,?,?,?,'merged')")
        .bind(exec).bind(op).bind(scope::hash(req)).bind(&dst).bind(dst_rev).execute(&mut *db).await?;
    tools::bind_sources(db, exec, &src).await?;
    tools::bind_sources(db, exec, &dst).await?;
    Ok(
        json!({"fact":dst,"fact_id":target["fact_id"],"source_fact_id":origin["fact_id"],"revision":dst_rev,"old_fact_id":origin["fact_id"],"new_fact_id":target["fact_id"],"reason_code":"merged","outcome":"committed","applied_operation":"merge","source_status":"superseded"}),
    )
}
