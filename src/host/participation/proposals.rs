use super::{Change, Policy, sources};
use crate::{Result, identity::new_id, runtime::main};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};
use std::collections::BTreeMap;

async fn receiver(db: &mut SqliteConnection, entry: &str, stage: &str) -> Result<()> {
    let row=sqlx::query("SELECT e.space,a.platform,a.namespace,a.native FROM entries e JOIN assignments s ON s.id=? JOIN accounts a ON a.id=s.account_id WHERE e.id=? AND e.scope='group' AND e.closed_at IS NULL AND s.kind='self' AND s.status='active'")
        .bind(stage).bind(entry).fetch_one(&mut *db).await?;
    let space: Vec<String> = serde_json::from_str(row.get("space"))?;
    if space.len() != 3
        || space[0] != row.get::<String, _>("platform")
        || space[1] != row.get::<String, _>("namespace")
    {
        return Err("SELF receiver outside original group namespace".into());
    }
    if space[0]=="qq" {
        if super::qq_route(db,&space[1],&space[2]).await?.is_none_or(|(pin,_,enabled)| !enabled || pin!=stage) {
            return Err("original SELF group receiver unavailable".into());
        }
    } else if !sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM host_installations WHERE platform=? AND namespace=? AND bot_stage=? AND enabled AND online AND EXISTS(SELECT 1 FROM json_each(grants) WHERE value='gateway.send'))")
        .bind(&space[0]).bind(&space[1]).bind(stage).fetch_one(&mut *db).await? {
        return Err("original SELF gateway unavailable".into());
    }
    Ok(())
}

pub(super) async fn submit(
    db: &mut SqliteConnection,
    c: &Change,
    policy: &Policy,
    revision: &str,
) -> Result<Value> {
    let entry = c.params["entry"].as_str().ok_or("scene required")?;
    sources::entry(db, policy, entry).await?;
    let stage = policy
        .self_scenes
        .get(entry)
        .ok_or("SELF scene not installed")?;
    receiver(db, entry, stage).await?;
    let key = c.params["key"].as_str().ok_or("proposal key required")?;
    let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&c.params)?));
    if let Some((old,old_revision,execution))=sqlx::query_as::<_,(String,String,String)>("SELECT digest,revision,execution FROM participation_proposals WHERE plugin=? AND proposal=?")
        .bind(&c.plugin).bind(key).fetch_optional(&mut *db).await? {
        if old!=digest || old_revision!=revision { return Err("original SELF proposal conflicts".into()); }
        authorize(db,&execution).await?;
        return Ok(json!({"execution":execution,"newly_accepted":false}));
    }
    let competing: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM host_installations i JOIN host_participation p
             ON p.plugin=i.plugin AND p.revision=i.revision
           JOIN json_each(p.body,'$.self_scenes') scene
           WHERE i.enabled AND i.plugin!=? AND scene.key=?
             AND EXISTS(SELECT 1 FROM json_each(i.grants) WHERE value='participation.self_propose')
        )",
    )
    .bind(&c.plugin)
    .bind(entry)
    .fetch_one(&mut *db)
    .await?;
    if competing {
        return Err("original SELF scene has multiple enabled owners".into());
    }
    let expected_generation = c
        .params
        .get("expected_generation")
        .map(|value| value.as_i64().ok_or("invalid SELF generation"))
        .transpose()?;
    if let Some(expected) = expected_generation
        && sources::current_generation(db, entry).await? != expected
    {
        return Err("original SELF generation changed before admission".into());
    }
    let selected = c.params["sources"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut guarded = BTreeMap::new();
    let source_guard = c.params["source_guard"].as_array();
    if let Some(guard) = source_guard {
        let generation = expected_generation.ok_or("SELF source guard requires a generation")?;
        if selected.is_empty() {
            return Err("SELF source guard requires selected context".into());
        }
        for reference in guard {
            let event = reference["event"]
                .as_i64()
                .ok_or("invalid guarded source")?;
            let body =
                sources::context_body(db, &c.plugin, policy, entry, generation, event).await?;
            if sources::reference(event, &serde_json::to_vec(&body)?) != *reference {
                return Err("original SELF source version changed".into());
            }
            if guarded.insert(event, body).is_some() {
                return Err("duplicate guarded SELF source".into());
            }
        }
    }
    let origin = format!("participation:{}", new_id(db).await?);
    sqlx::query("INSERT INTO main_origins(ref,kind,conversation) VALUES(?,'self',?)")
        .bind(&origin)
        .bind(entry)
        .execute(&mut *db)
        .await?;
    sqlx::query("INSERT INTO participation_proposals(plugin,proposal,revision,entry,receiver,origin,digest) VALUES(?,?,?,?,?,?,?)")
        .bind(&c.plugin).bind(key).bind(revision).bind(entry).bind(stage).bind(&origin).bind(digest).execute(&mut *db).await?;
    let mut contexts = vec![];
    let mut dependencies: Vec<_> = guarded.keys().copied().collect();
    for id in selected {
        let event = id.as_i64().ok_or("invalid proposal source")?;
        let frame = if source_guard.is_some() {
            guarded
                .remove(&event)
                .ok_or("selected SELF source is not guarded")?
        } else {
            let (source_entry, observed_generation): (String, Option<i64>) =
                sqlx::query_as("SELECT entry_id,observed_generation FROM events WHERE id=?")
                    .bind(event)
                    .fetch_one(&mut *db)
                    .await?;
            if source_entry != entry
                || (expected_generation.is_some() && observed_generation != expected_generation)
            {
                return Err("proposal source outside original scene".into());
            }
            dependencies.push(event);
            sources::checked(db, &c.plugin, policy, event).await?
        };
        contexts.push(json!({"event":event,"envelope":frame}));
    }
    let action = main::Change::Operator(main::Action::Trigger {
        id: origin,
        key: key.into(),
        body: json!({"text":c.params["text"],"context":contexts}).to_string(),
    });
    let result = main::apply(db, c.config.as_ref().ok_or("Main unavailable")?, &action).await?;
    let execution = result["execution"]
        .as_str()
        .ok_or("missing original SELF execution")?;
    for event in dependencies {
        sqlx::query("INSERT OR IGNORE INTO main_dependencies VALUES(?,?)")
            .bind(execution)
            .bind(event)
            .execute(&mut *db)
            .await?;
    }
    sqlx::query("UPDATE participation_proposals SET execution=? WHERE plugin=? AND proposal=?")
        .bind(execution)
        .bind(&c.plugin)
        .bind(key)
        .execute(db)
        .await?;
    Ok(json!({"execution":execution,"newly_accepted":true}))
}

/// Recheck every delegated SELF ancestor without substituting a nearby human event.
pub(crate) async fn authorize(db: &mut SqliteConnection, execution: &str) -> Result<()> {
    let rows=sqlx::query("WITH RECURSIVE family(id) AS(SELECT ? UNION SELECT o.parent FROM family f JOIN executions x ON x.id=f.id JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL)
      SELECT p.plugin,p.revision,p.entry,p.receiver FROM family f JOIN executions x ON x.id=f.id JOIN participation_proposals p ON p.origin=x.origin")
      .bind(execution).fetch_all(&mut *db).await?;
    for row in rows {
        let (revision, policy) = super::policy(db, row.get("plugin")).await?;
        let entry: &str = row.get("entry");
        let stage: &str = row.get("receiver");
        if revision != row.get::<String, _>("revision")
            || policy.self_scenes.get(entry).map(String::as_str) != Some(stage)
        {
            return Err("original SELF policy changed".into());
        }
        sources::entry(db, &policy, entry).await?;
        receiver(db, entry, stage).await?;
        let denied:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM main_dependencies d JOIN events e ON e.id=d.event JOIN persons p ON p.id=e.raw_actor JOIN persons r ON r.id=coalesce(p.current_root,p.id) JOIN assignments a ON a.id=e.actor_stage JOIN assignments b ON b.id=e.bot_stage WHERE d.execution=? AND (NOT p.enabled OR NOT r.enabled OR a.status!='active' OR b.status!='active'))")
            .bind(execution).fetch_one(&mut *db).await?;
        if denied {
            return Err("original SELF context authority changed".into());
        }
    }
    Ok(())
}
pub(crate) async fn self_binding(
    db: &mut SqliteConnection,
    execution: &str,
) -> Result<Option<(String, String, String)>> {
    let binding=sqlx::query_as("WITH RECURSIVE family(id) AS(SELECT ? UNION SELECT o.parent FROM family f JOIN executions x ON x.id=f.id JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL)
      SELECT p.origin,p.entry,p.receiver FROM family f JOIN executions x ON x.id=f.id JOIN participation_proposals p ON p.origin=x.origin LIMIT 1")
      .bind(execution).fetch_optional(&mut *db).await?;
    if binding.is_some() {
        authorize(db, execution).await?;
    }
    Ok(binding)
}
pub(crate) async fn self_group_route(
    db: &mut SqliteConnection,
    execution: &str,
) -> Result<Option<crate::social::send::Route>> {
    let Some((owner, entry, stage)) = self_binding(db, execution).await? else {
        return Ok(None);
    };
    let row=sqlx::query("SELECT e.space,t.window,w.generation FROM entries e JOIN main_turns t ON t.execution=? JOIN main_windows w ON w.conversation=e.id WHERE e.id=?")
        .bind(execution).bind(&entry).fetch_one(&mut *db).await?;
    let space: Vec<String> = serde_json::from_str(row.get("space"))?;
    if space[0] != "qq" {
        return Ok(None);
    }
    if row.get::<i64, _>("window") != row.get::<i64, _>("generation") {
        return Err("SELF conversation generation changed".into());
    }
    let route = super::qq_route(db, &space[1], &space[2])
        .await?
        .ok_or("missing original route")?;
    Ok(Some(crate::social::send::Route {
        entry,
        person: owner,
        stage,
        target: space[2].parse()?,
        private_stage: None,
        group: true,
        window: row.get("window"),
        revision: Some(route.1),
    }))
}
