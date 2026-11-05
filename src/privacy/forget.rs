use super::owner_erasure;
use crate::Result;
use serde_json::{Value, json};
use sqlx::SqliteConnection;

pub async fn apply(db: &mut SqliteConnection, key: &str, person: &str) -> Result<Value> {
    if key.is_empty() || key.len() > 256 {
        return Err("invalid_request_id".into());
    }
    if let Some(original)=sqlx::query_scalar::<_,String>("SELECT f.requested FROM person_forgets f JOIN source_erasure_requests r USING(watermark) WHERE r.erase_key=?").bind(key).fetch_optional(&mut *db).await?{
        if original!=person{return Err("request_conflict".into());}
        return status(db,key).await;
    }
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM source_erasure_requests WHERE erase_key=?)",
    )
    .bind(key)
    .fetch_one(&mut *db)
    .await?
    {
        return Err("request_conflict".into());
    }
    let root:String=sqlx::query_scalar("SELECT coalesce(current_root,id) FROM persons WHERE id=? AND NOT EXISTS(SELECT 1 FROM forgotten_people WHERE person=?)").bind(person).bind(person).fetch_optional(&mut *db).await?.ok_or("not_found")?;
    let members: Vec<String> =
        sqlx::query_scalar("SELECT id FROM persons WHERE coalesce(current_root,id)=? ORDER BY id")
            .bind(root)
            .fetch_all(&mut *db)
            .await?;
    let family = serde_json::to_string(&members)?;
    let ids:Vec<i64>=sqlx::query_scalar("SELECT e.id FROM events e LEFT JOIN entries d ON d.id=e.entry_id LEFT JOIN assignments a ON a.id=d.actor_stage WHERE e.raw_actor IN(SELECT value FROM json_each(?1)) OR (d.scope='private' AND a.original_person IN(SELECT value FROM json_each(?1))) ORDER BY e.id").bind(&family).fetch_all(&mut *db).await?;
    let r = owner_erasure::erase_sources(db, key, person, ids, Some(&family)).await?;
    let mark = r["watermark"].as_i64().ok_or("missing erasure watermark")?;
    // Changes only name/family-owned rows and derived facts. A reused transport
    // account remains bound to its independent newer stage.
    for statement in [
        "UPDATE memory_facts SET status='erased' WHERE person IN(SELECT value FROM json_each(?1)) AND status!='erased'",
        "UPDATE memory_versions SET body=NULL WHERE fact IN(SELECT id FROM memory_facts WHERE person IN(SELECT value FROM json_each(?1)))",
        "UPDATE memory_evidence SET quote=NULL WHERE fact IN(SELECT id FROM memory_facts WHERE person IN(SELECT value FROM json_each(?1)))",
        "DELETE FROM fact_vectors WHERE fact IN(SELECT id FROM memory_facts WHERE person IN(SELECT value FROM json_each(?1)))",
        "UPDATE schedule_versions SET goal=NULL,name=NULL WHERE schedule IN(SELECT id FROM schedules WHERE owner IN(SELECT value FROM json_each(?1)))",
        "UPDATE schedules SET status='blocked',next_due=NULL WHERE owner IN(SELECT value FROM json_each(?1))",
        "UPDATE work_nodes SET goal=NULL,note=NULL,summary=NULL,status='cancelled' WHERE principal IN(SELECT value FROM json_each(?1))",
        "UPDATE personal_codes SET code_hash='forgotten-'||id,expires=0 WHERE owner IN(SELECT value FROM json_each(?1))",
        "DELETE FROM personal_sessions WHERE device IN(SELECT device_id FROM device_origins WHERE original_person IN(SELECT value FROM json_each(?1)))",
        "DELETE FROM personal_proofs WHERE device IN(SELECT device_id FROM device_origins WHERE original_person IN(SELECT value FROM json_each(?1)))",
        "UPDATE personal_devices SET token_hash='forgotten-'||id,label='',revoked=coalesce(revoked,unixepoch()) WHERE id IN(SELECT device_id FROM device_origins WHERE original_person IN(SELECT value FROM json_each(?1)))",
        "UPDATE personal_receipts SET result='{\"erased\":true}',target=NULL WHERE owner IN(SELECT value FROM json_each(?1))",
        "UPDATE entries SET closed_at=coalesce(closed_at,unixepoch('subsec')*1000) WHERE actor_stage IN(SELECT id FROM assignments WHERE original_person IN(SELECT value FROM json_each(?1)))",
        "UPDATE assignments SET status='ended',ended_at=max(unixepoch('subsec')*1000,first_observed,coalesce(started_at,first_observed)) WHERE original_person IN(SELECT value FROM json_each(?1)) AND status!='ended'",
        "UPDATE persons SET name='',admin=0,enabled=0,revision=revision+1 WHERE id IN(SELECT value FROM json_each(?1))",
    ] {
        sqlx::query(statement)
            .bind(&family)
            .execute(&mut *db)
            .await?;
    }
    sqlx::query("INSERT INTO forgotten_people SELECT value,?2 FROM json_each(?1)")
        .bind(family)
        .bind(mark)
        .execute(&mut *db)
        .await?;
    crate::host::plugin_sessions::scrub_inactive(db).await?;
    status(db, key).await
}
pub(super) async fn capture(
    db: &mut SqliteConnection,
    mark: i64,
    person: &str,
    members: &str,
    ids: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO person_forgets VALUES(?,?,?)")
        .bind(mark)
        .bind(person)
        .bind(members)
        .execute(&mut *db)
        .await?;
    // Catalog only versions and paths actually reachable from erased sources before triggers detach them.
    let versions = "SELECT v.id FROM media_versions v JOIN executions x ON x.id=v.execution WHERE v.source IN(SELECT value FROM json_each(?2)) OR x.source IN(SELECT value FROM json_each(?2)) OR EXISTS(SELECT 1 FROM main_dependencies d JOIN json_each(?2) s ON s.value=d.event WHERE d.execution=x.id)";
    sqlx::query(&format!("INSERT OR IGNORE INTO forgotten_objects SELECT ?1,digest FROM media_versions WHERE id IN({versions}) AND digest IS NOT NULL UNION SELECT ?1,digest FROM media_sources WHERE event IN(SELECT value FROM json_each(?2)) AND digest IS NOT NULL")).bind(mark).bind(ids).execute(&mut *db).await?;
    sqlx::query(&format!("INSERT INTO forgotten_transfers SELECT ?1,relative FROM media_transfer_files WHERE version IN({versions})")).bind(mark).bind(ids).execute(db).await?;
    Ok(())
}
pub async fn status(db: &mut SqliteConnection, key: &str) -> Result<Value> {
    let (mark,person,body):(i64,String,String)=sqlx::query_as("SELECT r.watermark,f.requested,r.sources FROM source_erasure_requests r JOIN person_forgets f USING(watermark) WHERE erase_key=?").bind(key).fetch_optional(&mut *db).await?.ok_or("not_found")?;
    let (all,present,shared):(i64,i64,i64)=sqlx::query_as("SELECT count(*),count(b.digest),coalesce(sum(EXISTS(SELECT 1 FROM media_sources s WHERE s.digest=o.digest AND s.state='cached') OR EXISTS(SELECT 1 FROM media_versions v WHERE v.digest=o.digest)),0) FROM forgotten_objects o LEFT JOIN media_objects b ON b.digest=o.digest WHERE watermark=?").bind(mark).fetch_one(&mut *db).await?;
    let files:i64=sqlx::query_scalar("SELECT count(*) FROM forgotten_transfers t JOIN media_transfer_files f USING(relative) WHERE t.watermark=?").bind(mark).fetch_one(&mut *db).await?;
    let pending:Vec<String>=sqlx::query_scalar("SELECT DISTINCT e.owner FROM owner_erasures e WHERE e.proof IS NULL AND EXISTS(SELECT 1 FROM json_each(e.sources) s JOIN json_each(?1) erased ON s.value=erased.value) ORDER BY e.owner").bind(&body).fetch_all(db).await?;
    Ok(
        json!({"key":key,"person_id":person,"deleted":true,"watermark":mark,"logical":"revoked","sources":serde_json::from_str::<Vec<i64>>(&body)?,"managed_storage":{"unreferenced_blobs_removed":all-present,"shared_blobs_retained":shared,"unreferenced_blobs_pending":present-shared,"transfer_paths_pending":files},"owners_pending":pending,"external_boundaries":["third-party messages and provider records","devices or copies outside this owner","backups require replay of confirmed erasure watermarks"]}),
    )
}
