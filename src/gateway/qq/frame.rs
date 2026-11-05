use super::{GroupPolicy, Inner};
use crate::{
    Result,
    ingress::admit::{self, Frame, Kind, Segment, Sender},
    store::Mutation,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) fn id(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) if n.is_i64() || n.is_u64() => Some(n.to_string()),
        _ => None,
    }
}

async fn account(inner: &Inner, native: &str, observe: bool) -> Result<Option<String>> {
    let existing = sqlx::query_scalar(
        "SELECT id FROM accounts WHERE platform='qq' AND namespace=? AND native=?",
    )
    .bind(&inner.namespace)
    .bind(native)
    .fetch_optional(inner.store.readers())
    .await?;
    if existing.is_some() || !observe {
        return Ok(existing);
    }
    let result = inner
        .store
        .write(Mutation::ObserveQqAccount(
            inner.namespace.clone(),
            native.into(),
        ))
        .await?;
    Ok(Some(
        result.as_str().ok_or("missing observed account")?.into(),
    ))
}

fn face(id: &str) -> Option<String> {
    static FACES: OnceLock<HashMap<String, String>> = OnceLock::new();
    FACES
        .get_or_init(|| {
            serde_json::from_str(include_str!("../../../config/qq_face_map.json"))
                .expect("checked-in QQ face map")
        })
        .get(id)
        .cloned()
}

async fn segments(inner: &Inner, raw: &Value) -> Result<Vec<Segment>> {
    if let Some(text) = raw.as_str() {
        // String-form CQ-style text has no independently observed native mention segment.
        return Ok(vec![Segment::Text { text: text.into() }]);
    }
    let mut segments = Vec::new();
    for (index, item) in raw
        .as_array()
        .ok_or("missing OneBot segment list")?
        .iter()
        .enumerate()
    {
        let data = &item["data"];
        let segment = match item["type"].as_str() {
            Some("text") => Segment::Text {
                text: data["text"].as_str().unwrap_or("").into(),
            },
            Some("at") => {
                let Some(native) = id(&data["qq"]) else {
                    continue;
                };
                if native != "all" && !native.bytes().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                Segment::Mention {
                    account: account(inner, &native, false).await?,
                    label: if native == "all" {
                        "全体成员".into()
                    } else {
                        data["name"].as_str().unwrap_or("").into()
                    },
                    span: Some(crate::ingress::admit::Span {
                        segment: index,
                        start: 0,
                        end: 0,
                    }),
                    native: Some(native),
                }
            }
            Some("face") => {
                let native = id(&data["id"]).ok_or("face lacks id")?;
                Segment::Face {
                    id: native.clone(),
                    label: face(&native),
                }
            }
            Some("forward") => Segment::Media {
                kind: "forward".into(),
                reference: id(&data["id"]).ok_or("forward lacks reference")?,
            },
            Some("reply") => Segment::Reply {
                native_id: id(&data["id"]).ok_or("reply lacks locator")?,
            },
            Some(kind @ ("image" | "record" | "video" | "file")) => {
                let reference = id(&data["file"])
                    .or_else(|| id(&data["file_id"]))
                    .or_else(|| id(&data["url"]))
                    .ok_or("media lacks reference")?;
                Segment::Media {
                    kind: kind.into(),
                    reference,
                }
            }
            _ => continue,
        };
        segments.push(segment);
    }
    Ok(segments)
}

pub(super) async fn receive(inner: &Inner, bot: &str, packet: &Value) -> Result<()> {
    let (kind, group) = match (
        packet["post_type"].as_str(),
        packet["message_type"].as_str(),
        packet["notice_type"].as_str(),
    ) {
        (Some("message"), Some("group"), _) => (Kind::Message, true),
        (Some("message"), Some("private"), _) => (Kind::Message, false),
        (Some("message_sent"), Some(scope), _) => (Kind::Echo, scope == "group"),
        (Some("notice"), _, Some("group_recall")) => (Kind::Recall, true),
        (Some("notice"), _, Some("friend_recall")) => (Kind::Recall, false),
        _ => return Ok(()),
    };
    let group_id = if group {
        Some(id(&packet["group_id"]).ok_or("missing native group")?)
    } else {
        None
    };
    let recovered = if let Some(g) = &group_id {
        let mut reader = inner.store.readers().acquire().await?;
        crate::ingress::group_recovery::route(&mut reader, &inner.namespace, g).await?
    } else {
        None
    };
    if let Some(g) = &group_id {
        if let Some((stage, _, enabled)) = &recovered {
            if !enabled || *stage != inner.bots[bot].spec.stage {
                return Ok(());
            }
        } else if !inner.groups.contains_key(g)
            || matches!(inner.groups.get(g), Some(GroupPolicy::Paused))
        {
            return Ok(());
        }
    }
    let topic = id(&packet["topic_id"]);
    let shared = inner.shared.as_ref().filter(|s| {
        group_id.as_ref() == Some(&s.space) && s.topic == topic && s.receivers.contains(bot)
    });
    if group && recovered.is_none() && shared.is_none() && bot != inner.designated {
        return Ok(());
    }
    let native = id(&packet["message_id"]).ok_or("no stable native message reference")?;
    let key = if let Some(s) = shared {
        id(&packet[&s.sequence]).ok_or("verified group sequence missing")?
    } else {
        native.clone()
    };
    let source = match kind {
        Kind::Message => key,
        Kind::Echo => format!("echo:{key}"),
        Kind::Recall => format!("recall:{key}"),
        _ => unreachable!(),
    };
    let human = id(&packet["user_id"]).ok_or("missing native sender")?;
    let actor = account(inner, &human, true)
        .await?
        .ok_or("unknown sender account")?;
    let mut parts = if kind == Kind::Recall {
        Vec::new()
    } else {
        segments(inner, &packet["message"]).await?
    };
    let text =
        super::projection::render(&mut parts, &inner.bots.keys().map(String::as_str).collect());
    let targets: HashSet<_> = parts
        .iter()
        .filter_map(|part| match part {
            Segment::Mention {
                native: Some(target),
                ..
            } if inner.bots.contains_key(target) => Some(target.clone()),
            _ => None,
        })
        .collect();
    // Ambiguous multi-target input has no implicit send route.
    let accept = match group_id.as_ref().and_then(|id| inner.groups.get(id)) {
        None if group => !targets.is_empty(),
        None | Some(GroupPolicy::Open) => true,
        Some(GroupPolicy::Observe) => false,
        Some(GroupPolicy::Paused | GroupPolicy::Mention) => !targets.is_empty(),
    };
    let route = if !group || targets.is_empty() {
        Some(bot.into())
    } else if targets.len() == 1 {
        targets.into_iter().next()
    } else {
        None
    };
    let now = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
    let occurred = packet["time"]
        .as_i64()
        .and_then(|sec| sec.checked_mul(1000));
    let frame = Frame {
        source,
        native_id: Some(native),
        account: actor,
        bot_stage: inner.bots[bot].spec.stage.clone(),
        space: group_id
            .as_ref()
            .map(|id| json!(["qq", inner.namespace, id]).to_string()),
        topic,
        occurred,
        observed: now,
        kind,
        segments: parts,
        sender: Some(Sender {
            nickname: packet["sender"]["nickname"].as_str().map(str::to_owned),
            card: packet["sender"]["card"].as_str().map(str::to_owned),
        }),
        answer_work_id: None,
        reply_bot: route,
        text: Some(text),
    };
    admit::record_scoped(&inner.store, frame, accept, shared.map(|s| s.image.clone())).await?;
    Ok(())
}
