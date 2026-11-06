use crate::Result;
use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDateTime, Offset, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum Trigger {
    After {
        seconds: u64,
    },
    Once {
        local_datetime: String,
    },
    Interval {
        seconds: u64,
    },
    Daily {
        hour: u8,
        minute: u8,
        timezone: Option<String>,
    },
    Weekly {
        weekdays: Vec<u8>,
        hour: u8,
        minute: u8,
        timezone: Option<String>,
    },
    Cron {
        expression: String,
        timezone: Option<String>,
    },
}

impl Trigger {
    pub fn initial(&self, now: i64, default_zone: &str) -> Result<i64> {
        let due = match self {
            Self::After { seconds } | Self::Interval { seconds } => {
                let delta = i64::try_from(*seconds)?
                    .checked_mul(1000)
                    .ok_or("interval overflow")?;
                now.checked_add(delta).ok_or("instant overflow")?
            }
            Self::Once { local_datetime } => {
                DateTime::parse_from_rfc3339(local_datetime)?.timestamp_millis()
            }
            _ => self.calendar_next(now, default_zone)?,
        };
        if due <= now || DateTime::<Utc>::from_timestamp_millis(due).is_none() {
            return Err("scheduled instant must be representable and in the future".into());
        }
        Ok(due)
    }

    pub fn after_completion(&self, planned: i64, now: i64, zone: &str) -> Result<Option<i64>> {
        if matches!(self, Self::After { .. } | Self::Once { .. }) {
            Ok(None)
        } else {
            self.initial(planned.max(now), zone).map(Some)
        }
    }

    fn calendar_next(&self, after: i64, default_zone: &str) -> Result<i64> {
        let (calendar, override_zone) = match self {
            Self::Weekly {
                weekdays,
                hour,
                minute,
                timezone,
            } => {
                if weekdays.is_empty() || weekdays.iter().any(|d| !(1..=7).contains(d)) {
                    return Err("weekly requires ISO weekdays 1 through 7".into());
                }
                let days = weekdays
                    .iter()
                    .map(|d| (d % 7).to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                (format!("{minute} {hour} * * {days}"), timezone)
            }
            Self::Daily {
                hour,
                minute,
                timezone,
            } => (format!("{minute} {hour} * * *"), timezone),
            Self::Cron {
                expression,
                timezone,
            } => (expression.clone(), timezone),
            _ => return Err("calendar trigger required".into()),
        };
        let zone: Tz = override_zone.as_deref().unwrap_or(default_zone).parse()?;
        let cal = Cron::parse(&calendar)?;
        let instant = DateTime::<Utc>::from_timestamp_millis(after).ok_or("invalid start")?;
        let mut day = instant.with_timezone(&zone).date_naive();
        // Every Gregorian month/day/weekday combination repeats within 400 years.
        for _ in 0..=146_097 {
            let dom = cal.days.contains(&day.day());
            let dow = cal.weekdays.contains(&day.weekday().num_days_from_sunday());
            let matches_day = if cal.either_day {
                dom || dow
            } else {
                dom && dow
            };
            if matches_day && cal.months.contains(&day.month()) {
                let mut earliest: Option<i64> = None;
                for &hour in &cal.hours {
                    for &minute in &cal.minutes {
                        let wall = day
                            .and_hms_opt(hour, minute, 0)
                            .ok_or("invalid wall clock")?;
                        let utc = wall_to_utc(wall, zone)?;
                        if utc > after {
                            earliest = Some(earliest.map_or(utc, |old| old.min(utc)));
                        }
                    }
                }
                if let Some(due) = earliest {
                    return Ok(due);
                }
            }
            day = day.succ_opt().ok_or("date overflow")?;
        }
        Err("calendar has no representable occurrence".into())
    }
}

pub(crate) fn wall_to_utc(wall: NaiveDateTime, zone: Tz) -> Result<i64> {
    match zone.from_local_datetime(&wall) {
        LocalResult::Single(t) => Ok(t.timestamp_millis()),
        LocalResult::Ambiguous(first, second) => Ok(first.min(second).timestamp_millis()),
        LocalResult::None => {
            // v4 uses ZoneInfo fold=0. A nonexistent time keeps the offset before
            // the jump, including half-hour shifts and a skipped local date.
            for minutes in 1..=2880 {
                let prior = wall
                    .checked_sub_signed(Duration::minutes(minutes))
                    .ok_or("date underflow")?;
                if let Some(before) = zone.from_local_datetime(&prior).earliest() {
                    return Ok(wall.and_utc().timestamp_millis()
                        - i64::from(before.offset().fix().local_minus_utc()) * 1000);
                }
            }
            Err("cannot resolve original time-zone offset".into())
        }
    }
}

struct Cron {
    minutes: Vec<u32>,
    hours: Vec<u32>,
    days: Vec<u32>,
    months: Vec<u32>,
    weekdays: Vec<u32>,
    either_day: bool,
}
impl Cron {
    fn parse(rule: &str) -> Result<Self> {
        let fields = rule.split_whitespace().collect::<Vec<_>>();
        let [minute, hour, day, month, weekday] = fields.as_slice() else {
            return Err("cron requires five numeric fields".into());
        };
        let weekdays = field(weekday, 0, 7)?.into_iter().map(|d| d % 7).collect();
        Ok(Self {
            minutes: field(minute, 0, 59)?,
            hours: field(hour, 0, 23)?,
            days: field(day, 1, 31)?,
            months: field(month, 1, 12)?,
            weekdays,
            either_day: *day != "*" && *weekday != "*",
        })
    }
}

/// Accept comma lists, inclusive ranges and explicit step sizes. Leave unsupported
/// names, relative weekdays and ambiguous scalar/step forms to the caller.
fn field(text: &str, min: u32, max: u32) -> Result<Vec<u32>> {
    let number = |s: &str| -> Result<u32> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err("numeric cron field required".into());
        }
        Ok(s.parse()?)
    };
    let mut values = Vec::new();
    for part in text.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, number(s)?),
            None => (part, 1),
        };
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((l, h)) = range.split_once('-') {
            (number(l)?, number(h)?)
        } else if part.contains('/') {
            return Err("cron step requires * or a range".into());
        } else {
            let n = number(range)?;
            (n, n)
        };
        if step == 0 || lo < min || hi > max || lo > hi {
            return Err("cron field out of range".into());
        }
        values.extend((lo..=hi).step_by(usize::try_from(step)?));
    }
    values.sort_unstable();
    values.dedup();
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    fn ms(s: &str) -> i64 {
        DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()
    }
    #[test]
    fn original_calendar_and_separate_cron_agree_across_transitions() {
        let gold: Value = serde_json::from_str(include_str!(
            "../../../fixtures/schedule/v4-clock-tools.json"
        ))
        .unwrap();
        for v in gold["clock"].as_array().unwrap() {
            let trigger: Trigger = serde_json::from_value(v["trigger"].clone()).unwrap();
            let now = ms(v["now"].as_str().unwrap());
            let due = ms(v["due"].as_str().unwrap());
            assert_eq!(trigger.initial(now, "Asia/Shanghai").unwrap(), due);
            assert_eq!(
                trigger
                    .after_completion(due, ms(v["settled"].as_str().unwrap()), "Asia/Shanghai")
                    .unwrap(),
                v["next"].as_str().map(ms)
            );
            match trigger {
                Trigger::Daily {
                    hour,
                    minute,
                    timezone,
                }
                | Trigger::Weekly {
                    hour,
                    minute,
                    timezone,
                    ..
                } => {
                    let dow = if v["trigger"]["type"] == "weekly" {
                        "0,7"
                    } else {
                        "*"
                    };
                    let cron = Trigger::Cron {
                        expression: format!("{minute} {hour} * * {dow}"),
                        timezone,
                    };
                    assert_eq!(cron.initial(now, "Asia/Shanghai").unwrap(), due);
                }
                _ => {}
            }
        }
        for bad in [
            "0 2 * FOO *",
            "*/0 * * * *",
            "0 2 * * 8",
            "0 3 * * MON#2",
            "0 2 30 2 *",
            "0 * * *",
        ] {
            assert!(
                Trigger::Cron {
                    expression: bad.into(),
                    timezone: None
                }
                .initial(0, "UTC")
                .is_err()
            );
        }
    }
}
