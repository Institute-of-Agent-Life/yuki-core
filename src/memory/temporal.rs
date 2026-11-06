use crate::Result;
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, SecondsFormat, Timelike, Utc};
use regex::Regex;
use std::sync::LazyLock;

static ISO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)^([0-9]{4}-W[0-9]{2}(?:-[0-9])?|[0-9]{4}W[0-9]{2}[0-9]?|[0-9]{4}-[0-9]{2}-[0-9]{2}|[0-9]{8})(?:.(.+))?$").unwrap()
});
static CLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([0-9]{2})(?:(:?)([0-9]{2})(?:(:?)([0-9]{2}))?)?(?:[.,]([0-9]+))?$").unwrap()
});

fn clock(raw: &str) -> Result<(u32, u32, u32, u32)> {
    let c = CLOCK.captures(raw).ok_or("memory time must be ISO 8601")?;
    if c.get(5).is_some() && c.get(2).map(|v| v.as_str()) != c.get(4).map(|v| v.as_str()) {
        return Err("memory time must be ISO 8601".into());
    }
    let n = |i| -> Result<u32> { Ok(c.get(i).map(|v| v.as_str()).unwrap_or("0").parse()?) };
    let fraction = c.get(6).map(|v| v.as_str()).unwrap_or("");
    let mut micro = fraction.chars().take(6).collect::<String>();
    while micro.len() < 6 {
        micro.push('0');
    }
    Ok((n(1)?, n(3)?, n(5)?, micro.parse()?))
}

fn parse(raw: Option<&str>, zone: chrono_tz::Tz) -> Result<Option<DateTime<Utc>>> {
    let Some(raw) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let c = ISO.captures(raw).ok_or("memory time must be ISO 8601")?;
    let date = &c[1];
    let date = if date.contains('W') {
        let normalized = date.replace('-', "");
        let normalized = if normalized.len() == 7 {
            format!("{normalized}1")
        } else {
            normalized
        };
        NaiveDate::parse_from_str(&normalized, "%GW%V%u")?
    } else {
        NaiveDate::parse_from_str(
            date,
            if date.contains('-') {
                "%Y-%m-%d"
            } else {
                "%Y%m%d"
            },
        )?
    };
    if date.year() < 1 {
        return Err("memory date out of range".into());
    }
    let raw = c.get(2).map(|v| v.as_str()).unwrap_or("00");
    let (time, offset) = if let Some(time) = raw.strip_suffix('Z') {
        (time, Some(0))
    } else if let Some(i) = raw.find(['+', '-']) {
        let (h, m, s, micro) = clock(&raw[i + 1..])?;
        if h * 3600 + m * 60 + s >= 86400 {
            return Err("invalid memory UTC offset".into());
        }
        let sign = if raw.as_bytes()[i] == b'-' { -1 } else { 1 };
        let seconds = i64::from(h * 3600 + m * 60 + s);
        (
            &raw[..i],
            Some(sign * (seconds * 1_000_000 + if seconds == 0 { 0 } else { i64::from(micro) })),
        )
    } else {
        (raw, None)
    };
    let (h, m, s, micro) = clock(time)?;
    let time =
        NaiveTime::from_hms_micro_opt(h, m, s, micro).ok_or("memory time must be ISO 8601")?;
    let wall = date.and_time(time);
    let value = if let Some(offset) = offset {
        wall.and_utc()
            .checked_sub_signed(Duration::microseconds(offset))
            .ok_or("memory date overflow")?
    } else {
        let millis = crate::work::schedule::wall_to_utc(wall, zone)?;
        DateTime::<Utc>::from_timestamp_millis(millis)
            .and_then(|v| v.with_nanosecond(wall.nanosecond()))
            .ok_or("memory date overflow")?
    };
    if !(1..=9999).contains(&value.year()) {
        return Err("memory date out of range".into());
    }
    Ok(Some(value))
}

pub(super) struct Range {
    pub mode: &'static str,
    pub start: Option<String>,
    pub end: Option<String>,
}
pub(super) fn resolve(start: Option<&str>, end: Option<&str>, zone: &str) -> Result<Range> {
    let zone = zone.parse()?;
    let first = parse(start, zone)?;
    let last = parse(end, zone)?;
    if end.is_some() && last.is_none() {
        return Err("temporary memory requires valid_until".into());
    }
    if first.zip(last).is_some_and(|(a, b)| a > b) {
        return Err("memory valid_from must not be after valid_until".into());
    }
    let format = |v: DateTime<Utc>| {
        v.to_rfc3339_opts(
            if v.timestamp_subsec_micros() == 0 {
                SecondsFormat::Secs
            } else {
                SecondsFormat::Micros
            },
            false,
        )
    };
    Ok(Range {
        mode: if end.is_some() {
            "temporary"
        } else {
            "persistent"
        },
        start: first.map(format),
        end: last.map(format),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn original_temporal_vectors() {
        let oracle: serde_json::Value =
            serde_json::from_str(include_str!("../../fixtures/memory/v4-temporal.json")).unwrap();
        for v in oracle["vectors"].as_array().unwrap() {
            let result = super::resolve(
                v["valid_from"].as_str(),
                v["valid_until"].as_str(),
                v["zone"].as_str().unwrap(),
            );
            assert_eq!(result.is_ok(), v["ok"].as_bool().unwrap(), "{v}");
            if let Ok(r) = result {
                assert_eq!(
                    serde_json::json!([r.mode, r.start, r.end]),
                    serde_json::json!([v["mode"], v["start"], v["end"]]),
                    "{v}"
                );
            }
        }
    }
}
