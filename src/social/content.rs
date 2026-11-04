use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::collections::BTreeSet;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Emoji {
    pub goal: String,
    #[serde(default)]
    pub emotion: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub kind: String,
    pub id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub target: Option<Target>,
    #[serde(default)]
    pub text: String,
    pub emoji: Option<Emoji>,
    pub artifact_id: Option<String>,
    pub attachment_kind: Option<String>,
    pub name: Option<String>,
    pub reply_to_event_id: Option<i64>,
}
impl Message {
    pub fn validate(&self) -> Result<()> {
        if self
            .name
            .as_ref()
            .is_some_and(|n| n.is_empty() || n.contains(['/', '\\', '\0']))
            || self
                .emoji
                .as_ref()
                .is_some_and(|m| m.goal.trim().is_empty())
            || self.artifact_id.is_some() != self.attachment_kind.is_some()
            || self.emoji.is_some() && self.artifact_id.is_some()
            || self.text.trim().is_empty() && self.artifact_id.is_none() && self.emoji.is_none()
        {
            return Err("invalid social message".into());
        }
        Ok(())
    }
}
pub fn outbound_text(raw: &str) -> String {
    let body = raw
        .rsplit_once("<yuki-state>")
        .map_or(raw, |(body, _)| body);
    let normalized = body.replace("\r\n", "\n").replace('\r', "\n");
    let clean: String = normalized
        .chars()
        .filter(|c| !matches!(*c,'\0'..='\x08'|'\x0b'|'\x0c'|'\x0e'..='\x1f'|'\x7f'))
        .collect();
    clean
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .into()
}

pub fn schema() -> Value {
    json!({"type":"object","properties":{"target":{"type":"object","properties":{"kind":{"enum":["person","space"]},"id":{"type":"string","minLength":1}},"required":["kind","id"],"additionalProperties":false},"text":{"type":"string"},"emoji":{"type":"object","properties":{"goal":{"type":"string","minLength":1},"emotion":{"type":"string"}},"required":["goal"],"additionalProperties":false},
        "artifact_id":{"type":"string"},"attachment_kind":{"enum":["image","file"]},"name":{"type":"string"},"reply_to_event_id":{"type":"integer"}},
        "anyOf":[{"required":["text"]},{"required":["emoji"]},{"required":["artifact_id","attachment_kind"]}],"additionalProperties":false})
}

struct Candidate {
    account: String,
    native: String,
    name: String,
}

async fn candidates(
    conn: &mut SqliteConnection,
    entry: &str,
    text: &str,
) -> Result<Vec<Candidate>> {
    let mut names = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for (_, tail) in text.match_indices('@').map(|(i, _)| (i, &text[i + 1..])) {
        if let Some(id) = tail
            .strip_prefix("{account:")
            .and_then(|s| s.split_once('}').map(|p| p.0))
        {
            ids.insert(id);
        }
        let mut name = String::new();
        for c in tail
            .trim_start_matches([' ', '\t'])
            .chars()
            .take(64)
            .take_while(|c| *c != '\n' && *c != '\r')
        {
            name.push(c);
            names.insert(name.clone());
        }
    }
    if names.is_empty() && ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows=sqlx::query("WITH matches AS(SELECT n.account,a.native,n.name,n.occurred,n.source,row_number() OVER(PARTITION BY n.account,n.name ORDER BY n.occurred DESC,n.source DESC) AS recent
        FROM qq_social_names n JOIN accounts a ON a.id=n.account JOIN events e ON e.id=n.source LEFT JOIN assignments stage ON stage.id=e.actor_stage
        WHERE n.entry=? AND (n.name IN(SELECT value FROM json_each(?)) OR n.account IN(SELECT value FROM json_each(?)))
        AND (e.actor_stage IS NULL OR stage.status='active')) SELECT account,native,name FROM matches WHERE recent=1 ORDER BY occurred DESC,source DESC,account")
        .bind(entry).bind(serde_json::to_string(&names)?).bind(serde_json::to_string(&ids)?).fetch_all(conn).await?;
    rows.into_iter()
        .map(|r| {
            Ok(Candidate {
                account: r.try_get("account")?,
                native: r.try_get("native")?,
                name: r.try_get("name")?,
            })
        })
        .collect()
}

fn text_segment(text: &str) -> Value {
    json!({"type":"text","data":{"text":text}})
}
fn mentions(line: &str, candidates: &[Candidate]) -> Vec<Value> {
    let mut segments = Vec::new();
    let mut literal = String::new();
    let mut offset = 0;
    while let Some(at) = line[offset..].find('@').map(|n| n + offset) {
        literal.push_str(&line[offset..at]);
        let tail = &line[at + 1..];
        let word = line[..at]
            .rsplit(char::is_whitespace)
            .next()
            .unwrap_or_default();
        let suffix = tail.split_whitespace().next().unwrap_or_default();
        if literal.ends_with('\\') {
            literal.pop();
            literal.push('@');
            offset = at + 1;
            continue;
        }
        if word.contains("://")
            || (!word.is_empty()
                && word
                    .chars()
                    .last()
                    .is_some_and(|c| c.is_ascii_alphanumeric())
                && suffix.contains('.'))
        {
            literal.push('@');
            offset = at + 1;
            continue;
        }
        let trimmed = tail.trim_start_matches([' ', '\t']);
        let found = if let Some((id, rest)) = tail
            .strip_prefix("{account:")
            .and_then(|s| s.split_once('}'))
        {
            candidates
                .iter()
                .find(|c| c.account == id)
                .map(|c| (c, line.len() - rest.len()))
        } else {
            candidates
                .iter()
                .filter(|c| !c.name.is_empty() && trimmed.starts_with(&c.name))
                .fold(None, |best: Option<&Candidate>, c| {
                    if best.is_none_or(|b| c.name.len() > b.name.len()) {
                        Some(c)
                    } else {
                        best
                    }
                })
                .map(|c| (c, line.len() - trimmed.len() + c.name.len()))
        };
        if let Some((c, end)) = found {
            if !literal.is_empty() {
                segments.push(text_segment(&literal));
                literal.clear();
            }
            segments.push(json!({"type":"at","data":{"qq":c.native}}));
            offset = end;
        } else {
            literal.push('@');
            offset = at + 1;
        }
    }
    literal.push_str(&line[offset..]);
    if !literal.is_empty() {
        segments.push(text_segment(&literal));
    }
    segments
}

pub struct Part {
    pub action: String,
    pub params: Value,
}
pub async fn plan(
    conn: &mut SqliteConnection,
    exec: &str,
    route: &super::send::Route,
    msg: &Message,
    limits: super::split::Limits,
) -> Result<Vec<Part>> {
    msg.validate()?;
    let (entry, group, target) = (&route.entry, route.group, route.target);
    let names = candidates(conn, entry, &msg.text).await?;
    let dest = if group { "group_id" } else { "user_id" };
    let send = if group {
        "send_group_msg"
    } else {
        "send_private_msg"
    };
    let mut bodies = Vec::<Vec<Value>>::new();
    if !msg.text.contains('@') {
        bodies = super::split::plain(&msg.text, limits)
            .into_iter()
            .map(|s| vec![text_segment(&s)])
            .collect();
    } else {
        for section in super::split::sections(&msg.text, limits.hard_max_messages) {
            if section.is_empty() {
                continue;
            }
            let segments = mentions(&section, &names);
            let mut current = Vec::new();
            let mut length = 0;
            for part in segments {
                if part["type"] == "text" {
                    let mut chunk = String::new();
                    for c in part["data"]["text"].as_str().unwrap().chars() {
                        if length >= limits.max_qq_message_chars
                            && bodies.len() + 1 < limits.hard_max_messages
                        {
                            if !chunk.is_empty() {
                                current.push(text_segment(&chunk));
                                chunk.clear();
                            }
                            bodies.push(std::mem::take(&mut current));
                            length = 0;
                        }
                        chunk.push(c);
                        length += 1;
                    }
                    if !chunk.is_empty() {
                        current.push(text_segment(&chunk));
                    }
                } else {
                    current.push(part);
                    length += 1;
                }
            }
            if !current.is_empty() {
                if bodies.len() >= limits.hard_max_messages {
                    bodies[limits.hard_max_messages - 1].push(text_segment("\n"));
                    bodies[limits.hard_max_messages - 1].extend(current);
                } else {
                    bodies.push(current);
                }
            }
        }
    }
    let mut parts = Vec::new();
    if let Some(asset) = &msg.artifact_id {
        let row=sqlx::query("SELECT b.kind,b.filename,b.peer_reference FROM social_media_bindings b JOIN media_versions v ON v.id=b.id AND v.execution=b.execution WHERE b.id=? AND b.execution=? AND v.digest IS NOT NULL")
            .bind(asset).bind(exec).fetch_one(&mut *conn).await?;
        let kind: String = row.try_get("kind")?;
        if Some(&kind) != msg.attachment_kind.as_ref() {
            return Err("attachment kind differs from authorized binding".into());
        }
        let reference: String = row.try_get("peer_reference")?;
        if kind == "image" {
            if bodies.is_empty() {
                bodies.push(Vec::new());
            }
            bodies[0].push(json!({"type":"image","data":{"file":reference}}));
        } else {
            parts.push(Part{action:if group{"upload_group_file"}else{"upload_private_file"}.into(),
                params:json!({dest:target,"file":reference,"name":msg.name.as_ref().unwrap_or(&row.try_get("filename")?)})});
        }
    }
    for body in bodies {
        parts.push(Part {
            action: send.into(),
            params: json!({dest:target,"message":body}),
        });
    }
    Ok(parts)
}
