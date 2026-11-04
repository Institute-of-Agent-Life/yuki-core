use crate::ingress::admit::{Segment, Span};
use std::collections::HashSet;

/// Project native mention ranges into sanitized Unicode-scalar offsets.
pub(super) fn render(parts: &mut [Segment], bots: &HashSet<&str>) -> String {
    let mut raw = Vec::new();
    let mut attachments = 0;
    for part in &mut *parts {
        match part {
            Segment::Text { text } => raw.extend(text.chars()),
            Segment::Mention { native, span, .. } => {
                let Some(target) = native else { continue };
                let marker = if target == "all" {
                    "[提及全体成员]".into()
                } else if bots.contains(target.as_str()) {
                    "[提及Agent]".into()
                } else {
                    format!("[提及{target}]")
                };
                if let Some(offsets) = span {
                    offsets.start = raw.len();
                    raw.extend(marker.chars());
                    offsets.end = raw.len();
                }
            }
            Segment::Face { id, label } => raw.extend(
                format!(
                    "[QQ表情：{}]",
                    label.clone().unwrap_or_else(|| format!("ID {id}"))
                )
                .chars(),
            ),
            Segment::Media { kind, reference } => {
                let display = if reference.starts_with("base64://")
                    || reference.starts_with("data:")
                    || reference.contains("://")
                {
                    ""
                } else {
                    reference
                };
                let name = display
                    .rsplit(['\\', '/'])
                    .next()
                    .unwrap_or("")
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(80)
                    .collect::<String>();
                let label = match kind.as_str() {
                    "image" => Some("图片"),
                    "video" => Some("视频"),
                    "file" => Some("文件"),
                    _ => None,
                };
                if let Some(label) = label {
                    let name = name.trim_matches([' ', '[', ']']);
                    let suffix = if name.is_empty() {
                        String::new()
                    } else {
                        format!("：{name}")
                    };
                    raw.extend(format!(" [{label}附件{attachments}{suffix}] ").chars());
                }
                attachments += 1;
            }
            Segment::Opaque { kind } => {
                if !matches!(kind.as_str(), "yuki_context" | "agent_target") {
                    attachments += 1;
                }
            }
            Segment::Reply { .. } => {}
        }
    }
    let mut kept: Vec<(usize, char)> = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        let offset = i;
        let mut ch = raw[i];
        i += 1;
        if ch == '\r' {
            if raw.get(i) == Some(&'\n') {
                i += 1;
            }
            ch = '\n';
        }
        if ch == '\n' {
            while kept
                .last()
                .is_some_and(|(_, c)| *c != '\n' && c.is_whitespace())
            {
                kept.pop();
            }
        } else if (ch < ' ' && ch != '\t') || ch == '\u{7f}' {
            continue;
        }
        kept.push((offset, ch));
    }
    let start = kept
        .iter()
        .position(|(_, c)| !c.is_whitespace())
        .unwrap_or(kept.len());
    let end = kept
        .iter()
        .rposition(|(_, c)| !c.is_whitespace())
        .map_or(start, |n| n + 1);
    let kept = &kept[start..end];
    for part in parts {
        if let Segment::Mention {
            span: Some(Span { start, end, .. }),
            ..
        } = part
        {
            *start = kept.partition_point(|(at, _)| at < start);
            *end = kept.partition_point(|(at, _)| at < end);
        }
    }
    kept.iter().map(|(_, c)| c).collect()
}
