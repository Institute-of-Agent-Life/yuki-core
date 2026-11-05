use crate::{
    Result,
    ingress::admit::{Frame, Segment},
};
use chrono::{DateTime, Utc};
use regex::Regex;
use sqlx::{Row, SqliteConnection};
use std::sync::LazyLock;
static CONTROL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\p{C}").expect("Unicode category C"));
fn name(text: &str) -> String {
    let filtered = CONTROL.replace_all(text, "");
    filtered
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(128)
        .collect::<String>()
        .trim_end()
        .to_string()
}
pub async fn line(conn: &mut SqliteConnection, id: i64, zone: &str) -> Result<String> {
    let row=sqlx::query("SELECT e.envelope,e.raw_actor,e.occurred_at,e.entry_id,e.bot_stage,a.native FROM events e JOIN accounts a ON a.id=e.account_id WHERE e.id=? AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)").bind(id).fetch_one(&mut *conn).await?;
    let frame: Frame = serde_json::from_str(row.try_get("envelope")?)?;
    let person: String = row.try_get("raw_actor")?;
    let display = frame
        .sender
        .as_ref()
        .and_then(|s| {
            s.card
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .or(s.nickname.as_deref())
        })
        .map(name)
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    let display = if display.is_empty() {
        name(row.try_get("native")?)
    } else {
        display
    };
    let mut fields = vec![format!("#{id}")];
    if let Some(native) = frame.segments.iter().find_map(|s| {
        if let Segment::Reply { native_id } = s {
            Some(native_id)
        } else {
            None
        }
    }) {
        let target=sqlx::query("SELECT e.id,e.raw_actor,e.envelope FROM event_locators l JOIN events e ON e.id=l.event_id WHERE l.receiver=? AND l.native_id=? AND e.entry_id=? AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)").bind(row.try_get::<String,_>("bot_stage")?).bind(native).bind(row.try_get::<String,_>("entry_id")?).fetch_optional(&mut *conn).await?;
        if let Some(target) = target {
            let raw: Frame = serde_json::from_str(target.try_get("envelope")?)?;
            let display = raw
                .sender
                .as_ref()
                .and_then(|s| s.card.as_deref().or(s.nickname.as_deref()))
                .map(name)
                .unwrap_or_default();
            fields.push(format!(
                "回复:#{}/{}|person:{}",
                target.try_get::<i64, _>("id")?,
                display,
                target.try_get::<String, _>("raw_actor")?
            ));
        } else {
            fields.push("回复:引用不可用".into());
        }
    }
    let mentions: Vec<_> = frame
        .segments
        .iter()
        .filter_map(|s| {
            if let Segment::Mention {
                account,
                native,
                label,
                ..
            } = s
            {
                let label = name(if label.is_empty() {
                    if native.as_deref() == Some("all") {
                        "全体成员"
                    } else {
                        "未识别成员"
                    }
                } else {
                    label
                });
                Some(if let Some(a) = account {
                    format!("{label}/@{{account:{a}}}")
                } else {
                    label
                })
            } else {
                None
            }
        })
        .collect();
    if !mentions.is_empty() {
        fields.push(format!("提及:{}", mentions.join(",")));
    }
    let clock = match row.try_get::<Option<i64>, _>("occurred_at")? {
        Some(ms) => DateTime::<Utc>::from_timestamp_millis(ms)
            .ok_or("event time outside supported range")?
            .with_timezone(&zone.parse::<chrono_tz::Tz>()?)
            .format("%H:%M:%S｜")
            .to_string(),
        None => String::new(),
    };
    let text = crate::runtime::main::event_text(row.try_get("envelope")?)?;
    Ok(format!(
        "[{clock}{display}|person:{person}]\n{}>{text}",
        fields.join("|")
    ))
}
pub async fn trigger(conn: &mut SqliteConnection, events: &[i64], zone: &str) -> Result<String> {
    let Some(first) = events.first() else {
        return Ok(String::new());
    };
    let (kind,platform):(String,String)=sqlx::query_as("SELECT d.scope,a.platform FROM events e JOIN entries d ON d.id=e.entry_id JOIN accounts a ON a.id=e.account_id WHERE e.id=?").bind(first).fetch_one(&mut *conn).await?;
    let scene = if kind == "group" { "群聊" } else { "私聊" };
    let mut rendered = Vec::new();
    for id in events {
        rendered.push(line(conn, *id, zone).await?);
    }
    Ok(format!(
        "当前是{scene}。\n资料：{}\n\n{}",
        serde_json::json!({"platform":platform}),
        rendered.join("\n")
    ))
}
