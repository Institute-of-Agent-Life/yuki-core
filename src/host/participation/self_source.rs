use super::{Change, Policy, sources};
use crate::Result;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

async fn record(db: &mut SqliteConnection, policy: &Policy, event: i64) -> Result<Option<Value>> {
    let Some(row) = sqlx::query(
        "SELECT ack.entry,ack.stage,ack.body,ack.namespace,t.window,ack.observed_at
         FROM personal_live_deliveries ack
         JOIN entries e ON e.id=ack.entry AND e.scope='group' AND e.closed_at IS NULL
         JOIN main_turns t ON t.execution=ack.namespace AND t.conversation=e.id
         JOIN main_windows w ON w.conversation=e.id AND w.generation=t.window
         JOIN assignments b ON b.id=ack.stage AND b.kind='self' AND b.status='active'
         WHERE ack.event=? AND NOT EXISTS(
           SELECT 1 FROM main_dependencies dep JOIN participation_erased_mentions m ON m.event=dep.event
           WHERE dep.execution=ack.namespace)
         AND NOT EXISTS(SELECT 1 FROM executions x JOIN participation_erased_mentions m ON m.event=x.source
           WHERE x.id=ack.namespace)",
    )
    .bind(event)
    .fetch_optional(&mut *db)
    .await?
    else {
        return Ok(None);
    };
    let entry: &str = row.try_get("entry")?;
    sources::entry(db, policy, entry).await?;
    let execution: &str = row.try_get("namespace")?;
    if crate::privacy::source::execution_erased(db, execution, None).await? {
        return Ok(None);
    }
    // Use the confirmed part and delivery ID. A later echo cannot replace either.
    let text = crate::history::outbound_text(row.try_get("body")?)?;
    if text.is_empty() {
        return Ok(None);
    }
    Ok(Some(json!({
        "event":event,
        "kind":"self",
        "entry":entry,
        "receiver":row.try_get::<&str,_>("stage")?,
        "generation":row.try_get::<i64,_>("window")?,
        "observed":row.try_get::<i64,_>("observed_at")?,
        "text":text
    })))
}

pub(super) async fn read(
    db: &mut SqliteConnection,
    c: &Change,
    policy: &Policy,
    event: i64,
) -> Result<Value> {
    let body = record(db, policy, event)
        .await?
        .ok_or("original SELF source unavailable")?;
    crate::privacy::source::retain_self_delivery(
        db,
        &c.plugin,
        &format!("participation:{event}"),
        event,
    )
    .await?;
    let bytes = serde_json::to_vec(&body)?;
    let mut page = sources::page(c, event, &bytes)?;
    if c.params["include_reply"] == true {
        page["reply_to"] = Value::Null;
    }
    if c.params.get("mention_cursor").is_some() {
        return Err("SELF delivery has no ingress mention cursor".into());
    }
    Ok(page)
}

pub(in crate::host) async fn retained_review_reply(
    db: &mut SqliteConnection,
    plugin: &str,
    focus: Option<i64>,
    event: i64,
) -> Result<bool> {
    let Some(focus) = focus.filter(|focus| *focus > event) else {
        return Ok(false);
    };
    let retained: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM source_holds h JOIN personal_deliveries d ON d.event=h.source
         WHERE h.owner=? AND h.item=? AND h.source=?)",
    )
    .bind(plugin)
    .bind(format!("participation:{focus}"))
    .bind(event)
    .fetch_one(&mut *db)
    .await?;
    if !retained {
        return Ok(false);
    }
    let (_, policy) = super::policy(db, plugin).await?;
    Ok(record(db, &policy, event).await?.is_some())
}

pub(super) async fn anchor(
    db: &mut SqliteConnection,
    plugin: &str,
    policy: &Policy,
    focus: i64,
    generation: i64,
) -> Result<Option<Value>> {
    let candidate: Option<(i64, bool)> = sqlx::query_as(
        "SELECT d.event, EXISTS(SELECT 1 FROM event_locators l JOIN events e ON e.id=l.event_id
           WHERE l.receiver=source.bot_stage AND l.native_id=o.locator AND e.entry_id=source.entry_id
             AND e.id<source.id AND e.id IS NOT o.echo_event AND e.id!=d.event)
         FROM events source
         JOIN json_each(source.envelope,'$.segments') part ON json_extract(part.value,'$.type')='reply'
         JOIN social_outbound o ON o.stage=source.bot_stage AND o.entry=source.entry_id AND o.locator=json_extract(part.value,'$.native_id')
         JOIN personal_deliveries d ON d.outbound=o.id
         WHERE source.id=? AND
           (SELECT count(*) FROM json_each(source.envelope,'$.segments') p WHERE json_extract(p.value,'$.type')='reply')=1",
    )
    .bind(focus)
    .fetch_optional(&mut *db)
    .await?;
    let Some((event, ambiguous)) = candidate else {
        return Ok(None);
    };
    if ambiguous || event >= focus {
        return Ok(Some(Value::Null));
    }
    let Some(body) = record(db, policy, event).await? else {
        return Ok(Some(Value::Null));
    };
    if body["generation"] != generation {
        return Ok(Some(Value::Null));
    }
    crate::privacy::source::retain_self_delivery(
        db,
        plugin,
        &format!("participation:{focus}"),
        event,
    )
    .await?;
    Ok(Some(sources::reference(event, &serde_json::to_vec(&body)?)))
}
