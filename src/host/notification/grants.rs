use super::{Call, Policy, scope};
use crate::{Result, runtime::effects};
use serde_json::{Map, Value, json};
use sqlx::{Row, SqliteConnection};

struct Administrator {
    person: String,
    entry: String,
    receiver: String,
    native: String,
}

async fn administrator(
    db: &mut SqliteConnection,
    call: &Call,
    replay: bool,
) -> Result<Administrator> {
    let row = sqlx::query(
        "SELECT c.execution,c.generation,c.state,c.revision,i.revision AS installed,
                coalesce(x.original_actor,x.origin) AS actor
         FROM host_calls c JOIN executions x ON x.id=c.execution
         JOIN host_installations i ON i.plugin=c.plugin
         WHERE c.handle=? AND c.plugin=?",
    )
    .bind(call.params["handle"].as_str())
    .bind(&call.plugin)
    .fetch_one(&mut *db)
    .await?;
    if !replay
        && (row.get::<&str, _>("revision") != row.get::<&str, _>("installed")
            || row.get::<&str, _>("generation") != call.generation
            || row.get::<&str, _>("state") != "dispatched")
    {
        return Err("notification target mutation requires an active original host call".into());
    }
    let execution: &str = row.get("execution");
    effects::authorize(db, execution, row.get("actor"), !replay).await?;
    let origin = sqlx::query(
        "WITH RECURSIVE family(id) AS(SELECT ? UNION SELECT o.parent FROM family f
         JOIN executions x ON x.id=f.id JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL)
         SELECT e.raw_actor,e.entry_id,e.bot_stage,b.native FROM family f
         JOIN executions x ON x.id=f.id JOIN events e ON e.id=x.source
         JOIN persons p ON p.id=e.raw_actor AND p.enabled
         JOIN persons root ON root.id=coalesce(p.current_root,p.id) AND root.enabled AND root.admin
         JOIN assignments human ON human.id=e.actor_stage AND human.kind='person' AND human.status='active'
         JOIN assignments bot ON bot.id=e.bot_stage AND bot.kind='self' AND bot.status='active'
         JOIN accounts b ON b.id=bot.account_id
         WHERE NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id) LIMIT 1",
    )
    .bind(execution)
    .fetch_optional(&mut *db)
    .await?
    .ok_or("notification target mutation requires the original real administrator")?;
    Ok(Administrator {
        person: origin.get("raw_actor"),
        entry: origin.get("entry_id"),
        receiver: origin.get("bot_stage"),
        native: origin.get("native"),
    })
}

async fn target(
    db: &mut SqliteConnection,
    actor: &Administrator,
    requested: &Value,
) -> Result<scope::Target> {
    let candidates: Vec<String> = sqlx::query_scalar(
        "SELECT e.id FROM entries e JOIN assignments bot ON bot.id=?
         JOIN accounts b ON b.id=bot.account_id WHERE e.closed_at IS NULL
         AND ((e.scope='private' AND e.bot_stage=bot.id) OR
             (e.scope='group' AND json_extract(e.space,'$[0]')=b.platform
                 AND json_extract(e.space,'$[1]')=b.namespace))",
    )
    .bind(&actor.receiver)
    .fetch_all(&mut *db)
    .await?;
    let policy = Policy {
        targets: candidates
            .into_iter()
            .map(|id| (id, actor.receiver.clone()))
            .collect(),
        agent: false,
    };
    let mut selected = requested.clone();
    // The real subscription consumer omits conversation_id for its current Person/Space.
    let current_matches: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM entries e LEFT JOIN assignments a ON a.id=e.actor_stage
         LEFT JOIN persons p ON p.id=a.original_person LEFT JOIN persons target ON target.id=?3
         WHERE e.id=?1 AND ((?2='group' AND e.space=?3) OR (?2='private' AND e.scope='private'
             AND target.enabled AND coalesce(p.current_root,p.id)=coalesce(target.current_root,target.id))))",
    )
    .bind(&actor.entry)
    .bind(requested["target_type"].as_str())
    .bind(requested["target_id"].as_str())
    .fetch_one(&mut *db)
    .await?;
    if selected["conversation_id"].is_null() && current_matches {
        selected["conversation_id"] = json!(actor.entry);
    }
    scope::select(db, &policy, &selected).await
}

pub(super) async fn mutate(db: &mut SqliteConnection, call: &Call) -> Result<Value> {
    scope::policy(db, &call.plugin).await?;
    let handle = call.params["handle"]
        .as_str()
        .ok_or("original call required")?;
    let key = call.params["key"]
        .as_str()
        .ok_or("original mutation key required")?;
    let digest = crate::memory::scope::hash(&call.params);
    let prior: Option<(String, String, String)> = sqlx::query_as(
        "SELECT method,digest,result FROM notification_grant_operations WHERE plugin=? AND handle=? AND key=?",
    ).bind(&call.plugin).bind(handle).bind(key).fetch_optional(&mut *db).await?;
    let actor = administrator(db, call, prior.is_some()).await?;
    if let Some((method, original, result)) = prior {
        if method != call.method || original != digest {
            return Err("original notification target mutation conflicts".into());
        }
        return Ok(serde_json::from_str(&result)?);
    }
    if call.params["bot_user_id"]
        .as_str()
        .is_some_and(|bot| !bot.is_empty() && bot != actor.native)
    {
        return Err("notification grant bot differs from the original connected bot".into());
    }
    let target = target(db, &actor, &call.params["target"]).await?;
    let enabled = call.method == "notification.grant_target";
    let was_enabled: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM notification_target_scope WHERE plugin=? AND entry=?)",
    )
    .bind(&call.plugin)
    .bind(&target.entry)
    .fetch_one(&mut *db)
    .await?;
    sqlx::query(
        "INSERT INTO notification_target_grants(plugin,namespace,entry,receiver,created_by,source_call,enabled)
         SELECT plugin,namespace,?2,?3,?4,?5,?6 FROM host_installations WHERE plugin=?1
         ON CONFLICT(plugin,namespace,entry) DO UPDATE SET receiver=excluded.receiver,
             created_by=excluded.created_by,source_call=excluded.source_call,enabled=excluded.enabled,
             revision=notification_target_grants.revision+1",
    ).bind(&call.plugin).bind(&target.entry).bind(&target.receiver).bind(&actor.person)
        .bind(handle).bind(enabled).execute(&mut *db).await?;
    let result = if enabled {
        json!({"target_type":call.params["target"]["target_type"],
            "target_id":target.person.as_ref().or(target.space.as_ref()),
            "conversation_id":target.entry,"bot_user_id":actor.native,
            "enabled":true,"created_by_user_id":actor.person})
    } else {
        json!(was_enabled)
    };
    sqlx::query("INSERT INTO notification_grant_operations VALUES(?,?,?,?,?,?)")
        .bind(&call.plugin)
        .bind(handle)
        .bind(key)
        .bind(&call.method)
        .bind(digest)
        .bind(result.to_string())
        .execute(db)
        .await?;
    Ok(result)
}

pub(super) async fn status(db: &mut SqliteConnection, call: &Call) -> Result<Value> {
    scope::policy(db, &call.plugin).await?;
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT 'outbox_'||d.state,count(*) FROM notification_deliveries d
         JOIN notification_publications p ON p.id=d.publication WHERE p.plugin=?1 GROUP BY d.state
         UNION ALL SELECT 'turn_'||x.state,count(*) FROM notification_publications p
         JOIN executions x ON x.id=p.agent_execution WHERE p.plugin=?1 GROUP BY x.state",
    )
    .bind(&call.plugin)
    .fetch_all(db)
    .await?;
    Ok(Value::Object(
        rows.into_iter()
            .map(|(key, count)| (key, json!(count)))
            .collect::<Map<_, _>>(),
    ))
}
