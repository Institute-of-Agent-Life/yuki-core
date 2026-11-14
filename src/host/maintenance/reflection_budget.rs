use crate::Result;
use chrono::{DateTime, NaiveDateTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ReflectionBudget {
    timezone: String,
    max_daily_calls: u32,
    quiet_seconds: Option<u32>,
}

impl Default for ReflectionBudget {
    fn default() -> Self {
        Self {
            timezone: "Asia/Shanghai".into(),
            max_daily_calls: 96,
            quiet_seconds: None,
        }
    }
}

impl ReflectionBudget {
    pub(crate) fn validate(&self) -> Result<()> {
        self.timezone.parse::<Tz>()?;
        Ok(())
    }

    pub(crate) fn quiet_seconds(&self) -> Option<u32> {
        self.quiet_seconds
    }

    pub(crate) async fn admit(
        &self,
        db: &mut SqliteConnection,
        attempt: &str,
        purpose: &str,
        origin: &serde_json::Value,
    ) -> Result<()> {
        if purpose != "reflect" || !origin["maintenance"].is_string() {
            return Ok(());
        }
        let timezone: Tz = self.timezone.parse()?;
        let dispatched: String =
            sqlx::query_scalar("SELECT dispatched_at FROM model_attempts WHERE id=?")
                .bind(attempt)
                .fetch_one(&mut *db)
                .await?;
        let day = NaiveDateTime::parse_from_str(&dispatched, "%Y-%m-%d %H:%M:%S")?
            .and_utc()
            .with_timezone(&timezone)
            .date_naive();
        let midnight = day.and_hms_opt(0, 0, 0).ok_or("invalid reflection day")?;
        let next = day
            .succ_opt()
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .ok_or("invalid next reflection day")?;
        let boundary = |wall| -> Result<String> {
            let utc = crate::work::schedule::wall_to_utc(wall, timezone)?;
            Ok(DateTime::<Utc>::from_timestamp_millis(utc)
                .ok_or("invalid reflection day boundary")?
                .format("%Y-%m-%d %H:%M:%S")
                .to_string())
        };
        let attempts: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM host_aux_requests q
             JOIN model_attempts a ON a.logical=q.logical
             WHERE q.purpose='reflect' AND json_type(q.origin,'$.maintenance')='text'
               AND a.dispatched_at>=? AND a.dispatched_at<?",
        )
        .bind(boundary(midnight)?)
        .bind(boundary(next)?)
        .fetch_one(db)
        .await?;
        if attempts > i64::from(self.max_daily_calls) {
            return Err("reflection_daily_limit_reached".into());
        }
        Ok(())
    }
}
