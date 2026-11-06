use super::scope;
use crate::{Result, tools::registry::Tool};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::sync::LazyLock;

static DEFINITIONS: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../../fixtures/contacts/v4-declarations.json"
    ))
    .expect("original contact definitions")
});
pub(crate) fn declaration(name: &str) -> Option<Tool> {
    let d = DEFINITIONS["tools"]
        .as_array()?
        .iter()
        .find(|d| d["name"] == name)?;
    Some(Tool {
        name: name.into(),
        description: d["description"].as_str()?.into(),
        schema: d["parameters"].clone(),
        parallel_read: true,
        cacheable: false,
        code_direct: false,
        provider: None,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    platform: Option<String>,
    scope: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contacts {
    kind: String,
    target_id: Option<String>,
    subject_ref: Option<String>,
    #[serde(default)]
    display_name: String,
    source: Option<Source>,
    #[serde(default = "limit")]
    limit: i64,
    cursor: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Conversations {
    target: Option<Target>,
    source: Option<Source>,
    #[serde(default)]
    display_name: String,
    #[serde(default = "limit")]
    limit: i64,
    cursor: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    conversation_id: Option<String>,
    person_id: Option<String>,
    space_id: Option<String>,
    binding_id: Option<String>,
    operation_id: Option<String>,
}
fn limit() -> i64 {
    20
}
fn bounds(limit: i64, cursor: Option<&str>, source: Option<&Source>) -> Result<()> {
    if limit < 1
        || limit == i64::MAX
        || cursor.is_some_and(|c| !uuid(c))
        || source.is_some_and(|s| {
            s.scope
                .as_deref()
                .is_some_and(|c| !matches!(c, "private" | "group"))
        })
    {
        return Err("invalid contact page or source".into());
    }
    Ok(())
}
fn uuid(v: &str) -> bool {
    v.len() == 36
        && v.bytes().enumerate().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}
pub(crate) async fn read(
    db: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    name: &str,
    args: &Value,
) -> Result<Value> {
    let s = scope(db, exec, actor).await?;
    let entries = serde_json::to_string(&s.entries)?;
    if name == "find_contacts" {
        contacts(
            db,
            exec,
            &s.subject,
            &entries,
            serde_json::from_value(args.clone())?,
        )
        .await
    } else {
        conversations(db, &entries, serde_json::from_value(args.clone())?).await
    }
}
async fn referenced(
    db: &mut SqliteConnection,
    exec: &str,
    current: &str,
    reference: &str,
) -> Result<String> {
    if reference == "current_speaker" {
        return Ok(current.into());
    }
    let r=sqlx::query("WITH RECURSIVE lineage(id) AS(SELECT ?1 UNION SELECT o.parent FROM lineage l JOIN executions x ON x.id=l.id JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL) SELECT e.* FROM lineage l JOIN executions x ON x.id=l.id JOIN events e ON e.id=x.source WHERE NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id) LIMIT 1").bind(exec).fetch_one(&mut *db).await?;
    let found: Vec<String> = if reference == "mentioned_user" {
        let m: Value = serde_json::from_str(r.get("mentions"))?;
        m.as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p["original"].as_str().map(str::to_owned))
            .collect()
    } else if reference == "replied_message_author" {
        let raw: Value = serde_json::from_str(r.get("envelope"))?;
        let id = raw["segments"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|v| v["type"] == "reply")
            .and_then(|v| v["native_id"].as_str())
            .ok_or("subject_not_found")?;
        sqlx::query_scalar("SELECT DISTINCT e.raw_actor FROM event_locators l JOIN events e ON e.id=l.event_id WHERE l.receiver=? AND l.native_id=? AND e.entry_id=? AND e.raw_actor IS NOT NULL AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id)").bind(r.get::<String,_>("bot_stage")).bind(id).bind(r.get::<String,_>("entry_id")).fetch_all(db).await?
    } else {
        return Err("subject_not_found".into());
    };
    let unique = found.into_iter().collect::<std::collections::BTreeSet<_>>();
    if unique.len() != 1 {
        return Err("subject reference absent or ambiguous".into());
    }
    Ok(unique.into_iter().next().ok_or("subject_not_found")?)
}
async fn contacts(
    db: &mut SqliteConnection,
    exec: &str,
    current: &str,
    entries: &str,
    mut q: Contacts,
) -> Result<Value> {
    bounds(q.limit, q.cursor.as_deref(), q.source.as_ref())?;
    q.cursor = q.cursor.map(|id| id.to_ascii_lowercase());
    q.target_id = q.target_id.map(|id| id.to_ascii_lowercase());
    let selectors = usize::from(q.target_id.as_ref().is_some_and(|s| !s.is_empty()))
        + usize::from(q.subject_ref.as_ref().is_some_and(|s| !s.is_empty()))
        + usize::from(!q.display_name.is_empty());
    if selectors > 1
        || !matches!(q.kind.as_str(), "person" | "space")
        || q.subject_ref.is_some() && q.kind != "person"
        || q.target_id.as_deref().is_some_and(|v| !uuid(v))
    {
        return Err("invalid contact selector".into());
    }
    let selected = if let Some(reference) = &q.subject_ref {
        Some(referenced(db, exec, current, reference).await?)
    } else {
        q.target_id
    };
    let selected = if q.kind == "person" {
        if let Some(id) = selected {
            sqlx::query_scalar::<_, String>(
                "SELECT coalesce(current_root,id) FROM persons WHERE id=? AND enabled",
            )
            .bind(id)
            .fetch_optional(&mut *db)
            .await?
            .ok_or("target_not_found")
            .map(Some)?
        } else {
            None
        }
    } else {
        selected
    };
    let platform = q.source.as_ref().and_then(|s| s.platform.as_deref());
    let scope = q.source.as_ref().and_then(|s| s.scope.as_deref());
    let sql = if q.kind == "person" {
        "SELECT p.id,p.name FROM persons p WHERE p.enabled AND p.current_root IS NULL AND (?3 IS NULL OR p.id=?3) AND (?4 IS NULL OR p.id>?4) AND (?5 IS NULL OR ?5='private') AND EXISTS(SELECT 1 FROM events e JOIN persons author ON author.id=e.raw_actor WHERE coalesce(author.current_root,author.id)=p.id AND author.enabled AND e.entry_id IN(SELECT value FROM json_each(?1)) AND json_extract(e.envelope,'$.kind')='message' AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id)) AND (instr(lower(p.name),lower(?2))>0 OR EXISTS(SELECT 1 FROM events e JOIN persons old ON old.id=e.raw_actor WHERE coalesce(old.current_root,old.id)=p.id AND e.entry_id IN(SELECT value FROM json_each(?1)) AND (instr(lower(coalesce(json_extract(e.envelope,'$.sender.nickname'),'')),lower(?2))>0 OR instr(lower(coalesce(json_extract(e.envelope,'$.sender.card'),'')),lower(?2))>0) AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id))) AND (?6 IS NULL OR EXISTS(SELECT 1 FROM assignments b JOIN persons old ON old.id=b.original_person JOIN accounts a ON a.id=b.account_id WHERE coalesce(old.current_root,old.id)=p.id AND b.status='active' AND a.platform=?6)) ORDER BY p.id LIMIT ?7"
    } else {
        "SELECT s.id,coalesce((SELECT json_extract(e.envelope,'$.group_name') FROM events e JOIN entries d ON d.id=e.entry_id WHERE (d.space=s.space_key OR lower(d.space)=s.id) AND e.entry_id IN(SELECT value FROM json_each(?1)) AND json_extract(e.envelope,'$.group_name') IS NOT NULL AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id) ORDER BY e.id DESC LIMIT 1),'') AS name FROM contact_spaces s WHERE EXISTS(SELECT 1 FROM entries d WHERE (d.space=s.space_key OR lower(d.space)=s.id) AND d.closed_at IS NULL AND d.id IN(SELECT value FROM json_each(?1))) AND instr(lower(coalesce((SELECT json_extract(e.envelope,'$.group_name') FROM events e JOIN entries d ON d.id=e.entry_id WHERE (d.space=s.space_key OR lower(d.space)=s.id) AND e.entry_id IN(SELECT value FROM json_each(?1)) AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id) ORDER BY e.id DESC LIMIT 1),'')),lower(?2))>0 AND (?3 IS NULL OR s.id=?3) AND (?4 IS NULL OR s.id>?4) AND (?5 IS NULL OR ?5='group') AND (?6 IS NULL OR EXISTS(SELECT 1 FROM contact_space_bindings b JOIN observed_space_bindings o ON o.subject=b.subject AND o.platform=b.platform AND o.namespace=b.namespace AND o.native=b.native WHERE b.subject=s.id AND b.platform=?6)) ORDER BY s.id LIMIT ?7"
    };
    let rows = sqlx::query(sql)
        .bind(entries)
        .bind(&q.display_name)
        .bind(&selected)
        .bind(&q.cursor)
        .bind(scope)
        .bind(platform)
        .bind(q.limit + 1)
        .fetch_all(&mut *db)
        .await?;
    let more = rows.len() > q.limit as usize;
    let mut items = Vec::new();
    for r in rows.into_iter().take(q.limit as usize) {
        let id = r.get::<String, _>("id");
        let bindings: Vec<(String, String)> = if q.kind == "person" {
            sqlx::query_as("SELECT b.id,a.platform FROM assignments b JOIN persons p ON p.id=b.original_person AND p.enabled JOIN accounts a ON a.id=b.account_id WHERE coalesce(p.current_root,p.id)=? AND b.status='active' AND (?2 IS NULL OR a.platform=?2) ORDER BY b.id").bind(&id).bind(platform).fetch_all(&mut *db).await?
        } else {
            sqlx::query_as("SELECT b.id,b.platform FROM contact_space_bindings b JOIN observed_space_bindings o ON o.subject=b.subject AND o.platform=b.platform AND o.namespace=b.namespace AND o.native=b.native WHERE b.subject=?1 AND (?2 IS NULL OR b.platform=?2) ORDER BY b.id").bind(&id).bind(platform).fetch_all(&mut *db).await?
        };
        items.push(json!({"kind":q.kind,"target_id":id,"display_name":r.get::<String,_>("name"),"bindings":bindings.into_iter().map(|(id,platform)|json!({"binding_id":id,"platform":platform})).collect::<Vec<_>>()}));
    }
    let next = if more {
        items.last().map(|r| r["target_id"].clone())
    } else {
        None
    };
    if selected.is_some() && !items.is_empty() {
        let mut v = items.remove(0);
        v["has_more"] = json!(more);
        v["next_cursor"] = json!(next);
        Ok(v)
    } else {
        Ok(json!({"items":items,"has_more":more,"next_cursor":next}))
    }
}
async fn conversations(
    db: &mut SqliteConnection,
    entries: &str,
    mut q: Conversations,
) -> Result<Value> {
    bounds(q.limit, q.cursor.as_deref(), q.source.as_ref())?;
    q.cursor = q.cursor.map(|id| id.to_ascii_lowercase());
    let mut filter = ("all".to_string(), None);
    if let Some(t) = q.target {
        let supplied = [
            ("conversation_id", t.conversation_id),
            ("person_id", t.person_id),
            ("space_id", t.space_id),
            ("binding_id", t.binding_id),
            ("operation_id", t.operation_id),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.map(|s| (k, s)))
        .collect::<Vec<_>>();
        let [(key, value)] = supplied.as_slice() else {
            return Err("exactly one original conversation selector required".into());
        };
        if !uuid(value) {
            return Err("invalid original ID".into());
        }
        filter = (key.to_string(), Some(value.to_ascii_lowercase()));
    }
    let sql = "SELECT d.*,coalesce(p.current_root,p.id) AS person,s.id AS space_id,coalesce(a.platform,cs.platform) AS platform,b.status AS binding_status,cs.id AS space_binding,coalesce(p.name,'') AS display_name FROM entries d LEFT JOIN assignments b ON b.id=d.actor_stage LEFT JOIN persons p ON p.id=b.original_person LEFT JOIN accounts a ON a.id=b.account_id LEFT JOIN contact_spaces s ON (s.space_key=d.space OR s.id=lower(d.space)) LEFT JOIN contact_space_bindings cs ON cs.subject=s.id WHERE d.id IN(SELECT value FROM json_each(?1)) AND (?2 IS NULL OR d.id>?2) AND instr(lower(coalesce(p.name,'')),lower(?3))>0 AND (?4 IS NULL OR coalesce(a.platform,cs.platform)=?4) AND (?5 IS NULL OR d.scope=?5) AND (?6='all' OR (?6='conversation_id' AND d.id=?7) OR (?6='person_id' AND coalesce(p.current_root,p.id)=(SELECT coalesce(current_root,id) FROM persons WHERE id=?7)) OR (?6='space_id' AND s.id=?7) OR (?6='binding_id' AND ?7 IN(b.id,cs.id)) OR (?6='operation_id' AND EXISTS(SELECT 1 FROM social_outbound o WHERE o.entry=d.id AND o.child=?7 AND NOT o.recalled AND o.body!='{}'))) ORDER BY d.id LIMIT ?8";
    let rows = sqlx::query(sql)
        .bind(entries)
        .bind(&q.cursor)
        .bind(q.display_name)
        .bind(q.source.as_ref().and_then(|s| s.platform.as_deref()))
        .bind(q.source.as_ref().and_then(|s| s.scope.as_deref()))
        .bind(filter.0)
        .bind(filter.1)
        .bind(q.limit + 1)
        .fetch_all(db)
        .await?;
    let more = rows.len() > q.limit as usize;
    let items:Vec<_>=rows.into_iter().take(q.limit as usize).map(|r|{
      let private=r.get::<&str,_>("scope")=="private";
      json!({"conversation_id":r.get::<String,_>("id"),"kind":if private{"person"}else{"space"},"target_id":if private{r.get::<Option<String>,_>("person")}else{r.get::<Option<String>,_>("space_id")},"person_id":r.get::<Option<String>,_>("person"),"space_id":r.get::<Option<String>,_>("space_id"),"binding_id":if private{r.get::<Option<String>,_>("actor_stage")}else{r.get::<Option<String>,_>("space_binding")},"entry_presence_id":r.get::<Option<String>,_>("bot_stage"),"thread_id":r.get::<Option<String>,_>("topic"),"platform":r.get::<Option<String>,_>("platform"),"scope":r.get::<String,_>("scope"),"display_name":r.get::<String,_>("display_name"),"binding_status":r.get::<Option<String>,_>("binding_status"),"presence_status":null})
    }).collect();
    Ok(
        json!({"next_cursor":if more{items.last().map(|r|r["conversation_id"].clone())}else{None},"has_more":more,"items":items}),
    )
}

pub(crate) async fn bind_sources(
    db: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    value: &Value,
) -> Result<()> {
    let original = scope(db, exec, actor).await?;
    let items = if let Some(items) = value["items"].as_array() {
        items.clone()
    } else {
        vec![value.clone()]
    };
    let mut people = std::collections::BTreeSet::new();
    let mut entries = std::collections::BTreeSet::new();
    let mut spaces = std::collections::BTreeSet::new();
    for item in items {
        if item["kind"] == "person"
            && let Some(p) = item["target_id"].as_str()
        {
            people.insert(p.to_owned());
        }
        if let Some(id) = item["conversation_id"].as_str() {
            entries.insert(id.to_owned());
        }
        if item["kind"] == "space"
            && let Some(id) = item["target_id"].as_str()
        {
            spaces.insert(id.to_owned());
        }
    }
    sqlx::query("INSERT OR IGNORE INTO main_dependencies(execution,event)
      SELECT ?1,max(e.id) FROM events e LEFT JOIN persons p ON p.id=e.raw_actor JOIN entries d ON d.id=e.entry_id LEFT JOIN contact_spaces s ON (s.space_key=d.space OR s.id=lower(d.space))
      WHERE e.entry_id IN(SELECT value FROM json_each(?2)) AND json_extract(e.envelope,'$.kind')='message'
      AND (coalesce(p.current_root,p.id) IN(SELECT value FROM json_each(?3)) OR d.id IN(SELECT value FROM json_each(?4)) OR s.id IN(SELECT value FROM json_each(?5)))
      AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id)
      GROUP BY coalesce(p.current_root,p.id),e.entry_id")
      .bind(exec).bind(serde_json::to_string(&original.entries)?).bind(serde_json::to_string(&people)?).bind(serde_json::to_string(&entries)?).bind(serde_json::to_string(&spaces)?).execute(db).await?;
    Ok(())
}
