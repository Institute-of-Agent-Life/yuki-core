use super::{Request, Result, date};
use serde_json::{Value, json};
use sqlx::{QueryBuilder, Row, Sqlite, SqliteConnection};
use std::collections::BTreeMap;

pub(super) struct Page {
    pub cursor: Option<i64>,
    pub limit: i64,
    pub desc: bool,
    pub filters: bool,
}

fn base<'a>(
    sources: &BTreeMap<String, Value>,
    cutoff: i64,
    kind: &str,
) -> Result<QueryBuilder<'a, Sqlite>> {
    let mut sql = QueryBuilder::new(
        "SELECT h.item,h.event,h.entry,h.source_event,h.outbound,e.envelope,e.raw_actor,e.actor_stage,e.account_id,e.bot_stage,e.occurred_at,e.observed_at,e.mentions,o.body,o.observed_at AS sent_at,p.name FROM history_public_events h JOIN events e ON e.id=h.event LEFT JOIN social_outbound o ON o.id=h.outbound LEFT JOIN persons p ON p.id=e.raw_actor WHERE h.event<=",
    );
    sql.push_bind(cutoff)
        .push(" AND h.entry IN(SELECT value FROM json_each(")
        .push_bind(serde_json::to_string(&sources.keys().collect::<Vec<_>>())?)
        .push("))");
    if kind != "all" {
        sql.push(" AND (CASE WHEN h.outbound IS NOT NULL THEN 'message' ELSE h.kind END)=")
            .push_bind(kind.to_owned());
    }
    // A control input is a permanent event, but not a conversation message.
    sql.push(" AND NOT EXISTS(SELECT 1 FROM main_controls c WHERE c.event=h.event)");
    Ok(sql)
}

pub(super) async fn page(
    db: &mut SqliteConnection,
    q: &Request,
    sources: &BTreeMap<String, Value>,
    cutoff: i64,
    options: Page,
) -> Result<(Vec<Value>, bool)> {
    if options.limit == 0 || sources.is_empty() {
        return Ok((vec![], false));
    }
    let matcher = q.matcher()?;
    let entries = sources.keys().cloned().collect::<Vec<_>>();
    let fts = if options.filters {
        matcher.fts(&entries)
    } else {
        None
    };
    let mut cursor = options.cursor;
    let mut output = vec![];
    while (output.len() as i64) <= options.limit {
        let mut sql = base(sources, cutoff, &q.event_kind)?;
        if let Some(id) = cursor {
            sql.push(if options.desc {
                " AND h.event<"
            } else {
                " AND h.event>"
            })
            .push_bind(id);
        }
        if let Some(fts) = &fts {
            sql.push(" AND h.item IN(SELECT rowid FROM history_fts WHERE history_fts MATCH ")
                .push_bind(fts.clone())
                .push(")");
        }
        if options.filters {
            if let Some(author) = &q.person_id {
                sql.push(" AND h.outbound IS NULL AND e.raw_actor IN(SELECT id FROM persons WHERE coalesce(current_root,id)=(SELECT coalesce(current_root,id) FROM persons WHERE id=").push_bind(author.clone()).push("))");
            }
            if let Some(kind) = &q.author_kind {
                sql.push(" AND (CASE WHEN h.outbound IS NOT NULL THEN 'yuki' WHEN json_extract(e.envelope,'$.kind')='external_event' THEN 'system' WHEN e.raw_actor IS NOT NULL THEN 'person' ELSE 'external_bot' END)=").push_bind(kind.clone());
            }
            if let Some(t) = date(q.start_at.as_deref())? {
                sql.push(" AND coalesce(o.observed_at,e.occurred_at)>=")
                    .push_bind(t.timestamp_millis());
            }
            if let Some(t) = date(q.end_at.as_deref())? {
                sql.push(" AND coalesce(o.observed_at,e.occurred_at)<")
                    .push_bind(t.timestamp_millis());
            }
        }
        sql.push(if options.desc {
            " ORDER BY h.event DESC LIMIT 64"
        } else {
            " ORDER BY h.event LIMIT 64"
        });
        let batch = sql.build().fetch_all(&mut *db).await?;
        let count = batch.len();
        for r in batch {
            cursor = Some(r.get("event"));
            let fields:Vec<(String,String,String,String)>=sqlx::query_as("SELECT label,raw,base,simplified FROM history_fields WHERE item=? ORDER BY CASE WHEN label='content' THEN -1 ELSE CAST(substr(label,18) AS INTEGER) END").bind(r.get::<i64,_>("item")).fetch_all(&mut *db).await?;
            let labels = if options.filters {
                matcher.matched(&fields)
            } else {
                Some(vec![])
            };
            let Some(labels) = labels else { continue };
            let mut value = render(db, &r, sources, &fields).await?;
            if options.filters
                && !q.has.is_empty()
                && !value["attachments"].as_array().unwrap().iter().any(|a| {
                    q.has.iter().any(|kind| {
                        a["kind"] == kind.as_str() || kind == "audio" && a["kind"] == "record"
                    })
                })
            {
                continue;
            }
            value["_matched_fields"] = json!(labels);
            output.push(value);
            if output.len() as i64 > options.limit {
                break;
            }
        }
        if count < 64 {
            break;
        }
    }
    let more = output.len() as i64 > options.limit;
    if more {
        output.pop();
    }
    Ok((output, more))
}
pub(super) async fn one(
    db: &mut SqliteConnection,
    q: &Request,
    sources: &BTreeMap<String, Value>,
    cutoff: i64,
    id: i64,
) -> Result<Option<Value>> {
    let mut sql = base(sources, cutoff, &q.event_kind)?;
    sql.push(" AND h.event=").push_bind(id);
    let Some(row) = sql.build().fetch_optional(&mut *db).await? else {
        return Ok(None);
    };
    let fields=sqlx::query_as("SELECT label,raw,base,simplified FROM history_fields WHERE item=? ORDER BY CASE WHEN label='content' THEN -1 ELSE CAST(substr(label,18) AS INTEGER) END").bind(row.get::<i64,_>("item")).fetch_all(&mut *db).await?;
    Ok(Some(render(db, &row, sources, &fields).await?))
}
pub(super) async fn around(
    db: &mut SqliteConnection,
    q: &Request,
    sources: &BTreeMap<String, Value>,
    cutoff: i64,
    id: i64,
    before: i64,
    after: i64,
) -> Result<(Vec<Value>, Option<Value>, Vec<Value>)> {
    let center = one(db, q, sources, cutoff, id).await?;
    let Some(center) = center else {
        return Ok((vec![], None, vec![]));
    };
    let selected = sources
        .iter()
        .filter(|(k, _)| center["conversation_id"] == k.as_str())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut earlier = page(
        db,
        q,
        &selected,
        cutoff,
        Page {
            cursor: Some(id),
            limit: before,
            desc: true,
            filters: false,
        },
    )
    .await?
    .0;
    earlier.reverse();
    let later = page(
        db,
        q,
        &selected,
        cutoff,
        Page {
            cursor: Some(id),
            limit: after,
            desc: false,
            filters: false,
        },
    )
    .await?
    .0;
    Ok((earlier, Some(center), later))
}
fn timestamp(ms: Option<i64>) -> Result<Value> {
    ms.map(|ms| {
        chrono::DateTime::from_timestamp_millis(ms)
            .ok_or("invalid original clock")
            .map(|t| {
                json!(
                    t.with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
                        .to_rfc3339()
                )
            })
    })
    .transpose()
    .map(|v| v.unwrap_or(Value::Null))
    .map_err(Into::into)
}
async fn render(
    db: &mut SqliteConnection,
    r: &sqlx::sqlite::SqliteRow,
    sources: &BTreeMap<String, Value>,
    fields: &[(String, String, String, String)],
) -> Result<Value> {
    let event: i64 = r.get("event");
    let entry: String = r.get("entry");
    let source = &sources[&entry];
    let frame: Value = serde_json::from_str(r.get("envelope"))?;
    let outbound = r.get::<Option<i64>, _>("outbound").is_some();
    let external = !outbound && frame["kind"] == "external_event";
    let person = if outbound {
        None
    } else {
        r.get::<Option<String>, _>("raw_actor")
    };
    let author = if outbound {
        "yuki"
    } else if external {
        "system"
    } else if person.is_some() {
        "person"
    } else {
        "external_bot"
    };
    let origin = if outbound {
        "social_tool"
    } else if external {
        "plugin_background"
    } else {
        "user_message"
    };
    let mut content = fields
        .iter()
        .find(|f| f.0 == "content")
        .map(|f| f.1.clone())
        .unwrap_or_default();
    let asr = fields
        .iter()
        .filter_map(|(label, text, _, _)| {
            label
                .strip_prefix("audio_transcript:")
                .and_then(|s| s.parse::<i64>().ok())
                .map(|segment| json!({"segment_index":segment,"text":text,"source":"current"}))
        })
        .collect::<Vec<_>>();
    if !asr.is_empty() {
        let speech = asr
            .iter()
            .map(|a| {
                format!(
                    "[当前消息的语音转写（自动识别，可能有误）]\n{}",
                    a["text"].as_str().unwrap()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        content = format!("{content}\n{speech}").trim().to_owned();
    }
    let body: Value = if let Some(raw) = r.get::<Option<String>, _>("body") {
        serde_json::from_str(&raw)?
    } else {
        Value::Null
    };
    let segments = if outbound {
        &body["message"]
    } else {
        &frame["segments"]
    };
    let mut attachments = vec![];
    for (position, segment) in segments.as_array().into_iter().flatten().enumerate() {
        let kind = if segment["type"] == "media" {
            segment["kind"].as_str()
        } else {
            segment["type"].as_str()
        };
        let Some(kind) =
            kind.filter(|k| matches!(*k, "image" | "video" | "file" | "audio" | "record"))
        else {
            continue;
        };
        let mut a =
            json!({"attachment_index":attachments.len(),"segment_index":position,"kind":kind});
        let metadata = if outbound { &segment["data"] } else { segment };
        for name in [
            "media_id",
            "artifact_id",
            "version",
            "filename",
            "name",
            "mime_type",
            "size",
        ] {
            if let Some(v) = metadata.get(name) {
                a[name] = v.clone();
            }
        }
        if a.get("size").is_none()
            && let Some(v) = metadata.get("file_size")
        {
            a["size"] = v.clone();
        }
        attachments.push(a);
    }
    if outbound && body.get("file").is_some() && attachments.is_empty() {
        attachments.push(
            json!({"attachment_index":0,"segment_index":0,"kind":"file","name":body["name"]}),
        );
    }
    let reply = frame["segments"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s["type"] == "reply")
        .and_then(|s| s["native_id"].as_str());
    let reply = if let Some(native) = reply {
        let matches:Vec<i64>=sqlx::query_scalar("SELECT DISTINCT p.event FROM event_locators l JOIN history_public_events p ON p.event=l.event_id WHERE l.receiver=? AND l.native_id=? AND p.entry=? AND p.event<? UNION SELECT d.event FROM social_outbound o JOIN personal_live_deliveries d ON d.id=o.id WHERE o.stage=? AND o.locator=? AND o.entry=? AND d.event<?").bind(r.get::<String,_>("bot_stage")).bind(native).bind(&entry).bind(event).bind(r.get::<String,_>("bot_stage")).bind(native).bind(&entry).bind(event).fetch_all(&mut *db).await?;
        if matches.len() == 1 {
            Some(matches[0])
        } else {
            None
        }
    } else {
        None
    };
    let nickname = frame["sender"]["nickname"].as_str().unwrap_or("");
    let card = frame["sender"]["card"].as_str().unwrap_or("");
    let display = if !card.trim().is_empty() {
        card.to_owned()
    } else if !nickname.trim().is_empty() {
        nickname.to_owned()
    } else if outbound {
        "Agent".into()
    } else {
        match author {
            "system" => "system",
            "external_bot" => "external bot",
            _ => "成员",
        }
        .to_owned()
    };
    let stage = if outbound {
        r.get::<Option<String>, _>("bot_stage")
    } else {
        r.get("actor_stage")
    };
    let occurred = if outbound {
        r.get("sent_at")
    } else {
        r.get("occurred_at")
    };
    Ok(
        json!({"id":event,"event_id":event,"event_kind":if external{"external_event"}else{"message"},
        "source_plugin_id":if external{frame["publisher"].clone()}else{Value::Null},"external_source":if external{frame["external_source"].clone()}else{Value::Null},"external_event_type":if external{frame["event_type"].clone()}else{Value::Null},
        "author_kind":author,"source":origin,"origin":origin,"person_id":person,"author_person_id":person,"presence_id":stage,"author_presence_id":stage,
        "author_account_id":r.get::<String,_>("account_id"),"ingress_presence_id":r.get::<String,_>("bot_stage"),
        "sender_nickname":nickname,"sender_group_card":card,"sender_display_name":display,"scope":source["scope"],"conversation_id":entry,
        "direction":if outbound{"outbound"}else if external{"external"}else{"inbound"},"content":content,"mentions":serde_json::from_str::<Value>(r.get("mentions"))?,"asr":asr,
        "reply_to_event_id":reply,"attachments":attachments,"occurred_at":timestamp(occurred)?,"observed_at":timestamp(Some(r.get("observed_at")))?,
        "platform":source["platform"],"conversation_name":source["conversation_name"],"thread_id":source["thread_id"],"_item":r.get::<i64,_>("item")}),
    )
}
