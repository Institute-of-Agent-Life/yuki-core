mod child;
mod snapshot;

use super::{calls, domain, grants, write};
use crate::{Result, privacy::source, runtime::effects, store::Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

#[derive(Debug)]
pub(crate) struct Unavailable(pub &'static str);
impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Unavailable {}

pub(crate) async fn check_runtime(store: &Store, exec: &str, op: &str) -> Result<()> {
    let available:Option<bool>=sqlx::query_scalar("SELECT i.enabled AND i.online AND i.revision=c.revision FROM code_parents p JOIN host_calls c ON c.handle=p.handle JOIN host_installations i ON i.plugin=c.plugin WHERE p.execution=? AND p.operation=?")
        .bind(exec).bind(op).fetch_optional(store.readers()).await?;
    if available == Some(false) {
        return Err(Box::new(Unavailable("code_original_runtime_unavailable")));
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    pub tool: String,
    pub abi: String,
    pub engine_sha256: String,
    pub codec_sha256: String,
    pub launcher_sha256: String,
    #[serde(default = "snapshot_limit")]
    pub max_snapshot_bytes: usize,
    #[serde(default = "pending_limit")]
    pub max_pending_futures: i64,
    #[serde(default = "boundary_limit")]
    pub max_suspensions: i64,
    #[serde(default = "request_timeout")]
    pub request_timeout_seconds: f64,
}
fn snapshot_limit() -> usize {
    8 * 1024 * 1024
}
fn pending_limit() -> i64 {
    16
}
fn request_timeout() -> f64 {
    30.0
}
fn boundary_limit() -> i64 {
    1024
}
impl Runtime {
    pub(super) fn validate(&self, install: &grants::Install) -> Result<()> {
        let digest = |s: &str| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if self.abi.is_empty()
            || self.abi.len() > 128
            || ![
                &self.engine_sha256,
                &self.codec_sha256,
                &self.launcher_sha256,
            ]
            .into_iter()
            .all(|s| digest(s))
            || !(65536..=128 * 1024 * 1024).contains(&self.max_snapshot_bytes)
            || !(1..=16).contains(&self.max_pending_futures)
            || self.max_suspensions <= 0
            || !self.request_timeout_seconds.is_finite()
            || self.request_timeout_seconds <= 0.0
            || self.request_timeout_seconds > 600.0
            || !install
                .tools
                .iter()
                .any(|t| t.name == self.tool && t.code_direct)
            || ![
                "tool.call",
                "composition.read",
                "composition.execute",
                "forget.owner",
            ]
            .into_iter()
            .all(|g| install.grants.contains(g))
        {
            return Err("Code requires a pinned runtime, explicit tool and scoped grants".into());
        }
        Ok(())
    }
}

pub struct Callback {
    plugin: String,
    generation: String,
    method: String,
    params: Value,
    prepared: Option<snapshot::Prepared>,
}
pub enum Change {
    Callback(Callback),
    Enter(String, String, String, i64),
    Capture(String, i64),
    Rebind(String, String),
}
struct Parent {
    handle: String,
    exec: String,
    operation: String,
    actor: String,
    runtime: Runtime,
    checkpoint: i64,
    next_ordinal: i64,
    feed_index: i64,
}

pub(super) async fn configure(db: &mut SqliteConnection, install: &grants::Install) -> Result<()> {
    if let Some(runtime) = &install.composition {
        sqlx::query("INSERT INTO code_contracts VALUES(?,?,?) ON CONFLICT DO NOTHING")
            .bind(&install.id)
            .bind(install.revision()?)
            .bind(serde_json::to_string(runtime)?)
            .execute(db)
            .await?;
    }
    Ok(())
}

pub(super) async fn open(
    db: &mut SqliteConnection,
    r: &calls::Request,
    handle: &str,
) -> Result<()> {
    let contract: Option<String> = sqlx::query_scalar("SELECT body FROM code_contracts WHERE plugin=? AND revision=? AND json_extract(body,'$.tool')=?")
        .bind(&r.plugin).bind(&r.revision).bind(&r.name).fetch_optional(&mut *db).await?;
    let Some(contract) = contract else {
        return Ok(());
    };
    if crate::work::bound(db, &r.execution).await?.is_none() {
        return Err("Code composition requires an accepted Work".into());
    }
    sqlx::query("INSERT INTO code_parents(handle,execution,operation,contract) VALUES(?,?,?,?) ON CONFLICT(handle) DO NOTHING")
        .bind(handle).bind(&r.execution).bind(&r.operation).bind(contract).execute(&mut *db).await?;
    let sources: Vec<i64> = sqlx::query_scalar("SELECT id FROM events WHERE id IN(SELECT source FROM executions WHERE id=?1 UNION SELECT event FROM main_dependencies WHERE execution=?1)")
        .bind(&r.execution).fetch_all(&mut *db).await?;
    for event in sources {
        source::acquire(db, &r.plugin, handle, event).await?;
    }
    Ok(())
}

async fn parent(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    handle: &str,
    mutate: bool,
) -> Result<Parent> {
    domain::active(
        db,
        plugin,
        generation,
        if mutate {
            "composition.execute"
        } else {
            "composition.read"
        },
    )
    .await?;
    let r=sqlx::query("SELECT p.*,coalesce(x.original_actor,x.origin) AS actor,c.generation,c.revision,i.revision AS installed FROM code_parents p JOIN host_calls c ON c.handle=p.handle JOIN executions x ON x.id=p.execution JOIN host_installations i ON i.plugin=c.plugin WHERE p.handle=? AND c.plugin=?")
        .bind(handle).bind(plugin).fetch_one(&mut *db).await?;
    if r.get::<&str, _>("generation") != generation
        || r.get::<&str, _>("revision") != r.get::<&str, _>("installed")
        || mutate && r.get::<bool, _>("closed")
    {
        return Err("original Code generation or contract unavailable".into());
    }
    let exec: String = r.get("execution");
    let actor: String = r.get("actor");
    effects::authorize(db, &exec, &actor, mutate).await?;
    let operation: String = r.get("operation");
    if mutate && sqlx::query_scalar::<_,bool>("WITH RECURSIVE ancestors(op) AS(SELECT ?2 UNION SELECT o.parent FROM operations o JOIN ancestors a ON a.op=o.operation_id WHERE o.namespace=?1 AND o.parent IS NOT NULL) SELECT EXISTS(SELECT 1 FROM ancestors a LEFT JOIN model_tool_intents i ON i.execution=?1 AND i.operation_id=a.op LEFT JOIN code_parents p ON p.execution=?1 AND p.operation=a.op WHERE i.paired_receipt IS NOT NULL OR p.closed=1)")
        .bind(&exec).bind(&operation).fetch_one(&mut *db).await? {
        return Err("original composition already paired".into());
    }
    Ok(Parent {
        handle: handle.into(),
        exec,
        operation,
        actor,
        runtime: serde_json::from_str(r.get("contract"))?,
        checkpoint: r.get("checkpoint"),
        next_ordinal: r.get("next_ordinal"),
        feed_index: r.get("feed_index"),
    })
}

pub(super) fn handles(method: &str) -> bool {
    matches!(
        method,
        "tool.catalog"
            | "code.sources"
            | "tool.schema"
            | "tool.invoke"
            | "tool.resume"
            | "code.snapshot.put"
            | "code.snapshot.get"
    )
}
pub(super) async fn callback(
    store: &Store,
    plugin: &str,
    generation: &str,
    method: &str,
    params: Value,
) -> Result<Value> {
    grants::schema(method, "plugin_to_host", &params)?;
    let prepared =
        if method == "tool.invoke" || method == "tool.resume" && params["ack"].is_object() {
            snapshot::prepare(store, plugin, generation, &params).await?
        } else {
            None
        };
    let value = write(
        store,
        super::Change::Composition(Box::new(Change::Callback(Callback {
            plugin: plugin.into(),
            generation: generation.into(),
            method: method.into(),
            params: params.clone(),
            prepared,
        }))),
    )
    .await?;
    if method == "tool.invoke" {
        #[cfg(test)]
        crate::store::tests::commit_barrier("code-T1-committed").await;
    }
    if (method == "tool.invoke" || method == "tool.resume" && params["ack"].is_null())
        && value["closed"] != true
    {
        return child::drive(
            store,
            plugin,
            generation,
            params["handle"].as_str().ok_or("missing parent")?,
            params["ordinal"].as_i64().ok_or("missing ordinal")?,
        )
        .await;
    }
    #[cfg(test)]
    if method == "tool.resume" {
        crate::store::tests::commit_barrier("code-feed-committed").await;
    }
    Ok(value)
}

pub(super) async fn apply(db: &mut SqliteConnection, change: &Change) -> Result<Value> {
    match change {
        Change::Callback(c) => {
            let def = grants::schema(&c.method, "plugin_to_host", &c.params)?;
            domain::active(
                db,
                &c.plugin,
                &c.generation,
                def["grant"].as_str().ok_or("missing grant")?,
            )
            .await?;
            let read = matches!(
                c.method.as_str(),
                "tool.catalog" | "tool.schema" | "code.snapshot.get" | "code.sources"
            ) || c.method == "tool.resume" && c.params["ack"].is_null();
            let p = parent(
                db,
                &c.plugin,
                &c.generation,
                c.params["handle"].as_str().ok_or("missing parent")?,
                !read,
            )
            .await?;
            let result = match c.method.as_str() {
                "tool.catalog" | "tool.schema" => {
                    let raw:String=sqlx::query_scalar("SELECT m.body FROM tool_manifests m JOIN execution_tool_views v USING(revision) WHERE v.execution=?").bind(&p.exec).fetch_one(&mut *db).await?;
                    let manifest: crate::tools::registry::Manifest = serde_json::from_str(&raw)?;
                    let mut args = c.params.clone();
                    args.as_object_mut()
                        .ok_or("invalid object")?
                        .remove("handle");
                    manifest.lookup(&args, true)?
                }
                "code.sources" => {
                    let limit = c.params["limit"].as_i64().unwrap_or(64);
                    let sources:Vec<i64>=sqlx::query_scalar("SELECT id FROM events WHERE id>?2 AND id IN(SELECT source FROM executions WHERE id=?1 UNION SELECT event FROM main_dependencies WHERE execution=?1) ORDER BY id LIMIT ?3")
                        .bind(&p.exec).bind(c.params["cursor"].as_i64().unwrap_or(0)).bind(limit+1).fetch_all(&mut *db).await?;
                    let more = sources.len() > limit as usize;
                    let page: Vec<i64> = sources.into_iter().take(limit as usize).collect();
                    json!({"next_cursor":if more{page.last().copied()}else{None},"sources":page})
                }
                "code.snapshot.put" => snapshot::put(db, &p, &c.params).await?,
                "code.snapshot.get" => snapshot::get(db, &p, &c.params).await?,
                "tool.invoke" => child::intent(db, &p, &c.params, c.prepared.as_ref()).await?,
                "tool.resume" => child::ack(db, &p, &c.params, c.prepared.as_ref()).await?,
                _ => return Err("unknown composition method".into()),
            };
            if !jsonschema::validator_for(&def["result"])?.is_valid(&result) {
                return Err("Code result violates schema".into());
            }
            Ok(result)
        }
        Change::Enter(plugin, generation, handle, ordinal) => {
            let p = parent(db, plugin, generation, handle, true).await?;
            child::enter(db, &p, generation, *ordinal).await
        }
        Change::Capture(handle, ordinal) => child::capture(db, handle, *ordinal).await,
        Change::Rebind(handle, generation) => child::rebind(db, handle, generation).await,
    }
}

pub(super) async fn finished(
    db: &mut SqliteConnection,
    handle: &str,
    body: &mut Value,
) -> Result<()> {
    if sqlx::query("UPDATE code_parents SET closed=1 WHERE handle=?")
        .bind(handle)
        .execute(&mut *db)
        .await?
        .rows_affected()
        == 0
    {
        return Ok(());
    }
    sqlx::query("DELETE FROM code_snapshot_chunks WHERE handle=? AND id IN(SELECT id FROM code_snapshots WHERE handle=? AND NOT complete)").bind(handle).bind(handle).execute(&mut *db).await?;
    sqlx::query("DELETE FROM code_snapshots WHERE handle=? AND NOT complete")
        .bind(handle)
        .execute(&mut *db)
        .await?;
    let counts=sqlx::query("SELECT count(*) AS total,coalesce(sum(fed_snapshot IS NULL),0) AS unfed FROM code_children WHERE handle=?").bind(handle).fetch_one(&mut *db).await?;
    let total: i64 = counts.get("total");
    let complete = counts.get::<i64, _>("unfed") == 0;
    let children = child::summary(db, handle, false).await?;
    body["composition"] = json!({"status":if complete{"completed"}else{"partial"},"child_count":total,"has_more":total>children.len() as i64,"children":children});
    sqlx::query(
        "UPDATE code_resume_attempts SET state='finished' WHERE handle=? AND state='dispatched'",
    )
    .bind(handle)
    .execute(db)
    .await?;
    Ok(())
}
