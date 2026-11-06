//! Original v4 query contract over the same canonical events exposed by personal clients.
mod receipts;
mod rows;
use super::{Kind, Matcher, Mode, Order, Query, Scope, index, scope};
use crate::{
    Result,
    history::Change,
    store::{Mutation, Store},
    tools::registry::Tool,
};
use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Request {
    view: Kind,
    target: Option<Value>,
    source: Option<Value>,
    order: Order,
    keyword: String,
    #[serde(rename = "match")]
    mode: Mode,
    exclude: Vec<String>,
    person_id: Option<String>,
    author_kind: Option<String>,
    event_kind: String,
    start_at: Option<String>,
    end_at: Option<String>,
    has: Vec<String>,
    limit: Option<i64>,
    cursor: Option<i64>,
    event_id: Option<i64>,
    before: Option<i64>,
    after: Option<i64>,
    context_before: i64,
    context_after: i64,
}
impl Default for Request {
    fn default() -> Self {
        Self {
            view: Kind::Recent,
            target: None,
            source: None,
            order: Order::Recent,
            keyword: String::new(),
            mode: Mode::All,
            exclude: vec![],
            person_id: None,
            author_kind: None,
            event_kind: "message".into(),
            start_at: None,
            end_at: None,
            has: vec![],
            limit: None,
            cursor: None,
            event_id: None,
            before: None,
            after: None,
            context_before: 0,
            context_after: 0,
        }
    }
}
fn date(raw: Option<&str>) -> Result<Option<DateTime<Utc>>> {
    raw.map(|raw| {
        if let Ok(t) = DateTime::parse_from_rfc3339(raw) {
            return Ok(t.with_timezone(&Utc));
        }
        let t = NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f")?;
        Ok(FixedOffset::east_opt(8 * 3600)
            .ok_or("invalid default timezone")?
            .from_local_datetime(&t)
            .single()
            .ok_or("invalid history date")?
            .with_timezone(&Utc))
    })
    .transpose()
}
impl Request {
    fn parse(args: &str) -> Result<Self> {
        let mut q: Self = serde_json::from_str(args)?;
        if q.limit.is_some_and(|n| n < 1 || n == i64::MAX)
            || q.cursor.is_some_and(|n| n < 1)
            || q.event_id.is_some_and(|n| n < 1)
            || q.before.is_some_and(|n| n < 0)
            || q.after.is_some_and(|n| n < 0)
            || q.context_before < 0
            || q.context_after < 0
            || !matches!(q.event_kind.as_str(), "message" | "external_event" | "all")
            || q.author_kind
                .as_deref()
                .is_some_and(|s| !matches!(s, "person" | "yuki" | "external_bot" | "system"))
            || q.has
                .iter()
                .any(|s| !matches!(s.as_str(), "image" | "video" | "file" | "audio"))
        {
            return Err("invalid history query".into());
        }
        if let Some(person) = &mut q.person_id {
            if !super::contacts::uuid(person) {
                return Err("invalid history person ID".into());
            }
            *person = person.to_ascii_lowercase();
        }
        if let Some(target) = &mut q.target {
            let object = target.as_object_mut().ok_or("invalid history target")?;
            if object.keys().any(|k| {
                !matches!(
                    k.as_str(),
                    "conversation_id" | "person_id" | "space_id" | "binding_id" | "operation_id"
                )
            }) || object.values().filter(|v| !v.is_null()).count() != 1
            {
                return Err("exactly one original target required".into());
            }
            for value in object.values_mut().filter(|v| !v.is_null()) {
                let raw = value.as_str().ok_or("invalid history target ID")?;
                if !super::contacts::uuid(raw) {
                    return Err("invalid history target ID".into());
                }
                *value = json!(raw.to_ascii_lowercase());
            }
        }
        if let Some(source) = &q.source {
            let object = source.as_object().ok_or("invalid history source")?;
            if object
                .keys()
                .any(|k| !matches!(k.as_str(), "platform" | "scope"))
                || object
                    .get("platform")
                    .is_some_and(|v| !v.is_null() && !v.is_string())
                || object.get("scope").is_some_and(|v| {
                    !v.is_null() && !matches!(v.as_str(), Some("private" | "group"))
                })
            {
                return Err("invalid history source".into());
            }
        }
        q.start_at = date(q.start_at.as_deref())?.map(|t| t.to_rfc3339());
        q.end_at = date(q.end_at.as_deref())?.map(|t| t.to_rfc3339());
        Ok(q)
    }
    fn matcher(&self) -> Result<Matcher> {
        Matcher::new(&Query {
            text: self.keyword.clone(),
            mode: self.mode,
            exclude: self.exclude.clone(),
            ..Query::default()
        })
    }
}
pub(crate) fn declaration() -> Tool {
    let d: Value = serde_json::from_str(include_str!(
        "../../../fixtures/history/v4-query-declaration.json"
    ))
    .expect("fixed history declaration");
    Tool {
        name: "query_chat_history".into(),
        description: d["tool"]["description"].as_str().unwrap().into(),
        schema: d["tool"]["parameters"].clone(),
        parallel_read: true,
        cacheable: false,
        code_direct: false,
        provider: None,
    }
}

async fn sources(
    db: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    q: &Request,
    s: &Scope,
    receipt: Option<&Value>,
) -> Result<BTreeMap<String, Value>> {
    if receipt.is_some_and(|r| r["original_conversation_id"].is_null()) {
        return Ok(BTreeMap::new());
    }
    let target = if let Some(r) = receipt {
        json!({"conversation_id":r["original_conversation_id"]})
    } else {
        q.target.clone().unwrap_or(Value::Null)
    };
    let args = json!({"target":target,"source":q.source,"limit":i64::MAX-1});
    let directory = super::contacts::read(db, exec, actor, "find_conversations", &args).await?;
    let source_selected = q
        .source
        .as_ref()
        .and_then(Value::as_object)
        .is_some_and(|m| m.values().any(|v| !v.is_null()));
    let home = matches!(q.view, Kind::Recent) && q.target.is_none() && !source_selected;
    let mut result = BTreeMap::new();
    for r in directory["items"]
        .as_array()
        .ok_or("missing source directory")?
    {
        let id = r["conversation_id"]
            .as_str()
            .ok_or("missing original conversation")?;
        if home && id != s.home {
            continue;
        }
        let generation: i64 = sqlx::query_scalar(
            "SELECT coalesce((SELECT generation FROM main_windows WHERE conversation=?),1)",
        )
        .bind(id)
        .fetch_one(&mut *db)
        .await?;
        result.insert(
            id.into(),
            json!({"conversation_id":id,"platform":r["platform"],"scope":r["scope"],
            "conversation_name":if r["scope"]=="private"{r["display_name"].clone()}else{json!("")},
            "binding_id":r["binding_id"],"presence_id":Value::Null,"thread_id":r["thread_id"],
            "person_id":r["person_id"],"space_id":r["space_id"],"generation":generation}),
        );
    }
    if result.is_empty()
        && q.target.is_some()
        && receipt.is_none()
        && !source_selected
        && !matches!(q.view, Kind::Around)
    {
        return Err("history_unavailable".into());
    }
    Ok(result)
}
async fn receipt(db: &mut SqliteConnection, q: &Request, s: &Scope) -> Result<Option<Value>> {
    let Some(id) = q.target.as_ref().and_then(|t| t["operation_id"].as_str()) else {
        return Ok(None);
    };
    // A receipt can select only a send initiated in the original invocation's home.
    let candidates=sqlx::query("SELECT o.namespace,o.operation_id,o.state,x.source,send.entry,d.event FROM operations o JOIN executions x ON x.id=o.namespace LEFT JOIN events e ON e.id=x.source LEFT JOIN main_origins m ON m.ref=x.origin LEFT JOIN social_parts part ON part.namespace=o.namespace AND part.operation_id=o.operation_id LEFT JOIN social_outbound send ON send.namespace=o.namespace AND send.child=coalesce(part.child,o.operation_id) LEFT JOIN personal_deliveries d ON d.outbound=send.id WHERE o.operation_id=? AND o.target IN('send','complete') AND coalesce(e.entry_id,m.conversation)=? ORDER BY part.ordinal").bind(id).bind(&s.home).fetch_all(&mut *db).await?;
    if candidates
        .iter()
        .map(|r| r.get::<&str, _>("namespace"))
        .collect::<BTreeSet<_>>()
        .len()
        != 1
    {
        return Err("history_receipt_unavailable".into());
    }
    let r = candidates.first().ok_or("history_receipt_unavailable")?;
    let event = r.get::<Option<i64>, _>("event");
    let available = if let Some(event) = event {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM history_public_events WHERE event=?)",
        )
        .bind(event)
        .fetch_one(&mut *db)
        .await?
    } else {
        false
    };
    let status = if available {
        "available"
    } else if event.is_some() {
        "missing"
    } else {
        "unavailable"
    };
    Ok(Some(
        json!({"operation_id":id,"receipt_status":match r.get::<&str,_>("state"){"confirmed"=>"succeeded",v=>v},
        "event_id":event,"original_conversation_id":r.get::<Option<String>,_>("entry"),"anchor_status":status,
        "receipt_anchor":{"event_id":event,"conversation_id":r.get::<Option<String>,_>("entry"),"status":status}}),
    ))
}

async fn select(
    db: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    q: &Request,
    s: &Scope,
    cutoff: i64,
) -> Result<Value> {
    let mut receipt = receipt(db, q, s).await?;
    let mut sources = sources(db, exec, actor, q, s, receipt.as_ref()).await?;
    let mut matched = serde_json::Map::new();
    let mut context = vec![];
    let mut extra = serde_json::Map::new();
    let selected;
    if matches!(q.view, Kind::Around) {
        let center = q
            .event_id
            .or_else(|| receipt.as_ref().and_then(|r| r["event_id"].as_i64()))
            .ok_or("around requires event_id")?;
        let entry: Option<String> = sqlx::query_scalar(
            "SELECT entry FROM history_public_events WHERE event=? AND event<=?",
        )
        .bind(center)
        .bind(cutoff)
        .fetch_optional(&mut *db)
        .await?;
        sources.retain(|id, _| Some(id) == entry.as_ref());
        let (before, at, after) = rows::around(
            db,
            q,
            &sources,
            cutoff,
            center,
            q.before.unwrap_or(0),
            q.after.unwrap_or(0),
        )
        .await?;
        extra.insert("center_event_id".into(), json!(center));
        extra.insert("before".into(), json!(before.len()));
        extra.insert("after".into(), json!(after.len()));
        extra.insert("has_more".into(), json!(false));
        extra.insert(
            "center_status".into(),
            json!(if at.is_some() {
                "available"
            } else {
                "unavailable"
            }),
        );
        if at.is_none() {
            extra.insert("anchor_status".into(), json!("unavailable"));
        }
        selected = before
            .into_iter()
            .chain(at)
            .chain(after)
            .collect::<Vec<_>>();
    } else {
        let (items, more) = rows::page(
            db,
            q,
            &sources,
            cutoff,
            rows::Page {
                cursor: q.cursor,
                limit: q.limit.unwrap_or(20),
                desc: matches!(q.order, Order::Recent),
                filters: true,
            },
        )
        .await?;
        extra.insert("has_more".into(), json!(more));
        extra.insert(
            "next_cursor".into(),
            if more {
                items
                    .last()
                    .map(|r| r["event_id"].clone())
                    .unwrap_or(Value::Null)
            } else {
                Value::Null
            },
        );
        extra.insert("start_at".into(), json!(q.start_at));
        extra.insert("end_at".into(), json!(q.end_at));
        let mut neighbours = BTreeMap::new();
        for row in &items {
            let id = row["event_id"].as_i64().ok_or("missing original event")?;
            matched.insert(id.to_string(), row["_matched_fields"].clone());
            if q.context_before > 0 || q.context_after > 0 {
                let one = sources
                    .iter()
                    .filter(|(key, _)| row["conversation_id"] == key.as_str())
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let (before, _, after) =
                    rows::around(db, q, &one, cutoff, id, q.context_before, q.context_after)
                        .await?;
                for r in before.into_iter().chain(after) {
                    neighbours.insert(r["event_id"].as_i64().ok_or("missing neighbour")?, r);
                }
            }
        }
        for row in &items {
            neighbours.remove(&row["event_id"].as_i64().unwrap());
        }
        context.extend(neighbours.into_values());
        selected = items;
    }
    let mut returned = selected
        .iter()
        .chain(&context)
        .filter_map(|r| r["conversation_id"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let mut receipt_row = None;
    if let Some(r) = &mut receipt {
        let event = r["event_id"].as_i64();
        let found = if let Some(event) = event {
            rows::one(db, q, &sources, cutoff, event).await?
        } else {
            None
        };
        if let Some(row) = found {
            r["anchor_status"] = json!("available");
            r["receipt_anchor"]["status"] = json!("available");
            returned.push(row["conversation_id"].as_str().unwrap().to_owned());
            if !selected
                .iter()
                .chain(&context)
                .any(|v| v["event_id"] == row["event_id"])
            {
                receipt_row = Some(row)
            }
        } else if r["anchor_status"] == "available" {
            r["anchor_status"] = json!("unavailable");
            r["receipt_anchor"]["status"] = json!("unavailable");
        }
    }
    let mut seen = BTreeSet::new();
    returned.retain(|v| seen.insert(v.clone()));
    let single = if sources.len() == 1 {
        sources.values().next()
    } else {
        None
    };
    let mut result = json!({"source":"canonical_ledger","coverage":"observed","conversation_id":single.map(|v|v["conversation_id"].clone()),
        "events":selected,"returned_count":selected.len(),"order":if matches!(q.view,Kind::Around)||matches!(q.order,Order::Oldest){"oldest"}else{"recent"},
        "matched_fields":matched,"context":context,"sources":returned.iter().filter_map(|id|sources.get(id)).collect::<Vec<_>>()});
    result.as_object_mut().unwrap().extend(extra);
    if let Some(source) = single {
        for field in ["platform", "binding_id", "generation"] {
            result[field] = source[field].clone();
        }
        result["kind"] = json!(if source["scope"] == "private" {
            "person"
        } else {
            "space"
        });
        result["target_id"] = if source["person_id"].is_null() {
            source["space_id"].clone()
        } else {
            source["person_id"].clone()
        };
    }
    if let Some(mut r) = receipt {
        if let Some(row) = receipt_row {
            r["receipt_anchor"]["event"] = row;
        }
        result
            .as_object_mut()
            .unwrap()
            .extend(r.as_object().unwrap().clone());
    }
    Ok(result)
}
fn strip(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("_matched_fields");
            map.remove("_item");
            for v in map.values_mut() {
                strip(v);
            }
        }
        Value::Array(values) => {
            for v in values {
                strip(v);
            }
        }
        _ => {}
    }
}
pub async fn run(
    store: &Store,
    exec: &str,
    actor: &str,
    args: &str,
    operation: Option<&str>,
) -> Result<Value> {
    let q = Request::parse(args)?;
    let canonical = serde_json::to_string(&q)?;
    let cutoff: i64 = sqlx::query_scalar("SELECT coalesce(max(id),0) FROM events")
        .fetch_one(store.readers())
        .await?;
    let item_cutoff: i64 = sqlx::query_scalar("SELECT coalesce(max(id),0) FROM history_items")
        .fetch_one(store.readers())
        .await?;
    loop {
        let mut tx = store.readers().begin().await?;
        let s = scope(&mut tx, exec, actor).await?;
        if let Some(key) = operation
            && let Some(saved) = receipts::replay(&mut tx, exec, key, &canonical, &s).await?
        {
            return Ok(saved);
        }
        let unread:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM history_items WHERE visible AND indexed!=revision AND id<=?)").bind(item_cutoff).fetch_one(&mut *tx).await?;
        if unread {
            drop(tx);
            index::prepare_batch(store, item_cutoff).await?;
            continue;
        }
        let result = select(&mut tx, exec, actor, &q, &s, cutoff).await?;
        if let Some(key) = operation {
            drop(tx);
            store
                .write(Mutation::History(Box::new(Change::PublicAnchor {
                    exec: exec.into(),
                    actor: actor.into(),
                    key: key.into(),
                    args: canonical.clone(),
                    result,
                })))
                .await?;
        } else {
            let mut result = result;
            strip(&mut result);
            return Ok(result);
        }
    }
}
pub(crate) use receipts::anchor;
pub(crate) use receipts::material;
