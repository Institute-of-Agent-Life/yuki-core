use crate::{
    Result,
    ingress::admit::{Frame, Segment},
};
use chrono::{DateTime, Utc};
use regex::Regex;
use sqlx::{Row, SqliteConnection};
use std::{
    collections::{BTreeSet, HashSet},
    sync::LazyLock,
};
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
    line_with_audio(conn, id, zone, false).await
}

async fn line_with_audio(
    conn: &mut SqliteConnection,
    id: i64,
    zone: &str,
    pending: bool,
) -> Result<String> {
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
    let mut text = crate::runtime::main::event_text(row.try_get("envelope")?)?;
    let mut mentions = Vec::new();
    if frame
        .segments
        .iter()
        .any(|s| matches!(s, Segment::Mention { .. }))
    {
        // Use the Assignment admitted with the original event. The live QQ bot list can change.
        let owned: HashSet<(i64, String)> = sqlx::query_as(
            "SELECT json_extract(m.value,'$.position'), a.id FROM events e, json_each(e.mentions) m
             JOIN assignments s ON s.id=json_extract(m.value,'$.assignment') AND s.kind='self'
             JOIN accounts a ON a.id=s.account_id AND a.id=json_extract(m.value,'$.account')
               AND a.native=json_extract(m.value,'$.native') WHERE e.id=?",
        )
        .bind(id)
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .collect();
        let mut replacements = Vec::new();
        for (position, part) in frame.segments.iter().enumerate() {
            let Segment::Mention {
                account,
                native,
                label,
                span,
            } = part
            else {
                continue;
            };
            let agent = account
                .as_ref()
                .is_some_and(|a| owned.contains(&(position as i64, a.clone())));
            let label = if agent {
                "Agent".to_owned()
            } else if !label.is_empty() {
                name(label)
            } else if native.as_deref() == Some("all") {
                "全体成员".into()
            } else {
                "未识别成员".into()
            };
            mentions.push(match account {
                Some(a) => format!("{label}/@{{account:{a}}}"),
                None => label.clone(),
            });
            if let Some(r) = span
                && r.segment == position
            {
                replacements.push((r.start, r.end, agent));
            }
        }
        text = render_agent_spans(&text, &replacements);
    }
    if !mentions.is_empty() {
        fields.push(format!("提及:{}", mentions.join(",")));
    }
    let attachments = crate::media::display(conn, id).await?;
    if !attachments.is_empty() {
        fields.push(format!("附件:{}", serde_json::to_string(&attachments)?));
    }
    let clock = match row.try_get::<Option<i64>, _>("occurred_at")? {
        Some(ms) => DateTime::<Utc>::from_timestamp_millis(ms)
            .ok_or("event time outside supported range")?
            .with_timezone(&zone.parse::<chrono_tz::Tz>()?)
            .format("%H:%M:%S｜")
            .to_string(),
        None => String::new(),
    };
    let mut rendered = format!(
        "[{clock}{display}|person:{person}]\n{}>{text}",
        fields.join("|")
    );
    if !frame.segments.iter().any(|segment| {
        matches!(segment, Segment::Media { kind, .. } if matches!(kind.as_str(), "audio" | "record"))
    }) {
        return Ok(rendered);
    }
    let audio: Vec<(i64, Option<String>)> = sqlx::query_as(
        "SELECT s.segment,t.text FROM media_sources s LEFT JOIN history_transcripts t
         ON t.source=s.event AND t.segment=s.segment WHERE s.event=?
         AND s.kind IN('audio','record') AND (t.text IS NOT NULL OR ?2) ORDER BY s.segment",
    )
    .bind(id)
    .bind(pending)
    .fetch_all(conn)
    .await?;
    for (segment, transcript) in audio {
        let content = match transcript {
            Some(text) => {
                // Reader 58 and earlier admitted operator transcripts without
                // the processor's size bound. Keep their history record intact.
                let mut end = text.len().min(32768);
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                let mut result = serde_json::json!({
                    "source_event":id,"segment_index":segment,"text":&text[..end],
                });
                if end < text.len() {
                    result["truncated"] = serde_json::Value::Bool(true);
                }
                result
            }
            None => {
                serde_json::json!({"source_event":id,"segment_index":segment,"state":"not_ready"})
            }
        };
        rendered.push_str(&format!("\n语音转写（原附件内容） {content}"));
    }
    Ok(rendered)
}
// Span offsets refer to the sanitized source in Unicode scalars, never to labels
// or to a textual search for a marker (which may also occur in literal user text).
fn render_agent_spans(text: &str, spans: &[(usize, usize, bool)]) -> String {
    let offsets: Vec<_> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    let mut output = String::with_capacity(text.len());
    let mut last = 0;
    for &(start, end, agent) in spans {
        let (Some(&lo), Some(&hi)) = (offsets.get(start), offsets.get(end)) else {
            continue;
        };
        if lo < last || lo >= hi || &text[lo..hi] != "[提及Agent]" {
            continue;
        }
        output.push_str(&text[last..lo]);
        output.push_str(if agent { "@Agent" } else { "@未识别成员" });
        last = hi;
    }
    output.push_str(&text[last..]);
    output
}
pub async fn trigger(
    conn: &mut SqliteConnection,
    events: &[i64],
    zone: &str,
    audio: Option<&BTreeSet<i64>>,
) -> Result<String> {
    let Some(first) = events.first() else {
        return Ok(String::new());
    };
    let (kind,platform):(String,String)=sqlx::query_as("SELECT d.scope,a.platform FROM events e JOIN entries d ON d.id=e.entry_id JOIN accounts a ON a.id=e.account_id WHERE e.id=?").bind(first).fetch_one(&mut *conn).await?;
    let scene = if kind == "group" { "群聊" } else { "私聊" };
    let mut rendered = Vec::new();
    for id in events {
        rendered
            .push(line_with_audio(conn, *id, zone, audio.is_some_and(|a| a.contains(id))).await?);
    }
    Ok(format!(
        "当前是{scene}。\n资料：{}\n\n{}",
        serde_json::json!({"platform":platform}),
        rendered.join("\n")
    ))
}

#[cfg(test)]
mod mention_render_tests {
    use super::render_agent_spans;

    #[test]
    fn only_proved_native_ranges_can_change_literal_agent_markers() {
        let text = "猫[提及Agent] / [提及Agent] 😸[提及Agent]";
        let marker = "[提及Agent]";
        let ranges: Vec<_> = text
            .match_indices(marker)
            .map(|(byte, _)| {
                (
                    text[..byte].chars().count(),
                    text[..byte + marker.len()].chars().count(),
                )
            })
            .collect();
        assert_eq!(
            render_agent_spans(
                text,
                &[
                    (ranges[1].0, ranges[1].1, true),
                    (ranges[2].0, ranges[2].1, false),
                    (0, usize::MAX, true)
                ]
            ),
            "猫[提及Agent] / @Agent 😸@未识别成员"
        );
        assert_eq!(render_agent_spans(text, &[(0, 3, true)]), text);
    }
}
