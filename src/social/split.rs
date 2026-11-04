use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Limits {
    pub max_qq_message_chars: usize,
    pub hard_max_messages: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_qq_message_chars: 1800,
            hard_max_messages: 10,
        }
    }
}

static STRUCTURED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*(?:`{3}|~~~|[-*+]\s+|\d+[.)、]\s+|>\s+|#{1,6}\s+|\|.*\|[ \t]*$)")
        .unwrap()
});
static FENCED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)(`{3}[^\n]*\n.*?\n`{3}|~~~[^\n]*\n.*?\n~~~)").unwrap());
static PARAGRAPH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n\s*\n").unwrap());

pub fn sections(text: &str, max: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    if STRUCTURED.is_match(text) {
        return vec![text.into()];
    }
    let lines: Vec<_> = text
        .split('\n')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    match lines.len() {
        0 | 1 => vec![text.into()],
        n if n <= max => lines.into_iter().map(str::to_owned).collect(),
        n => {
            let mut groups = vec![Vec::new(); max];
            for (i, line) in lines.into_iter().enumerate() {
                groups[i * max / n].push(line);
            }
            groups.into_iter().map(|g| g.join("\n")).collect()
        }
    }
}
fn hard(s: &str, limit: usize) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    chars.chunks(limit).map(|p| p.iter().collect()).collect()
}
fn sentences(text: &str, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for s in text.split_inclusive(['。', '！', '？', '!', '?', '；', ';']) {
        let n = s.chars().count();
        if n > limit {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            out.extend(hard(s, limit));
        } else {
            if current.chars().count() + n > limit {
                out.push(std::mem::take(&mut current));
            }
            current.push_str(s);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}
fn pieces<'a>(s: &'a str, re: &Regex) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut start = 0;
    for m in re.find_iter(s) {
        if start < m.start() {
            parts.push(&s[start..m.start()]);
        }
        parts.push(m.as_str());
        start = m.end();
    }
    if start < s.len() {
        parts.push(&s[start..]);
    }
    parts
}
fn paragraphs(text: &str, limit: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for p in pieces(text, &PARAGRAPH) {
        let n = p.chars().count();
        if n > limit {
            chunks.push(std::mem::take(&mut current));
            chunks.extend(sentences(p, limit));
        } else {
            if current.chars().count() + n > limit {
                chunks.push(std::mem::take(&mut current));
            }
            current.push_str(p);
        }
    }
    chunks.push(current);
    chunks
        .into_iter()
        .map(|s| s.trim().into())
        .filter(|s: &String| !s.is_empty())
        .collect()
}
fn fenced(block: &str, limit: usize) -> Vec<String> {
    let lines: Vec<_> = block.lines().collect();
    if block.chars().count() <= limit || lines.len() < 3 {
        return vec![block.into()];
    }
    let (opening, closing) = (lines[0], lines[lines.len() - 1]);
    let available = limit
        .saturating_sub(opening.chars().count() + closing.chars().count() + 2)
        .max(1);
    let wrap = |body: String| format!("{opening}\n{body}\n{closing}");
    let mut out = Vec::new();
    let mut body = Vec::new();
    let mut length = 0;
    for line in &lines[1..lines.len() - 1] {
        let n = line.chars().count();
        let extra = n + usize::from(!body.is_empty());
        if !body.is_empty() && length + extra > available {
            out.push(wrap(body.join("\n")));
            body.clear();
            length = 0;
        }
        if n > available {
            out.extend(hard(line, available).into_iter().map(wrap));
            continue;
        }
        body.push(*line);
        length += extra;
    }
    if !body.is_empty() {
        out.push(wrap(body.join("\n")));
    }
    out
}
pub fn plain(text: &str, limits: Limits) -> Vec<String> {
    let max = limits.hard_max_messages;
    let limit = limits.max_qq_message_chars;
    let mut chunks = Vec::new();
    for s in sections(text, max) {
        if s.chars().count() <= limit {
            chunks.push(s);
            continue;
        }
        for part in pieces(&s, &FENCED) {
            if part.starts_with("```") || part.starts_with("~~~") {
                chunks.extend(fenced(part, limit));
            } else {
                chunks.extend(paragraphs(part, limit));
            }
        }
    }
    if chunks.len() > max {
        let tail = chunks.split_off(max - 1).join("\n");
        chunks.push(tail);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replays_v4_oracle_chunks() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../fixtures/social/split-gold.json")).unwrap();
        for c in fixture["cases"].as_array().unwrap() {
            let limits = Limits {
                max_qq_message_chars: c["limit"].as_u64().unwrap() as usize,
                hard_max_messages: c["cap"].as_u64().unwrap() as usize,
            };
            assert_eq!(
                serde_json::json!(plain(c["text"].as_str().unwrap(), limits)),
                c["expected"],
                "{}",
                c["text"]
            );
        }
    }
}
