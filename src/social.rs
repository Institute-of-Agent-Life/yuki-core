pub mod content;
pub mod send;
pub mod split;
use crate::{
    Result,
    gateway::qq::Hub,
    store::{Mutation, Store},
};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

#[derive(Serialize, Deserialize, Subcommand)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    SelectImages {
        owner: String,
        digest: String,
    },
    Deliver {
        exec: String,
        id: String,
        actor: String,
    },
    Inspect {
        exec: String,
        id: String,
        actor: String,
    },
    Recall {
        exec: String,
        id: String,
        actor: String,
        ordinal: i64,
    },
    Forward {
        exec: String,
        actor: String,
        reference: String,
    },
    Member {
        exec: String,
        actor: String,
        user: i64,
    },
    BindMedia {
        exec: String,
        actor: String,
        id: String,
        kind: String,
        filename: String,
        peer_reference: String,
    },
}
pub enum Change {
    Next(String, String, String, Option<u64>, split::Limits),
    GatewayReply(String, u64, String, Value),
    Reply(String, String, i64, Option<Value>),
    Recall(String, String, String, i64, u64),
    GatewayRecall(String, u64, String, Value),
    RecallReply(String, String, String),
    SelectImages(String, String),
}

pub async fn apply(conn: &mut SqliteConnection, c: &Change) -> Result<Value> {
    match c {
        Change::Next(exec, id, actor, session, limits) => {
            send::next(conn, exec, id, actor, *session, *limits).await
        }
        Change::GatewayReply(stage, session, child, reply) => {
            let (exec,id,ordinal):(String,String,i64)=sqlx::query_as("SELECT d.namespace,d.operation_id,d.ordinal FROM social_parts d JOIN social_plans p USING(namespace,operation_id)
                WHERE d.child=? AND p.stage=? AND d.session=?").bind(child).bind(stage).bind(*session as i64).fetch_one(&mut *conn).await?;
            send::settle(conn, &exec, &id, ordinal, Some(reply)).await
        }
        Change::Reply(exec, id, n, reply) => send::settle(conn, exec, id, *n, reply.as_ref()).await,
        Change::Recall(exec, id, actor, n, session) => {
            let route = send::route(conn, exec, actor).await?;
            let part=sqlx::query("SELECT d.locator,d.child,p.stage,p.entry,p.window,p.actor FROM social_parts d JOIN social_plans p USING(namespace,operation_id)
                WHERE p.namespace=? AND p.operation_id=? AND d.ordinal=? AND d.state='confirmed'")
                .bind(exec).bind(id).bind(n).fetch_one(&mut *conn).await?;
            if part.try_get::<String, _>("actor")? != *actor
                || part.try_get::<String, _>("stage")? != route.stage
                || part.try_get::<String, _>("entry")? != route.entry
                || part.try_get::<i64, _>("window")? != route.window
            {
                return Err("not the original sender or route".into());
            }
            let locator: i64 = part
                .try_get::<Option<String>, _>("locator")?
                .ok_or("no native recall locator")?
                .parse()?;
            let child: String = part.try_get("child")?;
            let grant=sqlx::query("INSERT INTO social_recalls(namespace,child,session,state) VALUES(?,?,?,'dispatched') ON CONFLICT DO NOTHING").bind(exec).bind(&child).bind(*session as i64).execute(&mut *conn).await?.rows_affected()==1;
            let state: String = sqlx::query_scalar(
                "SELECT state FROM social_recalls WHERE namespace=? AND child=?",
            )
            .bind(exec)
            .bind(&child)
            .fetch_one(conn)
            .await?;
            Ok(
                json!({"dispatch":grant,"stage":route.stage,"message_id":locator,"child":child,"state":state}),
            )
        }
        Change::GatewayRecall(stage, session, child, reply) => {
            let exec:String=sqlx::query_scalar("SELECT r.namespace FROM social_recalls r JOIN social_parts d ON d.namespace=r.namespace AND d.child=r.child JOIN social_plans p ON p.namespace=d.namespace AND p.operation_id=d.operation_id
                WHERE r.child=? AND r.session=? AND p.stage=?").bind(child).bind(*session as i64).bind(stage).fetch_one(&mut *conn).await?;
            record_recall(conn, &exec, child, recall_state(Some(reply))).await
        }
        Change::RecallReply(exec, child, state) => record_recall(conn, exec, child, state).await,
        Change::SelectImages(owner, digest) => {
            if digest.len() != 64 || !digest.as_bytes().iter().all(u8::is_ascii_hexdigit) {
                return Err("selected image owner requires its package digest".into());
            }
            sqlx::query_scalar::<_,String>("INSERT INTO social_image_selection SELECT 1,id,? FROM source_owners WHERE id=? AND enabled
              ON CONFLICT(singleton) DO UPDATE SET owner=excluded.owner,digest=excluded.digest RETURNING owner")
              .bind(digest).bind(owner).fetch_one(conn).await?;
            Ok(json!({"selected_owner":owner,"digest":digest}))
        }
    }
}

fn recall_state(reply: Option<&Value>) -> &'static str {
    match reply {
        Some(v) if v["retcode"] == 0 && v["status"] == "ok" => "confirmed",
        Some(v) if v["status"] == "failed" && v["retcode"].as_i64().is_some_and(|n| n != 0) => {
            "failed"
        }
        _ => "unknown",
    }
}
async fn record_recall(
    conn: &mut SqliteConnection,
    exec: &str,
    child: &str,
    state: &str,
) -> Result<Value> {
    let old: String =
        sqlx::query_scalar("SELECT state FROM social_recalls WHERE namespace=? AND child=?")
            .bind(exec)
            .bind(child)
            .fetch_one(&mut *conn)
            .await?;
    if matches!(old.as_str(), "confirmed" | "failed") {
        if old != state && state != "unknown" {
            return Err("conflicting recall result".into());
        }
        return Ok(json!({"state":old,"child":child}));
    }
    sqlx::query("UPDATE social_recalls SET state=? WHERE namespace=? AND child=?")
        .bind(state)
        .bind(exec)
        .bind(child)
        .execute(&mut *conn)
        .await?;
    if state == "confirmed" {
        sqlx::query("UPDATE social_outbound SET recalled=1 WHERE namespace=? AND child=?")
            .bind(exec)
            .bind(child)
            .execute(conn)
            .await?;
    }
    Ok(json!({"state":state,"child":child}))
}
async fn write(store: &Store, c: Change) -> Result<Value> {
    store.write(Mutation::Social(Box::new(c))).await
}
pub async fn deliver(store: &Store, hub: &Hub, exec: &str, id: &str, actor: &str) -> Result<Value> {
    loop {
        let session = {
            let mut tx = store.readers().begin().await?;
            let existing: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM social_plans WHERE namespace=? AND operation_id=?)",
            )
            .bind(exec)
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            if existing {
                let receipt = send::inspect(&mut tx, exec, id, actor).await?;
                if receipt["state"] != "dispatched"
                    || receipt["parts"]
                        .as_array()
                        .is_some_and(|items| items.iter().any(|p| p["state"] == "dispatched"))
                {
                    return Ok(receipt);
                }
            }
            let route = send::route(&mut tx, exec, actor).await?;
            hub.session(&route.stage)
                .ok_or("original receiver offline before dispatch")?
        };
        let next = write(
            store,
            Change::Next(
                exec.into(),
                id.into(),
                actor.into(),
                Some(session),
                hub.reply_limits(),
            ),
        )
        .await?;
        if let Some(receipt) = next.get("receipt") {
            return Ok(receipt.clone());
        }
        let echo = format!(
            "social:{}",
            next["child"].as_str().ok_or("missing child key")?
        );
        let result = hub
            .request_bound(
                next["stage"].as_str().ok_or("missing stage")?,
                next["action"].as_str().ok_or("missing action")?,
                next["params"].clone(),
                Some((session, &echo)),
            )
            .await
            .ok();
        write(
            store,
            Change::Reply(
                exec.into(),
                id.into(),
                next["ordinal"].as_i64().ok_or("missing ordinal")?,
                result,
            ),
        )
        .await?;
    }
}

pub async fn invoke(store: &Store, hub: &Hub, action: Action) -> Result<Value> {
    match action {
        Action::Deliver { exec, id, actor } => deliver(store, hub, &exec, &id, &actor).await,
        Action::Inspect { exec, id, actor } => {
            let mut tx = store.readers().begin().await?;
            send::inspect(&mut tx, &exec, &id, &actor).await
        }
        Action::Recall {
            exec,
            id,
            actor,
            ordinal,
        } => {
            let stage = {
                let mut tx = store.readers().begin().await?;
                send::route(&mut tx, &exec, &actor).await?.stage
            };
            let session = hub
                .session(&stage)
                .ok_or("original receiver offline before recall")?;
            let grant = write(
                store,
                Change::Recall(exec.clone(), id, actor, ordinal, session),
            )
            .await?;
            if grant["dispatch"] != true {
                return Ok(grant);
            }
            let echo = format!(
                "social-recall:{}",
                grant["child"].as_str().ok_or("missing child")?
            );
            let result = hub
                .request_bound(
                    &stage,
                    "delete_msg",
                    json!({"message_id":grant["message_id"]}),
                    Some((session, &echo)),
                )
                .await
                .ok();
            let state = recall_state(result.as_ref());
            write(
                store,
                Change::RecallReply(
                    exec,
                    grant["child"].as_str().ok_or("missing child")?.into(),
                    state.into(),
                ),
            )
            .await
        }
        Action::Forward {
            exec,
            actor,
            reference,
        } => lookup(store, hub, &exec, &actor, Some(&reference), None).await,
        Action::Member { exec, actor, user } => {
            lookup(store, hub, &exec, &actor, None, Some(user)).await
        }
        Action::SelectImages { owner, digest } => {
            write(store, Change::SelectImages(owner, digest)).await
        }
        Action::BindMedia {
            exec,
            actor,
            id,
            kind,
            filename,
            peer_reference,
        } => {
            crate::media::snapshot(
                store,
                crate::media::FileInput {
                    exec: &exec,
                    actor: &actor,
                    operation: &id,
                    relative: &peer_reference,
                    filename: &filename,
                    kind: &kind,
                    named: Some(&id),
                },
            )
            .await
        }
    }
}
async fn lookup(
    store: &Store,
    hub: &Hub,
    exec: &str,
    actor: &str,
    forward: Option<&str>,
    member: Option<i64>,
) -> Result<Value> {
    let (route, source) = {
        let mut tx = store.readers().begin().await?;
        let r = send::route(&mut tx, exec, actor).await?;
        let source = if let Some(reference) = forward {
            Some(sqlx::query_scalar::<_,i64>("SELECT e.id FROM events e JOIN json_each(e.envelope,'$.segments') s WHERE e.entry_id=?
                AND json_extract(s.value,'$.kind')='forward' AND json_extract(s.value,'$.reference')=? AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id) ORDER BY e.id DESC LIMIT 1")
                .bind(&r.entry).bind(reference).fetch_one(&mut *tx).await?)
        } else {
            if !r.group {
                return Err("group membership requires original group".into());
            }
            None
        };
        (r, source)
    };
    let (name, params) = match forward {
        Some(reference) => ("get_forward_msg", json!({"id":reference})),
        None => (
            "get_group_member_info",
            json!({"user_id":member,"group_id":route.target,"no_cache":true}),
        ),
    };
    let result = hub.request(&route.stage, name, params).await?;
    if result["status"] != "ok" || result["retcode"] != 0 {
        return Err("QQ lookup unavailable".into());
    }
    let mut tx = store.readers().begin().await?;
    let current = send::route(&mut tx, exec, actor).await?;
    if current.stage != route.stage
        || current.entry != route.entry
        || current.revision != route.revision
    {
        return Err("original route changed during lookup".into());
    }
    if let Some(source) = source
        && crate::privacy::source::is_erased(&mut tx, source).await?
    {
        return Err("original forward was erased".into());
    }
    if let Some(member) = member
        && (crate::gateway::qq::native_id(&result["data"]["user_id"]) != Some(member.to_string())
            || result["data"].get("group_id").is_some()
                && crate::gateway::qq::native_id(&result["data"]["group_id"])
                    != Some(route.target.to_string()))
    {
        return Err("membership answer has another target".into());
    }
    Ok(
        json!({"data":result["data"],"stage":route.stage,"entry":route.entry,"coverage":"single_original_qq_receiver"}),
    )
}
