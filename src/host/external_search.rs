//! A single paid provider dispatch under the original, approved tool operation.
use super::{domain, grants::Install};
use crate::{
    Result,
    identity::new_id,
    model::{attempt, search},
    runtime::effects,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    tool: String,
    provider: String,
    purpose: String,
    endpoint: String,
}

impl Policy {
    pub(super) fn validate(&self, install: &Install) -> Result<()> {
        let url = reqwest::Url::parse(&self.endpoint)?;
        let loopback = url.host_str().is_some_and(|s| {
            s.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|a| a.is_loopback())
        });
        if self.provider != "tavily"
            || self.purpose != "web_search"
            || !install.tools.iter().any(|t| t.name == self.tool)
            || !["tool.call", "forget.owner"]
                .iter()
                .all(|g| install.grants.contains(*g))
            || url.host().is_none()
            || !(url.scheme() == "https" || url.scheme() == "http" && loopback)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(
                "external search requires an approved tool, purpose and provider endpoint".into(),
            );
        }
        Ok(())
    }
}

async fn enabled(db: &mut SqliteConnection) -> Result<bool> {
    Ok(crate::operator::domain::get(db, "external-search-enabled").await? == Some(json!(true)))
}

async fn permitted(db: &mut SqliteConnection, plugin: &str, generation: &str) -> Result<bool> {
    for grant in ["search.external", "tool.call", "forget.owner"] {
        if !domain::granted(db, plugin, generation, grant).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn grants(db: &mut SqliteConnection, plugin: &str, generation: &str) -> Result<()> {
    if !permitted(db, plugin, generation).await? {
        return Err("plugin instance or grant unavailable".into());
    }
    Ok(())
}

/// Only new tool views consult readiness. A paid view retains its original
/// declaration and rechecks the live grants at dispatch.
pub(crate) async fn can_offer_external_search(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: Option<&str>,
) -> Result<bool> {
    let Some(generation) = generation else {
        return Ok(false);
    };
    if super::grants::stable_base() || !enabled(db).await? {
        return Ok(false);
    }
    permitted(db, plugin, generation).await
}

/// Fence a previously pinned tool view before releasing its query to the peer.
pub(super) async fn before_dispatch(
    db: &mut SqliteConnection,
    handle: &str,
    plugin: &str,
    generation: &str,
) -> Result<()> {
    let search = is_search(db, handle).await?;
    if search {
        grants(db, plugin, generation).await?;
        if !enabled(db).await? {
            return Err("external search disabled".into());
        }
        let sources: Vec<i64> = sqlx::query_scalar("SELECT e.id FROM host_calls c JOIN executions x ON x.id=c.execution JOIN events e ON e.id=x.source OR e.id IN(SELECT event FROM main_dependencies WHERE execution=x.id) WHERE c.handle=? AND e.raw_actor IS NOT NULL")
            .bind(handle).fetch_all(&mut *db).await?;
        for event in sources {
            crate::privacy::source::acquire(db, plugin, handle, event).await?;
        }
    }
    Ok(())
}

async fn is_search(db: &mut SqliteConnection, handle: &str) -> Result<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM host_calls c JOIN operations o ON o.namespace=c.execution AND o.operation_id=c.operation JOIN core_meta m ON m.key='plugin-install:'||c.plugin WHERE c.handle=? AND c.method='tool.call' AND o.target=json_extract(m.value,'$.external_search.tool'))")
        .bind(handle).fetch_one(db).await?)
}

/// Generic tool.status exposes only the operation state. The deployment-bound
/// search.status remains the reader for the original provider receipt.
pub(super) async fn receipt(
    db: &mut SqliteConnection,
    handle: &str,
    plugin: &str,
    generation: &str,
) -> Result<bool> {
    if !is_search(db, handle).await? {
        return Ok(false);
    }
    grants(db, plugin, generation).await?;
    let same: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM host_calls c JOIN core_meta a ON a.key='plugin-approval:'||c.plugin
        LEFT JOIN model_requests r ON r.id='external-search:'||c.handle WHERE c.handle=? AND c.plugin=?
        AND ((r.id IS NULL AND c.generation=?) OR json_extract(r.request_json,'$.deployment')=json_extract(a.value,'$.deployment')))")
        .bind(handle).bind(plugin).bind(generation).fetch_one(db).await?;
    if !same {
        return Err("search receipt outside original deployment".into());
    }
    Ok(true)
}

pub(super) async fn disconnected(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
) -> Result<()> {
    sqlx::query("UPDATE model_attempts SET state='unknown',ever_unknown=1 WHERE state='dispatched'
        AND logical IN(SELECT 'external-search:'||handle FROM host_calls WHERE plugin=? AND generation=?)")
        .bind(plugin).bind(generation).execute(&mut *db).await?;
    sqlx::query("UPDATE executions SET state='held' WHERE state='running' AND origin IN(
        SELECT t.logical FROM host_calls c JOIN model_attempts t ON t.logical='external-search:'||c.handle
        WHERE c.plugin=? AND c.generation=? AND t.state='unknown')")
        .bind(plugin).bind(generation).execute(db).await?;
    Ok(())
}

/// Refuse to publish a tool success when the underlying provider receipt is missing or adverse.
pub(super) async fn tool_outcome<'a>(
    db: &mut SqliteConnection,
    handle: &str,
    state: &'a str,
) -> Result<&'a str> {
    if !is_search(db, handle).await? {
        return Ok(state);
    }
    let logical = format!("external-search:{handle}");
    sqlx::query("UPDATE model_attempts SET state='unknown',ever_unknown=1 WHERE logical=? AND state='dispatched'").bind(&logical).execute(&mut *db).await?;
    sqlx::query(
        "UPDATE executions SET state='held' WHERE origin=?1 AND state='running' AND EXISTS(
        SELECT 1 FROM model_attempts WHERE logical=?1 AND state='unknown')",
    )
    .bind(&logical)
    .execute(&mut *db)
    .await?;
    let provider: Option<String> =
        sqlx::query_scalar("SELECT state FROM model_attempts WHERE logical=?")
            .bind(logical)
            .fetch_optional(db)
            .await?;
    Ok(match provider.as_deref() {
        Some("response") => state,
        Some("rejected") => "failed",
        None if state == "failed" => "failed",
        _ => "unknown",
    })
}

pub(super) async fn callback(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    method: &str,
    p: &Value,
) -> Result<Value> {
    grants(db, plugin, generation).await?;
    let handle = p["handle"].as_str().ok_or("missing tool handle")?;
    let row = sqlx::query("SELECT c.execution,c.operation,c.state,c.generation,coalesce(x.original_actor,x.origin) AS actor,
        coalesce(e.entry_id,origin.conversation) AS entry,json_extract(m.value,'$.external_search') AS policy,
        json_extract(a.value,'$.deployment') AS deployment FROM host_calls c
        JOIN operations o ON o.namespace=c.execution AND o.operation_id=c.operation
        JOIN host_installations i ON i.plugin=c.plugin AND i.revision=c.revision
        JOIN core_meta m ON m.key='plugin-install:'||c.plugin JOIN core_meta a ON a.key='plugin-approval:'||c.plugin
        JOIN executions x ON x.id=c.execution LEFT JOIN events e ON e.id=x.source LEFT JOIN main_origins origin ON origin.ref=x.origin
        WHERE c.handle=? AND c.plugin=? AND c.method='tool.call' AND o.target=json_extract(m.value,'$.external_search.tool')")
        .bind(handle).bind(plugin).fetch_one(&mut *db).await?;
    let exec: &str = row.try_get("execution")?;
    let logical = format!("external-search:{handle}");
    let prior = sqlx::query("SELECT t.id, json_extract(r.request_json,'$.deployment') AS deployment FROM model_requests r JOIN model_attempts t ON t.logical=r.id WHERE r.id=? AND t.ordinal=0").bind(&logical).fetch_optional(&mut *db).await?;
    let deployment: &str = row.try_get("deployment")?;
    let begin = method == "search.begin";
    let first = begin && prior.is_none();
    effects::authorize(db, exec, row.try_get("actor")?, first).await?;
    if (first || method == "search.finish")
        && (row.try_get::<&str, _>("generation")? != generation
            || row.try_get::<&str, _>("state")? != "dispatched")
    {
        return Err("original active invocation required".into());
    }
    let policy: Policy = serde_json::from_str(row.try_get("policy")?)?;
    let physical = match prior {
        Some(old) if old.try_get::<&str, _>("deployment")? == deployment => {
            old.try_get::<String, _>("id")?
        }
        None if first => {
            if !enabled(db).await? {
                return Err("external search disabled".into());
            }
            search::activate_child(db, exec, row.try_get("entry")?, &logical, deployment).await?;
            let request = json!({"provider":policy.provider,"purpose":policy.purpose,"endpoint":policy.endpoint,"deployment":deployment,"handle":handle,"operation":row.try_get::<&str,_>("operation")?});
            attempt::apply(
                db,
                &attempt::Action::Reserve(attempt::Reservation {
                    activation: logical.clone(),
                    logical: logical.clone(),
                    input: 0,
                    output: 0,
                    safe_rate_limit_once: false,
                    native_tools: false,
                    request: request.to_string(),
                }),
            )
            .await?;
            let id = new_id(db).await?;
            let permit = attempt::apply(
                db,
                &attempt::Action::Dispatch {
                    logical: logical.clone(),
                    attempt: id.clone(),
                },
            )
            .await?;
            if permit["dispatch_now"] != true {
                return Err("search attempt already dispatched".into());
            }
            id
        }
        _ => return Err("original paid search unavailable".into()),
    };
    if method == "search.finish" {
        if p["attempt"] != physical {
            return Err("wrong physical attempt".into());
        }
        finish(db, &physical, p).await?;
    }
    let t=sqlx::query("SELECT t.state,t.dispatched_at,t.received_at,t.status_code,t.response,t.usage_json,r.root FROM model_attempts t JOIN model_requests r ON r.id=t.logical WHERE t.id=?")
        .bind(&physical).fetch_one(db).await?;
    let usage = t
        .try_get::<Option<&str>, _>("usage_json")?
        .map(serde_json::from_str::<Value>)
        .transpose()?;
    Ok(
        json!({"handle":handle,"attempt":physical,"logical":logical,"root":t.try_get::<&str,_>("root")?,"provider":policy.provider,"purpose":policy.purpose,"endpoint":policy.endpoint,"dispatch_now":first,
        "state":t.try_get::<&str,_>("state")?,"dispatched_at":t.try_get::<&str,_>("dispatched_at")?,"received_at":t.try_get::<Option<&str>,_>("received_at")?,
        "status":t.try_get::<Option<i64>,_>("status_code")?,"response_sha256":t.try_get::<Option<&str>,_>("response")?, "usage":usage}),
    )
}

async fn finish(db: &mut SqliteConnection, physical: &str, p: &Value) -> Result<()> {
    let unknown = p["outcome"] == "unknown";
    let status = p["status"].as_i64();
    let state = if unknown {
        "unknown"
    } else if status.is_some_and(|s| (200..300).contains(&s)) {
        "response"
    } else {
        "rejected"
    };
    let digest = p["response_sha256"].as_str();
    let usage = p.get("usage").map(Value::to_string);
    // A repeated report must agree byte-for-byte with the first, including missing usage.
    let accepted=sqlx::query("UPDATE model_attempts SET state=?1,status_code=?2,response=?3,usage_json=?4,ever_unknown=?5,
        received_at=CASE WHEN ?5 THEN NULL ELSE coalesce(received_at,CURRENT_TIMESTAMP) END
        WHERE id=?6 AND (state='dispatched' OR (state=?1 AND status_code IS ?2 AND response IS ?3 AND usage_json IS ?4))")
        .bind(state).bind(status).bind(digest).bind(usage).bind(unknown).bind(physical).execute(&mut *db).await?;
    if accepted.rows_affected() != 1 {
        return Err("conflicting provider receipt".into());
    }
    sqlx::query("UPDATE executions SET state=? WHERE origin=(SELECT logical FROM model_attempts WHERE id=?) AND state='running'")
        .bind(if unknown { "held" } else { "completed" }).bind(physical).execute(db).await?;
    Ok(())
}
