//! Prepare platform-native mentions from original core accounts and observed source names.
use crate::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Row, SqliteConnection};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Mention {
    pub start: usize,
    pub end: usize,
    pub account: String,
    pub native: String,
    pub label: String,
    pub assignment: Option<String>,
    pub source: i64,
}

#[derive(Clone)]
struct Target {
    account: String,
    native: String,
    label: String,
    assignment: Option<String>,
    source: i64,
}
const DISPLAY_NAME_LIMIT: usize = 128;

struct Position {
    at: usize,
    name: usize,
    end: Option<usize>,
    account: Option<String>,
    escaped: bool,
}
fn positions(text: &str) -> Vec<Position> {
    static ADDRESS: OnceLock<Regex> = OnceLock::new();
    let address =
        ADDRESS.get_or_init(|| Regex::new(r"https?://\S+|[\w.+-]+@[\w.-]+\.[a-zA-Z]{2,}").unwrap());
    let mut addresses = address.find_iter(text).peekable();
    let mut result = Vec::new();
    let mut cursor = 0;
    while let Some(at) = text[cursor..].find('@').map(|offset| cursor + offset) {
        while addresses.peek().is_some_and(|span| span.end() <= at) {
            addresses.next();
        }
        if let Some(span) = addresses.peek().filter(|span| span.start() <= at) {
            cursor = span.end();
            continue;
        }
        cursor = at + 1;
        let escaped = text[..at].ends_with('\\');
        let mut item = Position {
            at,
            name: cursor,
            end: None,
            account: None,
            escaped,
        };
        if !escaped {
            if let Some((id, _)) = text[cursor..]
                .strip_prefix("{account:")
                .and_then(|tail| tail.split_once('}'))
            {
                item.end = Some(cursor + "{account:".len() + id.len() + 1);
                let compact = id.replace("urn:", "").replace("uuid:", "").replace('-', "");
                if compact.len() == 32 && compact.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    item.account = Some(
                        format!(
                            "{}-{}-{}-{}-{}",
                            &compact[..8],
                            &compact[8..12],
                            &compact[12..16],
                            &compact[16..20],
                            &compact[20..]
                        )
                        .to_ascii_lowercase(),
                    );
                }
                cursor = item.end.unwrap();
            } else {
                for ch in text[cursor..].chars().take_while(|ch| {
                    ch.is_whitespace()
                        && !matches!(
                            ch,
                            '\r' | '\n' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}'
                        )
                }) {
                    item.name += ch.len_utf8();
                }
            }
        }
        result.push(item);
    }
    result
}
fn clean_name(name: &str) -> String {
    static INVISIBLE: OnceLock<Regex> = OnceLock::new();
    let invisible = INVISIBLE.get_or_init(|| Regex::new(r"\p{C}").unwrap());
    invisible
        .replace_all(name, "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(DISPLAY_NAME_LIMIT)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

fn render(
    text: &str,
    positions: &[Position],
    names: &BTreeMap<String, Target>,
    accounts: &BTreeMap<String, Target>,
) -> (String, Vec<Mention>) {
    let mut body = String::new();
    let mut mentions = Vec::new();
    let mut cursor = 0;
    for item in positions {
        if item.at < cursor {
            continue;
        }
        body.push_str(&text[cursor..if item.escaped { item.at - 1 } else { item.at }]);
        if item.escaped {
            body.push('@');
            cursor = item.at + 1;
            continue;
        }
        let found = if let Some(end) = item.end {
            item.account
                .as_ref()
                .and_then(|id| accounts.get(id))
                .map(|target| (target, end))
        } else {
            let mut prefix = String::new();
            let mut found = None;
            for ch in text[item.name..].chars().take(DISPLAY_NAME_LIMIT) {
                prefix.push(ch);
                if let Some(target) = names.get(&prefix) {
                    found = Some((target, item.name + prefix.len()));
                }
            }
            found
        };
        if let Some((target, end)) = found {
            let start = body.len();
            if item.end.is_some() {
                body.push('@');
                body.push_str(&target.label);
            } else {
                body.push_str(&text[item.at..end]);
            }
            mentions.push(Mention {
                start,
                end: body.len(),
                account: target.account.clone(),
                native: target.native.clone(),
                label: target.label.clone(),
                assignment: target.assignment.clone(),
                source: target.source,
            });
            cursor = end;
        } else {
            cursor = item.end.unwrap_or(item.at + 1);
            body.push_str(&text[item.at..cursor]);
        }
    }
    body.push_str(&text[cursor..]);
    (body, mentions)
}

pub(crate) async fn prepare(
    db: &mut SqliteConnection,
    plugin: &str,
    entry: &str,
    execution: &str,
    text: &str,
) -> Result<(String, Vec<Mention>)> {
    let positions = positions(text);
    let mut requested_names = BTreeSet::new();
    let mut requested_accounts = BTreeSet::new();
    for item in &positions {
        if item.escaped {
            continue;
        }
        if let Some(id) = &item.account {
            requested_accounts.insert(id.clone());
        }
        if item.end.is_none() {
            let mut prefix = String::new();
            for ch in text[item.name..].chars().take(DISPLAY_NAME_LIMIT) {
                prefix.push(ch);
                requested_names.insert(prefix.clone());
            }
        }
    }
    if requested_names.is_empty() && requested_accounts.is_empty() {
        return Ok(render(text, &positions, &BTreeMap::new(), &BTreeMap::new()));
    }
    // The immutable event facts also cover data created before this reader was added.
    // Match physical entries within the same receiving stage and original Space.
    let rows = sqlx::query(
        "WITH observed AS (
          SELECT e.id AS source,e.account_id AS account,e.actor_stage AS assignment,j.value AS name,0 AS mention
          FROM events e JOIN entries source ON source.id=e.entry_id JOIN entries target ON target.id=?1
          JOIN json_each(json_array(json_extract(e.envelope,'$.sender.nickname'),json_extract(e.envelope,'$.sender.card'))) j
          WHERE e.bot_stage=(SELECT bot_stage FROM host_installations WHERE plugin=?2) AND (source.id=target.id OR source.space IS NOT NULL AND source.space=target.space)
          UNION ALL
          SELECT e.id,json_extract(j.value,'$.account'),json_extract(j.value,'$.assignment'),json_extract(j.value,'$.label'),1
          FROM events e JOIN entries source ON source.id=e.entry_id JOIN entries target ON target.id=?1 JOIN json_each(e.mentions) j
          WHERE e.bot_stage=(SELECT bot_stage FROM host_installations WHERE plugin=?2) AND (source.id=target.id OR source.space IS NOT NULL AND source.space=target.space)
        )
        SELECT o.*,a.native,
          (SELECT coalesce(recent.occurred_at,recent.observed_at) FROM events recent WHERE recent.entry_id=?1 AND recent.account_id=o.account
           AND json_extract(recent.envelope,'$.kind')='message' AND NOT EXISTS(SELECT 1 FROM invalidated_sources erased WHERE erased.source=recent.id)
           ORDER BY coalesce(recent.occurred_at,recent.observed_at) DESC,recent.id DESC LIMIT 1) AS recent_time,
          (SELECT recent.id FROM events recent WHERE recent.entry_id=?1 AND recent.account_id=o.account
           AND json_extract(recent.envelope,'$.kind')='message' AND NOT EXISTS(SELECT 1 FROM invalidated_sources erased WHERE erased.source=recent.id) ORDER BY coalesce(recent.occurred_at,recent.observed_at) DESC,recent.id DESC LIMIT 1) AS recent
        FROM observed o JOIN entries route_scope ON route_scope.id=?1 JOIN accounts a ON a.id=o.account JOIN host_installations i ON i.plugin=?2 AND i.platform=a.platform AND i.namespace=a.namespace
        JOIN events original ON original.id=o.source LEFT JOIN assignments actor ON actor.id=original.actor_stage
        LEFT JOIN assignments stage ON stage.id=o.assignment
        WHERE length(o.name)>0
          AND (route_scope.space IS NOT NULL OR o.account IN(SELECT account_id FROM assignments WHERE id IN(route_scope.actor_stage,route_scope.bot_stage)))
          AND json_extract(original.envelope,'$.kind')='message'
          AND (o.assignment IS NULL OR stage.status='active') AND (original.actor_stage IS NULL OR actor.status='active')
          AND NOT EXISTS(SELECT 1 FROM invalidated_sources erased WHERE erased.source=o.source)
        ORDER BY recent_time DESC,recent DESC,o.account,o.source DESC")
        .bind(entry).bind(plugin).fetch_all(&mut *db).await?;
    let mut names = BTreeMap::new();
    let mut observed_accounts = BTreeMap::new();
    for row in rows {
        let observed: &str = row.get("name");
        let name = if row.get::<bool, _>("mention") {
            observed.strip_prefix('@').unwrap_or(observed)
        } else {
            observed
        };
        let label = clean_name(name);
        let account: String = row.get("account");
        if label.is_empty()
            || (!requested_names.contains(&label) && !requested_accounts.contains(&account))
        {
            continue;
        }
        let target = Target {
            account,
            native: row.get("native"),
            label: label.clone(),
            assignment: row.get("assignment"),
            source: row.get("source"),
        };
        observed_accounts
            .entry(target.account.clone())
            .or_insert_with(|| target.clone());
        names.entry(label).or_insert(target);
    }
    let mut accounts = BTreeMap::new();
    if !requested_accounts.is_empty() {
        let rows = sqlx::query(
            "SELECT a.id,a.native,s.id AS assignment,coalesce(x.source,(SELECT min(event) FROM main_dependencies WHERE execution=x.id)) AS source
             FROM json_each(?1) requested JOIN accounts a ON a.id=requested.value
             JOIN host_installations i ON i.plugin=?2 AND i.platform=a.platform AND i.namespace=a.namespace
             JOIN executions x ON x.id=?3 LEFT JOIN assignments s ON s.account_id=a.id AND s.status='active'")
            .bind(serde_json::to_string(&requested_accounts)?).bind(plugin).bind(execution).fetch_all(&mut *db).await?;
        for row in rows {
            let account: String = row.get("id");
            let selected = observed_accounts.get(&account).cloned();
            accounts.insert(
                account.clone(),
                selected.unwrap_or(Target {
                    account,
                    native: row.get("native"),
                    label: "未识别成员".into(),
                    assignment: row.get("assignment"),
                    source: row.try_get("source")?,
                }),
            );
        }
    }
    Ok(render(text, &positions, &names, &accounts))
}

pub(crate) async fn check(
    db: &mut SqliteConnection,
    plugin: &str,
    mentions: &[Mention],
) -> Result<()> {
    let identities: BTreeSet<_> = mentions
        .iter()
        .map(|m| (&m.account, &m.native, &m.assignment, m.source))
        .collect();
    for (account, native, assignment, source) in identities {
        check_identity(db, plugin, account, native, assignment.as_deref(), source).await?;
    }
    Ok(())
}

pub(crate) async fn check_page(
    db: &mut SqliteConnection,
    plugin: &str,
    annotations: &[u8],
) -> Result<()> {
    // Call admission validated the full annotations. Each page rechecks their
    // distinct identities without allocating every repeated label and range.
    let identities: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT json_extract(value,'$.account','$.native','$.assignment','$.source')
         FROM json_each(?)",
    )
    .bind(annotations)
    .fetch_all(&mut *db)
    .await?;
    for identity in identities {
        let (account, native, assignment, source): (String, String, Option<String>, i64) =
            serde_json::from_str(&identity)?;
        check_identity(db, plugin, &account, &native, assignment.as_deref(), source).await?;
    }
    Ok(())
}

async fn check_identity(
    db: &mut SqliteConnection,
    plugin: &str,
    account: &str,
    native: &str,
    assignment: Option<&str>,
    source: i64,
) -> Result<()> {
    let valid: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM accounts a JOIN host_installations i ON i.plugin=?1 AND i.platform=a.platform AND i.namespace=a.namespace
             JOIN events e ON e.id=?5 LEFT JOIN assignments actor ON actor.id=e.actor_stage
             WHERE a.id=?2 AND a.native=?3 AND (e.actor_stage IS NULL OR actor.status='active')
             AND (?4 IS NULL AND NOT EXISTS(SELECT 1 FROM assignments s WHERE s.account_id=a.id AND s.status='active')
                  OR EXISTS(SELECT 1 FROM assignments s WHERE s.id=?4 AND s.account_id=a.id AND s.status='active'))
             AND NOT EXISTS(SELECT 1 FROM invalidated_sources erased WHERE erased.source=e.id))")
            .bind(plugin).bind(account).bind(native).bind(assignment).bind(source).fetch_one(&mut *db).await?;
    if !valid {
        return Err("original mention account, stage or source changed".into());
    }
    Ok(())
}

pub(crate) fn decode(args: &Value) -> Result<Vec<Mention>> {
    Ok(args
        .get("mentions")
        .map(|value| serde_json::from_value(value.clone()))
        .transpose()?
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_legacy_mentions_preserve_rendered_text_and_utf8_ranges() {
        let oracle: Value = serde_json::from_str(include_str!(
            "../../fixtures/social/v4-native-mentions.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["baseline"],
            "329ca7c487414d2f54ddd98ff4727446ed2446cd"
        );
        let first = Target {
            account: "00000000-0000-4000-8000-000000000001".into(),
            native: "ou_original".into(),
            label: "小雪".into(),
            assignment: Some("stage-original".into()),
            source: 1,
        };
        let second = Target {
            account: "00000000-0000-4000-8000-000000000002".into(),
            native: "ou_second".into(),
            label: "小雪子".into(),
            assignment: Some("stage-second".into()),
            source: 2,
        };
        let names = BTreeMap::from([
            (first.label.clone(), first.clone()),
            (second.label.clone(), second),
        ]);
        let accounts = BTreeMap::from([(first.account.clone(), first)]);
        for case in oracle["cases"].as_array().unwrap() {
            let raw = case["input"].as_str().unwrap();
            let (body, mentions) = render(raw, &positions(raw), &names, &accounts);
            assert_eq!(body, case["text"], "{raw}");
            assert_eq!(
                serde_json::to_value(mentions).unwrap(),
                case["mentions"],
                "{raw}"
            );
        }
        for case in oracle["sanitized_names"].as_array().unwrap() {
            assert_eq!(clean_name(case["input"].as_str().unwrap()), case["result"]);
        }
    }
}
