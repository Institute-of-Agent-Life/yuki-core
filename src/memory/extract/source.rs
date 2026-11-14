use crate::{Result, memory::scope};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::collections::BTreeMap;

pub struct Event {
    pub id: i64,
    pub entry: String,
    pub space: Option<String>,
    pub actor: String,
    pub label: String,
    pub content: String,
    pub version: String,
    pub subjects: Value,
    pub aliases: BTreeMap<String, String>,
    pub reply_source: Option<i64>,
    pub occurred: Option<String>,
}
impl Event {
    pub fn payload(&self) -> Value {
        json!({"source_event_id":self.id,"scope_type":if self.space.is_some(){"group"}else{"private"},"sender_label":self.label,"content":self.content,
            "occurred_at":self.occurred,"available_subjects":self.subjects})
    }
}
pub async fn load(conn: &mut SqliteConnection, id: i64, zone: &str) -> Result<Option<Event>> {
    load_inner(conn, id, zone, false).await
}
pub(crate) async fn load_reviewed(
    conn: &mut SqliteConnection,
    id: i64,
    zone: &str,
) -> Result<Option<Event>> {
    load_inner(conn, id, zone, true).await
}
async fn load_inner(
    conn: &mut SqliteConnection,
    id: i64,
    zone: &str,
    historical: bool,
) -> Result<Option<Event>> {
    let row=sqlx::query("SELECT e.*,d.space,d.scope,a.native FROM events e JOIN entries d ON d.id=e.entry_id JOIN assignments s ON s.id=e.actor_stage
        JOIN assignments bot ON bot.id=e.bot_stage JOIN accounts a ON a.id=e.account_id JOIN persons p ON p.id=e.raw_actor JOIN persons root ON root.id=coalesce(p.current_root,p.id)
        WHERE e.id=? AND s.original_person=e.raw_actor AND s.kind='person' AND (?2 OR (s.status='active' AND bot.status='active' AND d.closed_at IS NULL)) AND p.enabled AND root.enabled
        AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)").bind(id).bind(historical).fetch_optional(&mut *conn).await?;
    let Some(row) = row else { return Ok(None) };
    let raw: Value = serde_json::from_str(row.try_get("envelope")?)?;
    if raw["kind"] != "message" {
        return Ok(None);
    }
    let mut content = raw["segments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["type"] == "text")
        .filter_map(|s| s["text"].as_str())
        .collect::<String>();
    let speech: Vec<(i64, String)> = sqlx::query_as(
        "SELECT segment,text FROM history_transcripts WHERE source=? ORDER BY segment",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    for (_, text) in &speech {
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(text);
    }
    let actor: String = row.try_get("raw_actor")?;
    let entry: String = row.try_get("entry_id")?;
    let space: Option<String> = row.try_get("space")?;
    let label = raw["sender"]["card"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .or(raw["sender"]["nickname"].as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(row.try_get("native")?)
        .to_owned();
    let mut subjects = vec![
        json!({"subject_ref":"speaker","display_label":format!("{label}（Person {actor}）"),"allowed_scopes":if space.is_some(){json!(["person","person_group"])}else{json!(["person"])},"relation_to_speaker":"self"}),
    ];
    let mut aliases = BTreeMap::from([("speaker".into(), actor.clone())]);
    let mut reply_source = None;
    if space.is_some() {
        subjects.push(json!({"subject_ref":"group","display_label":"当前 Space","allowed_scopes":["group"],"relation_to_speaker":"current_group"}));
        subjects.push(json!({"subject_ref":"named_member","display_label":"当前群中姓名唯一匹配的成员","allowed_scopes":["person_group"],"relation_to_speaker":"named_member"}));
        let members=sqlx::query("WITH ranked AS (SELECT e.raw_actor,e.envelope,row_number() OVER(PARTITION BY e.raw_actor ORDER BY e.id DESC) AS n FROM events e JOIN entries d ON d.id=e.entry_id JOIN persons p ON p.id=e.raw_actor JOIN persons root ON root.id=coalesce(p.current_root,p.id)
            WHERE d.space=? AND e.id<=? AND p.enabled AND root.enabled AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)) SELECT raw_actor,envelope FROM ranked WHERE n=1").bind(&space).bind(id).fetch_all(&mut *conn).await?;
        let mut names: BTreeMap<String, Option<String>> = BTreeMap::new();
        for member in members {
            let frame: Value = serde_json::from_str(member.get("envelope"))?;
            let person: String = member.try_get("raw_actor")?;
            for label in ["card", "nickname"]
                .iter()
                .filter_map(|k| frame["sender"][*k].as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let owner = names
                    .entry(label.into())
                    .or_insert_with(|| Some(person.clone()));
                if owner.as_deref() != Some(&person) {
                    *owner = None;
                }
            }
        }
        for (label, person) in names {
            if let Some(person) = person {
                aliases.insert(format!("name:{label}"), person);
            }
        }
        let mentions: Value = serde_json::from_str(row.try_get("mentions")?)?;
        for mention in mentions.as_array().into_iter().flatten() {
            if let Some(person) = mention["original"].as_str().filter(|p| *p != actor) {
                let key = format!("member_{person}");
                if aliases.insert(key.clone(), person.into()).is_none() {
                    subjects.push(json!({"subject_ref":key,"display_label":format!("Person {person}"),"allowed_scopes":["person_group"],"relation_to_speaker":"mentioned_member"}));
                }
            }
        }
        let reply = if historical {
            // Maintenance review still uses its original source version contract.
            let locator = raw["segments"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|v| v["type"] == "reply")
                .and_then(|v| v["native_id"].as_str());
            if let Some(locator) = locator {
                sqlx::query_as("SELECT e.id,e.raw_actor FROM event_locators l JOIN events e ON e.id=l.event_id WHERE e.entry_id=? AND l.receiver=? AND l.native_id=? AND e.raw_actor IS NOT NULL AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)")
                    .bind(&entry).bind(row.get::<String,_>("bot_stage")).bind(locator)
                    .fetch_optional(&mut *conn).await?
            } else {
                None
            }
        } else {
            crate::ingress::reply::group_human_reply(conn, id)
                .await?
                .map(|reply| (reply.event, reply.person))
        };
        if let Some((original, person)) = reply {
            aliases.insert("reply_author".into(), person);
            if !historical {
                reply_source = Some(original);
            }
            subjects.push(json!({"subject_ref":"reply_author","display_label":"回复消息的发言者","allowed_scopes":["person_group"],"relation_to_speaker":"reply_author"}));
        }
    }
    let zone = zone.parse::<chrono_tz::Tz>()?;
    let occurred = row
        .try_get::<Option<i64>, _>("occurred_at")?
        .and_then(DateTime::<Utc>::from_timestamp_millis)
        .map(|t| t.with_timezone(&zone).to_rfc3339());
    Ok(Some(Event {
        id,
        entry,
        space,
        actor,
        label,
        content,
        version: scope::hash(&json!([raw, speech, aliases])),
        subjects: json!(subjects),
        aliases,
        reply_source,
        occurred,
    }))
}
pub async fn subject(
    conn: &mut SqliteConnection,
    event: &Event,
    claim: &Value,
) -> Result<Option<(String, Option<String>, Option<String>)>> {
    let Some(kind) = claim["scope_type"].as_str() else {
        return Ok(None);
    };
    let reference = claim["subject_ref"].as_str().unwrap_or("");
    if kind == "group" && reference == "group" && event.space.is_some() {
        return Ok(Some((kind.into(), None, event.space.clone())));
    }
    let person = if reference == "named_member" {
        let Some(name) = claim["subject_name"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return Ok(None);
        };
        let Some(person) = event.aliases.get(&format!("name:{name}")) else {
            return Ok(None);
        };
        person.clone()
    } else if !reference.starts_with("name:")
        && let Some(person) = event.aliases.get(reference)
    {
        person.clone()
    } else {
        return Ok(None);
    };
    let allowed = match kind {
        "person" => reference == "speaker",
        "person_group" => event.space.is_some(),
        _ => false,
    };
    if !allowed {
        return Ok(None);
    }
    let enabled:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM persons p JOIN persons r ON r.id=coalesce(p.current_root,p.id) WHERE p.id=? AND p.enabled AND r.enabled)")
        .bind(&person).fetch_one(conn).await?;
    Ok(enabled.then(|| {
        (
            kind.into(),
            Some(person),
            if kind == "person_group" {
                event.space.clone()
            } else {
                None
            },
        )
    }))
}
