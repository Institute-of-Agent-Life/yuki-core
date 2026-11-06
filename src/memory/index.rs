use crate::{Result, memory::embedding::profile::Profile};
use sqlx::{Row, SqliteConnection};
use std::{
    ffi::{c_char, c_int},
    sync::OnceLock,
};

unsafe extern "C" {
    fn sqlite3_vec1_extra_init(input: *const c_char) -> c_int;
}
pub fn register() -> Result<()> {
    static STATUS: OnceLock<c_int> = OnceLock::new();
    // The pinned upstream initializer registers a process-lifetime function on the
    // bundled SQLite. It ignores the pointer; no extension path comes from configuration.
    if *STATUS.get_or_init(|| unsafe { sqlite3_vec1_extra_init(std::ptr::null()) }) != 0 {
        return Err("Vec1 registration failed".into());
    }
    Ok(())
}
pub fn table(profile: &str) -> Result<String> {
    if profile.len() != 64
        || !profile
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
    {
        return Err("invalid original profile fingerprint".into());
    }
    Ok(format!("retrieval_vec_{profile}"))
}
pub async fn open(conn: &mut SqliteConnection, p: &Profile) -> Result<bool> {
    let profile = p.identity();
    let name = table(&profile)?;
    let new = sqlx::query("INSERT OR IGNORE INTO retrieval_profiles(profile,dim,upper_bound) SELECT ?,?,coalesce(max(number),0) FROM memory_facts")
        .bind(&profile)
        .bind(i64::try_from(p.dimensions)?)
        .execute(&mut *conn)
        .await?
        .rows_affected()
        > 0;
    if new {
        sqlx::query(&format!(
            "CREATE VIRTUAL TABLE {name} USING vec1(vector,fact_id)"
        ))
        .execute(&mut *conn)
        .await?;
        sqlx::query(&format!("INSERT INTO {name}(cmd,arg) VALUES('rebuild',?)"))
            .bind(r#"{"index":"flat","distance":"cos"}"#)
            .execute(&mut *conn)
            .await?;
    }
    let (cursor, upper_bound): (i64, i64) =
        sqlx::query_as("SELECT cursor,upper_bound FROM retrieval_profiles WHERE profile=?")
            .bind(&profile)
            .fetch_one(&mut *conn)
            .await?;
    if cursor >= upper_bound {
        return Ok(true);
    }
    let next = cursor.saturating_add(512).min(upper_bound);
    sqlx::query("INSERT OR IGNORE INTO retrieval_index_queue SELECT v.profile,v.fact FROM memory_facts f JOIN fact_vectors v ON v.fact=f.id AND v.profile=?1 WHERE f.number>?2 AND f.number<=?3").bind(&profile).bind(cursor).bind(next).execute(&mut *conn).await?;
    sqlx::query("UPDATE retrieval_profiles SET cursor=? WHERE profile=?")
        .bind(next)
        .bind(&profile)
        .execute(conn)
        .await?;
    Ok(next >= upper_bound)
}
pub async fn sync(conn: &mut SqliteConnection) -> Result<()> {
    // Changes on the main writer, including erasures for inactive profiles, publish
    // together with their native mirror. A cold owner drains offline fixture changes.
    let profiles: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT profile FROM retrieval_index_queue")
            .fetch_all(&mut *conn)
            .await?;
    for profile in profiles {
        let name = table(&profile)?;
        let dirty = "SELECT f.number FROM retrieval_index_queue q JOIN memory_facts f ON f.id=q.fact WHERE q.profile=?1";
        sqlx::query(&format!("DELETE FROM {name} WHERE rowid IN({dirty})"))
            .bind(&profile)
            .execute(&mut *conn)
            .await?;
        sqlx::query("DELETE FROM retrieval_index_members WHERE profile=?1 AND fact IN(SELECT fact FROM retrieval_index_queue WHERE profile=?1)").bind(&profile).execute(&mut *conn).await?;
        let eligible = " FROM fact_vectors v JOIN retrieval_profiles p ON p.profile=v.profile AND p.dim=v.dimension JOIN retrieval_index_queue q ON q.profile=v.profile AND q.fact=v.fact JOIN memory_facts f ON f.id=v.fact AND f.revision=v.revision AND f.status='active' WHERE q.profile=?1 AND v.purpose='document' AND length(v.vector)=4*v.dimension AND v.rowid=(SELECT max(x.rowid) FROM fact_vectors x WHERE x.fact=v.fact AND x.profile=v.profile AND x.revision=v.revision)";
        sqlx::query(&format!(
            "INSERT INTO {name}(rowid,fact_id,vector) SELECT f.number,f.number,v.vector{eligible}"
        ))
        .bind(&profile)
        .execute(&mut *conn)
        .await?;
        sqlx::query(&format!("INSERT INTO retrieval_index_members SELECT v.profile,v.fact,v.revision,v.content_hash{eligible}")).bind(&profile).execute(&mut *conn).await?;
        sqlx::query("DELETE FROM retrieval_index_queue WHERE profile=?")
            .bind(&profile)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}
pub async fn nearest(
    conn: &mut SqliteConnection,
    profile: &str,
    ids: &[i64],
    vector: &[f32],
    limit: usize,
) -> Result<Vec<i64>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let name = table(profile)?;
    let sql = nearest_sql(&name);
    let bytes: Vec<u8> = vector.iter().flat_map(|x| x.to_le_bytes()).collect();
    let rows = sqlx::query(&sql)
        .bind(bytes)
        .bind(serde_json::json!({"K":limit}).to_string())
        .bind(serde_json::to_string(ids)?)
        .fetch_all(conn)
        .await?;
    rows.iter().map(|r| Ok(r.try_get("fact_id")?)).collect()
}

pub fn nearest_sql(name: &str) -> String {
    format!("SELECT fact_id FROM {name}(?1,?2) WHERE fact_id IN(SELECT value FROM json_each(?3))")
}
