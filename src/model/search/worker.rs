use super::{Change, Config, Mode, Query, source};
use crate::{
    Result,
    identity::new_id,
    model::attempt,
    runtime::{effects, execution, main::transport},
    store::{Mutation, Store},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};
pub enum Reply {
    Response(u16, Vec<u8>, bool),
    Unknown,
}
fn hash(v: &Value) -> String {
    format!("{:x}", Sha256::digest(v.to_string().as_bytes()))
}
fn logical(exec: &str, op: &str) -> String {
    format!("native-search:{}", hash(&json!([exec, op])))
}
async fn store_result(
    conn: &mut SqliteConnection,
    exec: &str,
    id: &str,
    value: &Value,
) -> Result<()> {
    let state = match value["status"].as_str() {
        Some("unknown") => "unknown",
        Some("complete" | "partial") => "confirmed",
        _ => "failed",
    };
    sqlx::query("UPDATE operations SET state=?,result=? WHERE namespace=? AND operation_id=? AND state='intent'").bind(state).bind(value.to_string()).bind(exec).bind(id).execute(conn).await?;
    Ok(())
}
async fn prepare(
    conn: &mut SqliteConnection,
    cfg: &Config,
    exec: &str,
    actor: &str,
    op: &str,
    q: &Query,
    namespace: &str,
) -> Result<Value> {
    effects::authorize(conn, exec, actor, true).await?;
    if op.trim().is_empty() || op.len() > 256 {
        return Err("original search operation required".into());
    }
    let profile = cfg
        .connection
        .as_ref()
        .ok_or("native search connection unavailable")?;
    let logical = logical(exec, op);
    let entry:String=sqlx::query_scalar("SELECT coalesce(e.entry_id,o.conversation) FROM executions x LEFT JOIN events e ON e.id=x.source LEFT JOIN main_origins o ON o.ref=x.origin WHERE x.id=?").bind(exec).fetch_one(&mut *conn).await?;
    let key = hash(&json!([
        "independent-native-search-v2",
        namespace,
        q,
        actor,
        entry,
        cfg.cache_seconds
    ]));
    let effect = format!("search-result:{logical}");
    effects::apply(
        conn,
        &effects::Action::Intent {
            execution: exec.into(),
            id: effect.clone(),
            actor: actor.into(),
            action: effects::Operation::External,
            target: "core.native_search".into(),
            payload: json!({"key":key}).to_string(),
            parent: None,
            ordinal: None,
        },
    )
    .await?;
    if let Some(v)=sqlx::query_scalar::<_,String>("SELECT result FROM operations WHERE namespace=? AND operation_id=? AND state!='intent' AND result IS NOT NULL").bind(exec).bind(&effect).fetch_optional(&mut *conn).await? {
        return Ok(json!({"result":serde_json::from_str::<Value>(&v)?}));
    }
    // A reserved original request always settles from its own attempt, even if a sibling filled the cache.
    let existing=sqlx::query("SELECT a.execution,r.request_json,t.state,t.response,t.status_code FROM model_requests r JOIN activations a ON a.id=r.activation LEFT JOIN model_attempts t ON t.logical=r.id WHERE r.id=? ORDER BY t.ordinal DESC LIMIT 1").bind(&logical).fetch_optional(&mut *conn).await?;
    if existing.is_none() {
        if let Some((origin, raw)) = sqlx::query_as::<_,(String,String)>(
            "SELECT o.namespace,o.result FROM operations o JOIN executions x ON x.id=o.namespace WHERE o.target='core.native_search' AND o.state='confirmed' AND json_extract(o.payload,'$.key')=? AND json_extract(o.result,'$.status')='complete' AND json_extract(o.result,'$.cached')=0 AND json_extract(o.result,'$.cache_expires_at')>unixepoch() AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=x.source OR i.source IN(SELECT event FROM main_dependencies WHERE execution=x.id)) ORDER BY o.rowid DESC LIMIT 1",
        ).bind(&key).fetch_optional(&mut *conn).await? {
            sqlx::query("INSERT OR IGNORE INTO main_dependencies SELECT ?1,e.id FROM events e WHERE e.id IN(SELECT source FROM executions WHERE id=?2 UNION SELECT event FROM main_dependencies WHERE execution=?2)").bind(exec).bind(origin).execute(&mut *conn).await?;
            let mut value: Value = serde_json::from_str(&raw)?;
            value["cached"] = json!(true);
            store_result(conn, exec, &effect, &value).await?;
            return Ok(json!({"result":value}));
        }
    } else if let Some(r) = &existing {
        let state = r.get::<Option<&str>, _>("state");
        if matches!(state, Some("unknown" | "dispatched")) {
            return Ok(json!({"result":{"status":"unknown","logical":logical,"sources":[]}}));
        }
        // The response and search receipt publish in one transaction, except when an
        // operator has reconciled a held original. Never dispatch a second paid attempt.
        if state.is_some() {
            return Err("original search attempt requires reconciliation".into());
        }
    }
    let body = q.wire(profile)?;
    if existing.is_none() {
        let child = new_id(conn).await?;
        sqlx::query("INSERT INTO main_origins(ref,kind,conversation,parent) VALUES(?,'child',?,?)")
            .bind(&logical)
            .bind(&entry)
            .bind(exec)
            .execute(&mut *conn)
            .await?;
        sqlx::query("INSERT INTO executions(id,origin,state,work_id) SELECT ?,?,'running',work_id FROM executions WHERE id=?").bind(&child).bind(&logical).bind(exec).execute(&mut *conn).await?;
        sqlx::query("INSERT OR IGNORE INTO main_dependencies SELECT ?1,e.id FROM events e WHERE e.id IN(SELECT source FROM executions WHERE id=?2 UNION SELECT event FROM main_dependencies WHERE execution=?2)").bind(&child).bind(exec).execute(&mut *conn).await?;
        sqlx::query_scalar::<_,String>("INSERT INTO budget_members SELECT ?1,m.root FROM budget_members m JOIN main_origins o ON o.parent=m.execution JOIN executions x ON x.origin=o.ref WHERE x.id=?1 AND o.ref=?2 RETURNING root").bind(&child).bind(&logical).fetch_one(&mut *conn).await?;
        execution::activate(conn, &child, &logical, namespace).await?;
        attempt::apply(
            conn,
            &attempt::Action::Reserve(attempt::Reservation {
                activation: logical.clone(),
                logical: logical.clone(),
                input: i64::try_from(body.to_string().chars().count())?,
                output: profile.max_output,
                safe_rate_limit_once: false,
                native_tools: true,
                request: body.to_string(),
            }),
        )
        .await?;
    }
    let physical = attempt::apply(
        conn,
        &attempt::Action::Dispatch {
            logical: logical.clone(),
            attempt: format!("{logical}:0"),
        },
    )
    .await?;
    if physical["dispatch_now"] != true {
        return Err("original native search already dispatched".into());
    }
    Ok(json!({"logical":logical,"wire":physical["request_raw"]}))
}
async fn finish(
    conn: &mut SqliteConnection,
    cfg: &Config,
    caller: (&str, &str),
    id: &str,
    q: &Query,
    reply: &Reply,
) -> Result<Value> {
    let (exec, actor) = caller;
    let profile = cfg.connection.as_ref().ok_or("no native profile")?;
    let physical = format!("{id}:0");
    let mut v = match reply {
        Reply::Unknown => {
            attempt::apply(
                conn,
                &attempt::Action::Unknown {
                    attempt: physical.clone(),
                },
            )
            .await?;
            json!({"status":"unknown","sources":[]})
        }
        Reply::Response(code, bytes, complete) => {
            let raw = String::from_utf8_lossy(bytes);
            let usage = profile.protocol.usage(&raw).to_string();
            if !*complete {
                attempt::apply(
                    conn,
                    &attempt::Action::Hold {
                        attempt: physical.clone(),
                        body: raw.to_string(),
                        partial_bytes: Some(bytes.clone()),
                        usage,
                        diagnostic: "incomplete_search_response".into(),
                        unknown: true,
                        status: i64::from(*code),
                    },
                )
                .await?;
                json!({"status":"unknown","sources":[]})
            } else if !(200..300).contains(code) {
                attempt::apply(
                    conn,
                    &attempt::Action::Rejected {
                        attempt: physical.clone(),
                        status: i64::from(*code),
                        unbilled_before_model: false,
                        body: raw.to_string(),
                        usage,
                    },
                )
                .await?;
                json!({"status":"rejected","sources":[],"http_status":code})
            } else {
                let retained = attempt::apply(
                    conn,
                    &attempt::Action::Response {
                        attempt: physical.clone(),
                        body: raw.to_string(),
                        usage,
                        intents: "[]".into(),
                    },
                )
                .await?;
                if retained["state"] != "response" {
                    json!({"status":"invalid_or_revoked","sources":[]})
                } else {
                    match serde_json::from_str::<Value>(&raw) {
                        Ok(body) => source::collect(profile.protocol, &body, q),
                        Err(_) => json!({"status":"invalid_response","sources":[]}),
                    }
                }
            }
        }
    };
    if effects::authorize(conn, exec, actor, true).await.is_err() {
        return Ok(json!({"status":"source_unavailable","sources":[]}));
    }
    v["logical"] = json!(id);
    v["attempt"] = json!(physical);
    v["cached"] = json!(false);
    if v["status"] == "complete" && cfg.cache_seconds > 0 {
        v["cache_expires_at"] = json!(
            sqlx::query_scalar::<_, i64>("SELECT unixepoch()+?")
                .bind(cfg.cache_seconds)
                .fetch_one(&mut *conn)
                .await?
        );
    }
    store_result(conn, exec, &format!("search-result:{id}"), &v).await?;
    sqlx::query("UPDATE executions SET state=? WHERE origin=?")
        .bind(if v["status"] == "complete" || v["status"] == "partial" {
            "completed"
        } else {
            "held"
        })
        .bind(id)
        .execute(conn)
        .await?;
    Ok(v)
}
pub async fn apply(conn: &mut SqliteConnection, c: &Change) -> Result<Value> {
    match c {
        Change::Prepare {
            cfg,
            exec,
            actor,
            op,
            query,
            namespace,
        } => prepare(conn, cfg, exec, actor, op, query, namespace).await,
        Change::Finish {
            cfg,
            exec,
            actor,
            logical,
            query,
            reply,
        } => finish(conn, cfg, (exec, actor), logical, query, reply).await,
        Change::Tool {
            exec,
            actor,
            op,
            value,
            ok,
        } => {
            effects::authorize(conn, exec, actor, true).await?;
            sqlx::query("UPDATE operations SET state=?,result=? WHERE namespace=? AND operation_id=? AND target='web_search' AND state='intent'").bind(if *ok{"confirmed"}else if value["status"]=="unknown"{"unknown"}else{"failed"}).bind(value.to_string()).bind(exec).bind(op).execute(conn).await?;
            Ok(Value::Null)
        }
    }
}
pub async fn run(store: &Store, exec: &str, actor: &str, op: &str, q: Query) -> Result<Value> {
    let eng = store.search.as_ref().ok_or("search_disabled")?;
    if !eng.cfg.bridge() {
        return Err(if matches!(eng.cfg.mode, Mode::Tavily | Mode::Both) {
            "external search requires its authorized owner"
        } else {
            "native bridge disabled"
        }
        .into());
    }
    let q = q.normalize()?;
    let selected = crate::model::profile_store::runtime::search(store, exec, &eng.cfg).await?;
    let cfg = &selected;
    let profile = cfg
        .connection
        .as_ref()
        .ok_or("no authorized search connection")?;
    let secret = profile
        .key_env
        .as_ref()
        .map(std::env::var)
        .transpose()
        .map_err(|_| "search credential unavailable")?;
    let namespace = hash(&json!([
        cfg.connection_id,
        cfg.credential_namespace,
        profile.contract(),
        secret
            .as_deref()
            .map(|s| format!("{:x}", Sha256::digest(s.as_bytes())))
    ]));
    let stripe =
        usize::from(Sha256::digest(json!([&namespace, &q, actor]).to_string().as_bytes())[0])
            % eng.locks.len();
    let _lock = eng.locks[stripe].lock().await;
    let prepared = store
        .write(Mutation::Search(Box::new(Change::Prepare {
            cfg: cfg.clone(),
            exec: exec.into(),
            actor: actor.into(),
            op: op.into(),
            query: q.clone(),
            namespace,
        })))
        .await?;
    if let Some(result) = prepared.get("result") {
        return Ok(result.clone());
    }
    let id = prepared["logical"]
        .as_str()
        .ok_or("missing original logical request")?;
    let outcome = match transport::physical(
        &eng.client,
        profile,
        prepared["wire"].as_str().ok_or("missing wire")?,
    )
    .await
    {
        Ok((code, bytes, complete)) => Reply::Response(code, bytes, complete),
        Err(_) => Reply::Unknown,
    };
    store
        .write(Mutation::Search(Box::new(Change::Finish {
            cfg: cfg.clone(),
            exec: exec.into(),
            actor: actor.into(),
            logical: id.into(),
            query: q,
            reply: outcome,
        })))
        .await
}
