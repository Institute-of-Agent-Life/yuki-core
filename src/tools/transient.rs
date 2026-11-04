use crate::Result;
use chrono::{DateTime, Datelike, SecondsFormat, Utc};
use chrono_tz::Tz;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> Result<i64> {
    Ok(i64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
    )?)
}
pub fn clock(seconds: i64, zone: &str) -> Result<Value> {
    let zone: Tz = zone.parse()?;
    let utc: DateTime<Utc> =
        DateTime::from_timestamp(seconds, 0).ok_or("time outside supported range")?;
    let local = utc.with_timezone(&zone);
    Ok(
        json!({"utc":utc.to_rfc3339_opts(SecondsFormat::Secs,true),"local":local.to_rfc3339_opts(SecondsFormat::Secs,false),"timezone":zone.name(),"date":local.date_naive().to_string(),"weekday":match local.weekday().num_days_from_monday(){0=>"Monday",1=>"Tuesday",2=>"Wednesday",3=>"Thursday",4=>"Friday",5=>"Saturday",_=>"Sunday"}}),
    )
}
pub async fn update(
    conn: &mut SqliteConnection,
    exec: &str,
    args: &Value,
    now: i64,
) -> Result<Value> {
    let slot = args["slot"]
        .as_i64()
        .filter(|s| (1..=3).contains(s))
        .ok_or("invalid short-state slot")?;
    let text = args["text"]
        .as_str()
        .filter(|s| s.chars().count() <= 300)
        .ok_or("invalid short-state text")?
        .trim();
    sqlx::query("UPDATE short_state SET text='' WHERE expires<=?")
        .bind(now)
        .execute(&mut *conn)
        .await?;
    sqlx::query("INSERT INTO short_state VALUES(?,?,1,?,?) ON CONFLICT(slot) DO UPDATE SET text=excluded.text,revision=revision+1,expires=excluded.expires,execution=excluded.execution")
        .bind(slot).bind(text).bind(now.checked_add(86400).ok_or("expiry overflow")?).bind(exec).execute(conn).await?;
    Ok(json!({"slot":slot,"text":text}))
}
pub async fn live(conn: &mut SqliteConnection, exec: &str, now: i64) -> Result<Vec<Value>> {
    let rows=sqlx::query("SELECT slot,text,revision,expires,execution FROM short_state WHERE text<>'' AND expires>? ORDER BY slot").bind(now).fetch_all(&mut *conn).await?;
    let mut items = Vec::new();
    for row in rows {
        // These three slots are shared transient data. Bind any source propagated into a new request.
        let origin: String = row.try_get("execution")?;
        sqlx::query("INSERT OR IGNORE INTO main_dependencies(execution,event) SELECT ?1,id FROM events WHERE id IN (SELECT source FROM executions WHERE id=?2 UNION SELECT event FROM main_dependencies WHERE execution=?2) AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=events.id)").bind(exec).bind(origin).execute(&mut *conn).await?;
        items.push(json!({"slot":row.try_get::<i64,_>("slot")?,"text":row.try_get::<String,_>("text")?,"revision":row.try_get::<i64,_>("revision")?,"expires_at":row.try_get::<i64,_>("expires")?}));
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_new_time_read_crosses_shanghai_midnight() {
        let first = clock(1704038399, "Asia/Shanghai").unwrap();
        let next = clock(1704038400, "Asia/Shanghai").unwrap();
        assert_eq!(first["date"], "2023-12-31");
        assert_eq!(next["date"], "2024-01-01");
        assert_eq!(next["weekday"], "Monday");
    }
}
