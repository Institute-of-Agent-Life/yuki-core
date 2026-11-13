//! Keep original account annotations bound to the planner and each physical text range.
use crate::{
    Result,
    privacy::source,
    social::native_content::{self, Mention},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use std::collections::BTreeSet;

pub(super) fn project(params: &mut Value) -> Result<()> {
    let Some(mentions) = params["arguments"].get("mentions") else {
        return Ok(());
    };
    let bytes = serde_json::to_vec(mentions)?;
    let descriptor = json!({"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))});
    params["arguments"]
        .as_object_mut()
        .ok_or("missing gateway arguments")?
        .remove("mentions");
    params["arguments"]["mentions_source"] = descriptor;
    Ok(())
}

pub(super) fn validate(parts: &[Value], mentions: &[Mention]) -> Result<()> {
    if parts
        .iter()
        .any(|part| part.get("mentions").is_some() || part.get("mentions_source").is_some())
    {
        return Err("plugin cannot replace original account annotations".into());
    }
    if mentions.is_empty() {
        return Ok(());
    }
    for part in parts {
        if part["preparation"] == true || part.get("prepared_part").is_some() {
            continue;
        }
        let start = part["text_range"]["start"]
            .as_u64()
            .ok_or("original mention plan requires text ranges")? as usize;
        let length = part["text_range"]["length"]
            .as_u64()
            .ok_or("missing original text range")? as usize;
        let end = start.checked_add(length).ok_or("mention range overflow")?;
        if mentions
            .iter()
            .any(|m| (m.start < start && start < m.end) || (m.start < end && end < m.end))
        {
            return Err("text range cuts an original mention".into());
        }
    }
    Ok(())
}

pub(super) fn attach(args: &mut Value, mentions: &[Mention]) -> Result<()> {
    let Some(range) = args["part"].get("text_range") else {
        return Ok(());
    };
    let start = range["start"]
        .as_u64()
        .ok_or("missing original text start")? as usize;
    let length = range["length"]
        .as_u64()
        .ok_or("missing original text length")? as usize;
    let end = start.checked_add(length).ok_or("mention range overflow")?;
    let selected: Vec<_> = mentions
        .iter()
        .filter(|m| m.start >= start && m.end <= end)
        .map(|m| {
            let mut local = m.clone();
            local.start -= start;
            local.end -= start;
            local
        })
        .collect();
    if !selected.is_empty() {
        args["mentions"] = serde_json::to_value(selected)?;
    }
    Ok(())
}

pub(super) async fn check(db: &mut SqliteConnection, plugin: &str, args: &Value) -> Result<()> {
    native_content::check(db, plugin, &native_content::decode(args)?).await
}

pub(super) async fn hold(
    db: &mut SqliteConnection,
    plugin: &str,
    handle: &str,
    execution: &str,
    args: &Value,
) -> Result<()> {
    let mentions = native_content::decode(args)?;
    let sources: BTreeSet<_> = mentions.iter().map(|m| m.source).collect();
    for event in sources {
        source::acquire(db, plugin, handle, event).await?;
        sqlx::query("INSERT OR IGNORE INTO main_dependencies(execution,event) VALUES(?,?)")
            .bind(execution)
            .bind(event)
            .execute(&mut *db)
            .await?;
    }
    let identities: BTreeSet<_> = mentions
        .iter()
        .map(|m| (&m.account, &m.native, &m.assignment, m.source))
        .collect();
    sqlx::query(
        "UPDATE host_calls SET args=json_set(args,'$._mention_identities',json(?)) WHERE handle=?",
    )
    .bind(serde_json::to_string(&identities)?)
    .bind(handle)
    .execute(db)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn original() -> Mention {
        Mention {
            start: 3,
            end: 10,
            account: "original".into(),
            native: "opaque".into(),
            label: "小雪".into(),
            assignment: Some("original-stage".into()),
            source: 3,
        }
    }
    #[test]
    fn physical_parts_preserve_whole_original_mentions() {
        let mentions = vec![original()];
        for boundary in 4..10 {
            assert!(
                validate(
                    &[json!({"text_range":{"start":0,"length":boundary}})],
                    &mentions
                )
                .is_err()
            );
        }
        assert!(validate(&[json!({"text":"雪@小雪"})], &mentions).is_err());
        assert!(
            validate(
                &[json!({"text_range":{"start":0,"length":10},"mentions":[]})],
                &mentions
            )
            .is_err()
        );
        let parts = vec![
            json!({"text_range":{"start":0,"length":3}}),
            json!({"text_range":{"start":3,"length":7}}),
        ];
        validate(&parts, &mentions).unwrap();
        let mut args = json!({"part":parts[1]});
        attach(&mut args, &mentions).unwrap();
        let mut expected = original();
        expected.start = 0;
        expected.end = 7;
        assert_eq!(native_content::decode(&args).unwrap(), vec![expected]);
        let mut params = json!({"arguments":args});
        let bytes = serde_json::to_vec(&params["arguments"]["mentions"]).unwrap();
        project(&mut params).unwrap();
        assert!(params["arguments"].get("mentions").is_none());
        assert_eq!(params["arguments"]["mentions_source"]["bytes"], bytes.len());
        assert_eq!(
            params["arguments"]["mentions_source"]["sha256"],
            format!("{:x}", Sha256::digest(&bytes))
        );
    }
}
