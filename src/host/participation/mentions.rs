use crate::{Result, social::native_content::clean_name};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

const PAGE_SIZE: i64 = 16;

#[derive(Serialize)]
struct Mention {
    kind: &'static str,
    display_name: String,
    account_id: Option<String>,
    assignment_id: Option<String>,
    person_id: Option<String>,
    presence_id: Option<String>,
    text_start: Option<i64>,
    text_end: Option<i64>,
}

pub(super) async fn page(db: &mut SqliteConnection, event: i64, cursor: i64) -> Result<Value> {
    let count: i64 =
        sqlx::query_scalar("SELECT json_array_length(mentions) FROM events WHERE id=?")
            .bind(event)
            .fetch_one(&mut *db)
            .await?;
    if cursor > count {
        return Err("invalid mention cursor".into());
    }
    let rows = sqlx::query(
        "SELECT m.value,account.id AS account,stage.id AS assignment,
                stage.original_person AS person,stage.kind='self' AS self
         FROM events e JOIN accounts author ON author.id=e.account_id
         JOIN json_each(e.mentions) m
         LEFT JOIN accounts account ON account.id=json_extract(m.value,'$.account')
           AND account.platform=author.platform AND account.namespace=author.namespace
         LEFT JOIN assignments stage ON stage.id=json_extract(m.value,'$.assignment')
           AND stage.account_id=account.id AND stage.original_person IS json_extract(m.value,'$.original')
         WHERE e.id=? ORDER BY m.key LIMIT ? OFFSET ?",
    )
    .bind(event)
    .bind(PAGE_SIZE)
    .bind(cursor)
    .fetch_all(db)
    .await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let stored: Value = serde_json::from_str(row.try_get("value")?)?;
        let account_id: Option<String> = row.try_get("account")?;
        let assignment_id: Option<String> = row.try_get("assignment")?;
        let is_self = row.try_get::<Option<bool>, _>("self")? == Some(true);
        let kind = match (is_self, stored["native"] == "all" && account_id.is_none()) {
            (true, _) => "yuki",
            (false, true) => "all",
            _ => "member",
        };
        items.push(Mention {
            kind,
            display_name: clean_name(stored["label"].as_str().unwrap_or_default()),
            account_id,
            person_id: row.try_get("person")?,
            presence_id: if is_self { assignment_id.clone() } else { None },
            assignment_id,
            text_start: stored["text_start"].as_i64(),
            text_end: stored["text_end"].as_i64(),
        });
    }
    let next = cursor + items.len() as i64;
    Ok(json!({"items":items,"next":next,"complete":next==count}))
}
