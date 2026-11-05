//! Observed account directory; reading it never acquires a sending route or calls QQ.
use crate::{Result, runtime::effects, tools::registry::Tool};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::sync::LazyLock;

fn display_name(value: &str) -> String {
    static INVISIBLE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"\p{C}").unwrap());
    INVISIBLE
        .replace_all(value, "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(128)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

pub(crate) fn declaration() -> Tool {
    let source: Value =
        serde_json::from_str(include_str!("../../../fixtures/social/v4-members.json"))
            .expect("fixed original member declaration");
    let d = &source["declarations"][0];
    Tool {
        name: "get_group_members".into(),
        description: d["description"].as_str().unwrap().into(),
        schema: d["parameters"].clone(),
        parallel_read: true,
        cacheable: false,
        code_direct: false,
        provider: None,
    }
}
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Request {
    target_id: Option<String>,
    display_name: Option<String>,
    binding_id: Option<String>,
    conversation_id: Option<String>,
    cursor: Option<String>,
    limit: i64,
}
impl Default for Request {
    fn default() -> Self {
        Self {
            target_id: None,
            display_name: None,
            binding_id: None,
            conversation_id: None,
            cursor: None,
            limit: 20,
        }
    }
}

pub(crate) async fn scope(
    db: &mut SqliteConnection,
    exec: &str,
    actor: &str,
) -> Result<super::Scope> {
    effects::authorize(db, exec, actor, false).await?;
    if let Some((origin, entry, _)) = crate::host::participation::self_binding(db, exec).await? {
        return Ok(super::Scope {
            home: entry.clone(),
            entries: vec![entry],
            subject: origin,
        });
    }
    let registered: Option<(String,String)>=sqlx::query_as("WITH RECURSIVE family(id) AS(SELECT ?1 UNION SELECT o.parent FROM family f JOIN executions x ON x.id=f.id JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL) SELECT o.ref,o.conversation FROM family f JOIN executions x ON x.id=f.id JOIN main_origins o ON o.ref=x.origin AND o.kind='self' JOIN entries e ON e.id=o.conversation AND e.scope='group' LIMIT 1")
        .bind(exec).fetch_optional(&mut *db).await?;
    if let Some((origin, entry)) = registered {
        return Ok(super::Scope {
            home: entry.clone(),
            entries: vec![entry],
            subject: origin,
        });
    }
    super::scope(db, exec, actor).await
}

pub(crate) async fn read(
    db: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    args: &Value,
    bind: bool,
) -> Result<Value> {
    let mut q: Request = serde_json::from_value(args.clone())?;
    if !(1..=100).contains(&q.limit) {
        return Err("invalid group directory limit".into());
    }
    for value in [
        &mut q.target_id,
        &mut q.binding_id,
        &mut q.conversation_id,
        &mut q.cursor,
    ]
    .into_iter()
    .flatten()
    {
        if !super::contacts::uuid(value) {
            return Err("invalid group directory identifier".into());
        }
        *value = value.to_ascii_lowercase();
    }
    if q.display_name
        .as_ref()
        .is_some_and(|v| v.chars().count() > 128)
    {
        return Err("invalid group name".into());
    }
    if q.target_id.is_some() && q.display_name.as_ref().is_some_and(|v| !v.is_empty()) {
        return Err("target_selector_conflict".into());
    }
    let s = scope(db, exec, actor).await?;
    let entries = serde_json::to_string(&s.entries)?;
    let selected = if let Some(id) = &q.target_id {
        id.clone()
    } else if let Some(name) = q.display_name.as_ref().filter(|v| !v.is_empty()) {
        let ids:Vec<String>=sqlx::query_scalar("SELECT DISTINCT s.id FROM contact_spaces s JOIN entries d ON (d.space=s.space_key OR lower(d.space)=s.id) WHERE d.id IN(SELECT value FROM json_each(?1)) AND coalesce((SELECT json_extract(e.envelope,'$.group_name') FROM events e WHERE e.entry_id=d.id AND json_extract(e.envelope,'$.group_name') IS NOT NULL AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id) ORDER BY e.id DESC LIMIT 1),'')=?2 ORDER BY s.id")
            .bind(&entries).bind(name).fetch_all(&mut *db).await?;
        if ids.len() != 1 {
            return Err(if ids.is_empty() {
                "target_not_found"
            } else {
                "target_ambiguous"
            }
            .into());
        }
        ids[0].clone()
    } else {
        sqlx::query_scalar::<_,String>("SELECT s.id FROM entries d JOIN contact_spaces s ON (d.space=s.space_key OR lower(d.space)=s.id) WHERE d.id=? AND d.scope='group'")
            .bind(&s.home).fetch_optional(&mut *db).await?.ok_or("target_selector_conflict")?
    };
    let permitted:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM entries d JOIN contact_spaces s ON (d.space=s.space_key OR lower(d.space)=s.id) WHERE s.id=?1 AND d.id IN(SELECT value FROM json_each(?2)))")
        .bind(&selected).bind(&entries).fetch_one(&mut *db).await?;
    if !permitted {
        return Err("group directory outside original scope".into());
    }
    let conversation = q.conversation_id.as_deref().unwrap_or(&s.home);
    if !s.entries.iter().any(|e| e == conversation) {
        return Err("conversation outside original scope".into());
    }
    let binding = if let Some(id) = q.binding_id {
        Some(id)
    } else {
        sqlx::query_scalar::<_,String>("SELECT b.id FROM entries d JOIN contact_spaces s ON (s.space_key=d.space OR s.id=lower(d.space)) JOIN contact_space_bindings b ON b.subject=s.id WHERE d.id=?1 AND s.id=?2 AND json_valid(d.space) AND b.platform=json_extract(CASE WHEN json_valid(d.space) THEN d.space ELSE '[]' END,'$[0]') AND b.namespace=json_extract(CASE WHEN json_valid(d.space) THEN d.space ELSE '[]' END,'$[1]') AND b.native=json_extract(CASE WHEN json_valid(d.space) THEN d.space ELSE '[]' END,'$[2]')")
            .bind(conversation).bind(&selected).fetch_optional(&mut *db).await?
    };
    let bindings=sqlx::query("SELECT b.* FROM contact_space_bindings b JOIN observed_space_bindings o ON (o.subject=b.subject AND o.platform=b.platform AND o.namespace=b.namespace AND o.native=b.native) WHERE b.subject=?1 AND (?2 IS NULL OR b.id=?2) ORDER BY b.id")
        .bind(&selected).bind(&binding).fetch_all(&mut *db).await?;
    if bindings.len() != 1 {
        return Ok(
            json!({"error":if bindings.is_empty(){"space_entry_unavailable"}else{"space_entry_ambiguous"},"bindings":bindings.iter().map(|r|json!({"binding_id":r.get::<String,_>("id"),"platform":r.get::<String,_>("platform"),"display_name":""})).collect::<Vec<_>>()}),
        );
    }
    let b = &bindings[0];
    // Only original non-erased messages in the selected authorized binding can
    // introduce an account. Mentions do not manufacture a Person or assignment.
    let mut rows=sqlx::query(r#"
        WITH messages AS (
          SELECT e.id,e.account_id,e.mentions,coalesce(e.occurred_at,e.observed_at) AS moment,
                 json_extract(e.envelope,'$.sender.card') AS card,
                 nullif(json_extract(e.envelope,'$.sender.nickname'),'') AS nickname
          FROM events e JOIN entries d ON d.id=e.entry_id JOIN accounts a ON a.id=e.account_id
          WHERE d.id IN(SELECT value FROM json_each(?1))
            AND json_extract(CASE WHEN json_valid(d.space) THEN d.space ELSE '[]' END,'$[0]')=?2
            AND json_extract(CASE WHEN json_valid(d.space) THEN d.space ELSE '[]' END,'$[1]')=?3
            AND json_extract(CASE WHEN json_valid(d.space) THEN d.space ELSE '[]' END,'$[2]')=?4
            AND a.platform=?2 AND a.namespace=?3 AND json_extract(e.envelope,'$.kind')='message'
            AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id)
        ), observed AS (
          SELECT id,account_id,moment,card,nickname FROM messages
          UNION ALL
          SELECT m.id,a.id,m.moment,NULL,NULL FROM messages m,json_each(m.mentions) j
          JOIN accounts a ON a.id=json_extract(j.value,'$.account') AND a.platform=?2 AND a.namespace=?3
        ), page AS (
          SELECT DISTINCT account_id FROM observed WHERE (?5 IS NULL OR account_id>?5)
          ORDER BY account_id LIMIT ?6
        )
        SELECT p.account_id,a.id AS assignment_id,a.kind,coalesce(w.current_root,w.id) AS person,
          (SELECT id FROM observed o WHERE o.account_id=p.account_id ORDER BY moment DESC,id DESC LIMIT 1) AS observation_source,
          (SELECT card FROM observed o WHERE o.account_id=p.account_id AND card IS NOT NULL ORDER BY moment DESC,id DESC LIMIT 1) AS card,
          (SELECT id FROM observed o WHERE o.account_id=p.account_id AND card IS NOT NULL ORDER BY moment DESC,id DESC LIMIT 1) AS card_source,
          (SELECT nickname FROM observed o WHERE o.account_id=p.account_id AND nickname IS NOT NULL ORDER BY moment DESC,id DESC LIMIT 1) AS nickname,
          (SELECT id FROM observed o WHERE o.account_id=p.account_id AND nickname IS NOT NULL ORDER BY moment DESC,id DESC LIMIT 1) AS nickname_source
        FROM page p LEFT JOIN assignments a ON a.account_id=p.account_id AND a.status!='ended'
        LEFT JOIN persons w ON w.id=a.original_person ORDER BY p.account_id
    "#).bind(&entries).bind(b.get::<&str,_>("platform")).bind(b.get::<&str,_>("namespace"))
        .bind(b.get::<&str,_>("native")).bind(&q.cursor).bind(q.limit+1).fetch_all(&mut *db).await?;
    let more = rows.len() > q.limit as usize;
    rows.truncate(q.limit as usize);
    let next = more.then(|| rows.last().unwrap().get::<String, _>("account_id"));
    let mut items = vec![];
    for row in rows {
        let account: String = row.get("account_id");
        let assignment: Option<String> = row.get("assignment_id");
        let presence = (row.get::<Option<&str>, _>("kind") == Some("self"))
            .then(|| assignment.clone())
            .flatten();
        let card: Option<String> = row
            .get::<Option<String>, _>("card")
            .map(|v| display_name(&v));
        let name = card
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                row.get::<Option<String>, _>("nickname")
                    .map(|v| display_name(&v))
                    .filter(|v| !v.is_empty())
            })
            .unwrap_or_else(|| "未识别成员".into());
        items.push(json!({"account_id":account,"reference":format!("@{{account:{account}}}"),"display_name":name,"assignment_id":assignment,"person_id":row.get::<Option<String>,_>("person"),"presence_id":presence}));
        if bind {
            let mut sources = vec![
                row.get::<Option<i64>, _>("observation_source"),
                row.get("card_source"),
            ];
            if card.is_none_or(|s| s.is_empty()) {
                sources.push(row.get("nickname_source"));
            }
            for event in sources.into_iter().flatten() {
                sqlx::query("INSERT OR IGNORE INTO main_dependencies(execution,event) VALUES(?,?)")
                    .bind(exec)
                    .bind(event)
                    .execute(&mut *db)
                    .await?;
            }
        }
    }
    Ok(
        json!({"items":items,"coverage":"observed","binding_id":b.get::<String,_>("id"),"next_cursor":next}),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn original_display_names_remain_one_line_and_bounded() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/social/v4-members.json")).unwrap();
        for sample in fixture["sanitized_names"].as_array().unwrap() {
            assert_eq!(
                super::display_name(sample["input"].as_str().unwrap()),
                sample["output"].as_str().unwrap()
            );
        }
    }
}
