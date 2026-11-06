//! Admin's original usage projection: logical calls, physical known subtotals and missing fields.
use super::*;
use chrono::{DateTime, Duration, Offset, TimeZone, Utc};
use sqlx::Row;
use std::collections::{BTreeMap, BTreeSet};

const FIELDS: [(&str, &str); 7] = [
    ("prompt_tokens", "input"),
    ("completion_tokens", "output"),
    ("total_tokens", "total"),
    ("cached_prompt_tokens", "cache"),
    ("cache_creation_input_tokens", "cache_creation"),
    ("cache_creation_5m_input_tokens", "cache_creation_5m"),
    ("cache_creation_1h_input_tokens", "cache_creation_1h"),
];
#[derive(Deserialize)]
pub(super) struct Query {
    range: String,
    timezone: String,
    #[serde(default = "group_model")]
    group_by: String,
    end: Option<String>,
    task: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    profile_id: Option<String>,
}
fn group_model() -> String {
    "model".into()
}
#[derive(Default, Clone)]
struct Stats {
    calls: i64,
    successes: i64,
    unknown: i64,
    latency: f64,
    fields: [i64; 7],
    coverage: [i64; 7],
    paired: i64,
    paired_input: i64,
    paired_cache: i64,
}
impl Stats {
    fn add(&mut self, other: &Self) -> Result<()> {
        for (v, n) in [
            &mut self.calls,
            &mut self.successes,
            &mut self.unknown,
            &mut self.paired,
            &mut self.paired_input,
            &mut self.paired_cache,
        ]
        .into_iter()
        .zip([
            other.calls,
            other.successes,
            other.unknown,
            other.paired,
            other.paired_input,
            other.paired_cache,
        ]) {
            *v = v.checked_add(n).ok_or("usage_overflow")?;
        }
        for i in 0..7 {
            self.fields[i] = self.fields[i]
                .checked_add(other.fields[i])
                .ok_or("usage_overflow")?;
            self.coverage[i] = self.coverage[i]
                .checked_add(other.coverage[i])
                .ok_or("usage_overflow")?;
        }
        self.latency += other.latency;
        Ok(())
    }
    fn value(&self) -> Value {
        let mut value = json!({"invocations":self.calls,"successes":self.successes,"failures":self.calls-self.successes,"unknown_usage":self.unknown,
            "average_latency_seconds":if self.calls>0 {self.latency/self.calls as f64} else {0.},
            "coverage":{},"cache_read":{"reported_calls":self.paired,"prompt_tokens":self.paired_input,"cached_prompt_tokens":self.paired_cache,"ratio":if self.paired_input>0 {Some(self.paired_cache as f64/self.paired_input as f64)} else {None}}});
        for (i, (public, _)) in FIELDS.iter().enumerate() {
            value[*public] = json!(self.fields[i]);
            value["coverage"][*public] = json!(self.coverage[i]);
        }
        value
    }
}
fn instant(text: &str) -> Result<DateTime<Utc>> {
    if let Ok(t) = DateTime::parse_from_rfc3339(text) {
        return Ok(t.with_timezone(&Utc));
    }
    Ok(chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%.f")?.and_utc())
}
type Dimensions = (String, String, String, String);
pub(super) async fn read(db: &mut SqliteConnection, args: &Value) -> Result<Value> {
    let q: Query = serde_json::from_value(args.clone()).map_err(|_| "validation_error")?;
    let zone: chrono_tz::Tz = q.timezone.parse().map_err(|_| "validation_error")?;
    let end = if let Some(end) = &q.end {
        DateTime::parse_from_rfc3339(end)
            .map_err(|_| "validation_error")?
            .with_timezone(&Utc)
    } else {
        DateTime::from_timestamp_millis(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis()
                .try_into()?,
        )
        .ok_or("validation_error")?
    };
    let start = match q.range.as_str() {
        "all" => None,
        "24h" => Some(end - Duration::hours(24)),
        "7d" => Some(end - Duration::days(7)),
        "today" => {
            let midnight = end
                .with_timezone(&zone)
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .ok_or("validation_error")?;
            // An ambiguous midnight uses the first occurrence, matching fold=0 in v4.
            Some(
                if let Some(t) = zone.from_local_datetime(&midnight).earliest() {
                    t.with_timezone(&Utc)
                } else {
                    let before = zone
                        .from_local_datetime(&(midnight - Duration::seconds(1)))
                        .latest()
                        .ok_or("validation_error")?;
                    midnight.and_utc()
                        - Duration::seconds(i64::from(before.offset().fix().local_minus_utc()))
                },
            )
        }
        _ => return Err("validation_error".into()),
    };
    if !matches!(q.group_by.as_str(), "model" | "task") {
        return Err("validation_error".into());
    }
    let rows=sqlx::query("SELECT d.*,t.ordinal,t.state,t.usage_json,EXISTS(SELECT 1 FROM model_usage_unbilled u WHERE u.attempt=t.id) AS unbilled,t.dispatched_at,t.received_at FROM model_usage_dimensions d JOIN model_attempts t ON t.logical=d.logical ORDER BY d.logical,t.ordinal").fetch_all(db).await?;
    let mut logical = BTreeMap::<
        String,
        (
            Dimensions,
            DateTime<Utc>,
            Option<DateTime<Utc>>,
            String,
            Stats,
        ),
    >::new();
    for row in rows {
        let id: String = row.try_get("logical")?;
        let dims = (
            row.try_get("task")?,
            row.try_get("provider")?,
            row.try_get("model")?,
            row.try_get("profile_id")?,
        );
        let dispatched = instant(row.try_get("dispatched_at")?)?;
        let item =
            logical
                .entry(id)
                .or_insert((dims, dispatched, None, String::new(), Stats::default()));
        item.1 = item.1.min(dispatched);
        let received: Option<&str> = row.try_get("received_at")?;
        if let Some(t) = received {
            item.2 = Some(instant(t)?);
        }
        item.3 = row.try_get("state")?;
        // Charges belong to their saved physical dispatch time. A retry can
        // cross a calendar boundary without moving the earlier attempt's usage.
        if dispatched >= end || start.is_some_and(|s| dispatched < s) {
            continue;
        }
        item.4.calls = 1;
        let usage = row
            .try_get::<Option<&str>, _>("usage_json")?
            .map(serde_json::from_str::<Value>)
            .transpose()?
            .unwrap_or(Value::Null);
        let mut known = [false; 7];
        for (i, (_, field)) in FIELDS.iter().enumerate() {
            if let Some(n) = usage[*field].as_i64().filter(|n| *n >= 0) {
                item.4.fields[i] = item.4.fields[i].checked_add(n).ok_or("usage_overflow")?;
                item.4.coverage[i] = 1;
                known[i] = true;
            }
        }
        // Do not pair an input-only attempt with a cache-only later attempt.
        // The public count remains logical invocations; only co-reported
        // physical fields contribute to the numerator and denominator.
        if known[0] && known[3] {
            item.4.paired = 1;
            item.4.paired_input = item
                .4
                .paired_input
                .checked_add(usage["input"].as_i64().ok_or("invalid_usage")?)
                .ok_or("usage_overflow")?;
            item.4.paired_cache = item
                .4
                .paired_cache
                .checked_add(usage["cache"].as_i64().ok_or("invalid_usage")?)
                .ok_or("usage_overflow")?;
        }
        let unbilled: bool = row.try_get("unbilled")?;
        if !unbilled && (!known[2] || matches!(item.3.as_str(), "unknown" | "dispatched")) {
            item.4.unknown = 1;
        }
    }
    let mut sources = BTreeMap::<Dimensions, Stats>::new();
    for (_, (dims, created, received, state, mut stats)) in logical {
        if stats.calls == 0 || state == "dispatched" {
            continue;
        }
        stats.calls = 1;
        stats.successes = i64::from(matches!(state.as_str(), "response" | "paired"));
        stats.latency = received
            .map(|t| (t - created).num_milliseconds().max(0) as f64 / 1000.)
            .unwrap_or(0.);
        sources.entry(dims).or_default().add(&stats)?;
    }
    let tasks: BTreeSet<_> = sources.keys().map(|d| d.0.clone()).collect();
    let models: BTreeSet<_> = sources.keys().map(|d| (d.1.clone(), d.2.clone())).collect();
    let mut summary = Stats::default();
    let mut groups = BTreeMap::<Vec<String>, (Stats, Vec<Value>)>::new();
    for (d, stats) in sources {
        if [&q.task, &q.provider, &q.model, &q.profile_id]
            .into_iter()
            .zip([&d.0, &d.1, &d.2, &d.3])
            .any(|(filter, actual)| filter.as_ref().is_some_and(|f| f != actual))
        {
            continue;
        }
        summary.add(&stats)?;
        let key = if q.group_by == "task" {
            vec![d.0.clone()]
        } else {
            vec![d.1.clone(), d.2.clone()]
        };
        let group = groups.entry(key).or_default();
        group.0.add(&stats)?;
        group.1.push(
            json!({"task":d.0,"provider":d.1,"model":d.2,"profile_id":d.3,"stats":stats.value()}),
        );
    }
    let mut groups=groups.into_iter().map(|(key,(stats,sources))|json!({"task":if q.group_by=="task"{Some(&key[0])}else{None},"provider":if q.group_by=="model"{Some(&key[0])}else{None},"model":key.get(1),"stats":stats.value(),"sources":sources})).collect::<Vec<_>>();
    groups.sort_by_key(|g| std::cmp::Reverse(g["stats"]["total_tokens"].as_i64().unwrap_or(0)));
    Ok(
        json!({"range":{"range":q.range,"timezone":q.timezone,"start":start.map(|t|t.to_rfc3339()),"end":end.to_rfc3339()},"group_by":q.group_by,
        "filters":{"task":q.task,"provider":q.provider,"model":q.model,"profile_id":q.profile_id},
        "options":{"tasks":tasks,"models":models.into_iter().map(|(provider,model)|json!({"provider":provider,"model":model})).collect::<Vec<_>>()},"summary":summary.value(),"groups":groups}),
    )
}
