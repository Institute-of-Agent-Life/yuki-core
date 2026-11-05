pub mod batch;
pub(crate) mod budget;
pub mod registry;
pub mod results;
pub mod transient;
use crate::{
    Result, history,
    runtime::effects,
    store::{Mutation, Store},
};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub struct Change {
    exec: String,
    actor: String,
    op: String,
    prepared: std::result::Result<Value, String>,
    artifact_version: Option<String>,
}

pub(crate) async fn settle(
    store: &Store,
    exec: &str,
    actor: &str,
    op: &str,
    value: Result<Value>,
) -> Result<()> {
    store
        .write(Mutation::Tool(Box::new(Change {
            exec: exec.into(),
            actor: actor.into(),
            op: op.into(),
            prepared: value.map_err(|e| e.to_string()),
            artifact_version: None,
        })))
        .await?;
    Ok(())
}
pub async fn reject(
    store: &Store,
    exec: &str,
    actor: &str,
    op: &str,
    reason: String,
) -> Result<()> {
    store
        .write(Mutation::Tool(Box::new(Change {
            exec: exec.into(),
            op: op.into(),
            actor: actor.into(),
            prepared: Err(reason),
            artifact_version: None,
        })))
        .await?;
    Ok(())
}
pub async fn perform(
    store: &Store,
    exec: &str,
    actor: &str,
    op: &str,
    name: &str,
    raw: &str,
) -> Result<()> {
    let args: Value = serde_json::from_str(raw)?;
    let mut tx = store.readers().begin().await?;
    effects::authorize(&mut tx, exec, actor, true).await?;
    let initial = effects::receipt(&mut tx, exec, op).await?;
    if initial["state"] != "intent" {
        return Ok(());
    }
    let cfg=sqlx::query("SELECT v.timezone,v.page_budget,v.code_projection,m.body FROM execution_tool_views v JOIN tool_manifests m USING(revision) WHERE v.execution=?")
        .bind(exec).fetch_one(&mut *tx).await?;
    let manifest: registry::Manifest = serde_json::from_str(cfg.try_get("body")?)?;
    let tool = manifest
        .0
        .get(name)
        .ok_or("tool absent from original execution")?;
    let timezone: String = cfg.try_get("timezone")?;
    let budget = usize::try_from(cfg.try_get::<i64, _>("page_budget")?)?;
    let cached = if tool.cacheable {
        sqlx::query_scalar::<_,String>("SELECT first.result FROM operations first JOIN operations current ON current.namespace=first.namespace AND current.operation_id=? WHERE first.namespace=? AND first.target=current.target AND first.payload=current.payload AND first.state='confirmed' AND first.rowid<current.rowid ORDER BY first.rowid LIMIT 1")
            .bind(op).bind(exec).fetch_optional(&mut *tx).await?
    } else {
        None
    };
    let mut artifact_version = None;
    let result = if let Some(original) = cached {
        Ok(serde_json::from_str(&original)?)
    } else {
        match name {
            "lookup_tools" => manifest.lookup(&args, cfg.try_get("code_projection")?),
            "time_get_current" => transient::clock(transient::now()?, &timezone),
            "update_short_state" => Ok(Value::Null),
            "history_directory" => history::query::directory(&mut tx, exec, actor).await,
            "find_contacts" | "find_conversations" => {
                history::query::contacts::read(&mut tx, exec, actor, name, &args).await
            }
            "read_tool_artifact" => {
                // Release the SQLite reader before CPU-heavy JSON decoding. Publication
                // resolves the original handle and both readers again after this await.
                let input = results::load(&mut tx, exec, actor, &args).await;
                drop(tx);
                match input {
                    Ok(snapshot) => {
                        artifact_version = Some(snapshot.hash.clone());
                        let mut shell = initial;
                        shell["state"] = json!("confirmed");
                        let overhead = shell.to_string().chars().count().saturating_sub(4);
                        tokio::task::spawn_blocking(move || {
                            snapshot.page(&args, budget.saturating_sub(overhead))
                        })
                        .await?
                    }
                    Err(e) => Err(e),
                }
            }
            _ => return Err("unknown native handler".into()),
        }
    };
    store
        .write(Mutation::Tool(Box::new(Change {
            exec: exec.into(),
            op: op.into(),
            actor: actor.into(),
            prepared: result.map_err(|e| e.to_string()),
            artifact_version,
        })))
        .await?;
    Ok(())
}

pub async fn apply(conn: &mut SqliteConnection, call: &Change) -> Result<Value> {
    let Change {
        exec,
        actor,
        op,
        prepared,
        artifact_version,
    } = call;
    finish_guarded(
        conn,
        exec,
        actor,
        op,
        prepared.clone(),
        artifact_version.as_deref(),
    )
    .await
}

pub(crate) async fn finish_guarded(
    conn: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    op: &str,
    prepared: std::result::Result<Value, String>,
    artifact_version: Option<&str>,
) -> Result<Value> {
    effects::authorize(conn, exec, actor, true).await?;
    let row=sqlx::query("SELECT target,payload,state FROM operations WHERE namespace=? AND operation_id=? AND action='external'").bind(exec).bind(op).fetch_one(&mut *conn).await?;
    if row.try_get::<String, _>("state")? != "intent" {
        return effects::receipt(conn, exec, op).await;
    }
    let name: String = row.try_get("target")?;
    let args: Value = serde_json::from_str(row.try_get("payload")?)?;
    let mut output = prepared;
    if output.is_ok() {
        match name.as_str() {
            "search_memory" => {
                output = crate::memory::search_tool::confirm(conn, exec, actor, &args, &output?)
                    .await
                    .map_err(|e| e.to_string());
            }
            "update_short_state" => {
                output = Ok(transient::update(conn, exec, &args, transient::now()?).await?);
            }
            "find_contacts" | "find_conversations" => {
                output = history::query::contacts::read(conn, exec, actor, &name, &args)
                    .await
                    .map_err(|e| e.to_string());
                if let Ok(value) = &output {
                    history::query::contacts::bind_sources(conn, exec, actor, value).await?;
                }
            }
            "history_directory" => {
                output = history::query::directory(conn, exec, actor)
                    .await
                    .map_err(|e| e.to_string());
            }
            "read_tool_artifact" => {
                match results::authorize(
                    conn,
                    exec,
                    actor,
                    args["handle"].as_str().ok_or("missing handle")?,
                    artifact_version
                        .or_else(|| args["version"].as_str())
                        .ok_or("missing artifact read version")?,
                )
                .await
                {
                    Err(_) => output = Err("artifact unavailable".into()),
                    Ok(producer) => {
                        sqlx::query("INSERT OR IGNORE INTO main_dependencies(execution,event) SELECT ?1,id FROM events WHERE id IN (SELECT source FROM executions WHERE id=?2 UNION SELECT event FROM main_dependencies WHERE execution=?2)").bind(exec).bind(producer).execute(&mut *conn).await?;
                    }
                }
            }
            "lookup_tools" => {
                // The manifest is immutable. Only this read can represent a later equal call.
                if let Some((representative,raw))=sqlx::query_as::<_,(String,String)>("SELECT first.operation_id,first.result FROM operations first JOIN operations current ON current.namespace=first.namespace AND current.operation_id=? WHERE first.namespace=? AND first.target=current.target AND first.payload=current.payload AND first.state='confirmed' AND first.rowid<current.rowid ORDER BY first.rowid LIMIT 1")
                    .bind(op).bind(exec).fetch_optional(&mut *conn).await?{
                    sqlx::query("INSERT INTO tool_aliases VALUES(?,?,?)").bind(exec).bind(op).bind(representative).execute(&mut *conn).await?;
                    output=Ok(serde_json::from_str(&raw)?);
                }
            }
            "time_get_current" => {}
            _ => return Err("unknown native tool".into()),
        }
    }
    let (state, result) = match output {
        Ok(v) => ("confirmed", v),
        Err(e) => ("failed", json!({"error":e})),
    };
    sqlx::query("UPDATE operations SET state=?,result=? WHERE namespace=? AND operation_id=?")
        .bind(state)
        .bind(result.to_string())
        .bind(exec)
        .bind(op)
        .execute(&mut *conn)
        .await?;
    effects::receipt(conn, exec, op).await
}

#[derive(clap::Subcommand, serde::Serialize, serde::Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Views {
        execution: String,
        actor: String,
    },
    ReadArtifact {
        execution: String,
        actor: String,
        args: String,
    },
}
pub async fn invoke(store: &Store, action: Action) -> Result<Value> {
    match action {
        Action::Views { execution, actor } => {
            let mut tx = store.readers().begin().await?;
            effects::authorize(&mut tx, &execution, &actor, false).await?;
            let (body,code,revision):(String,bool,String)=sqlx::query_as("SELECT m.body,v.code_projection,v.revision FROM execution_tool_views v JOIN tool_manifests m USING(revision) WHERE execution=?").bind(&execution).fetch_one(&mut *tx).await?;
            let m: registry::Manifest = serde_json::from_str(&body)?;
            Ok(
                json!({"revision":revision,"executable":m,"direct":m.declarations(code),"directory":m.lookup(&json!({}),code)?,"code_projection":code}),
            )
        }
        Action::ReadArtifact {
            execution,
            actor,
            args,
        } => {
            let params: Value = serde_json::from_str(&args)?;
            if !jsonschema::validator_for(&results::operator_schema())?.is_valid(&params) {
                return Err("invalid artifact request".into());
            }
            let mut tx = store.readers().begin().await?;
            let snapshot = results::load(&mut tx, &execution, &actor, &params).await?;
            let version = snapshot.hash.clone();
            let budget: i64 = sqlx::query_scalar(
                "SELECT page_budget FROM execution_tool_views WHERE execution=?",
            )
            .bind(&execution)
            .fetch_one(&mut *tx)
            .await?;
            drop(tx);
            let selection = params.clone();
            let result =
                tokio::task::spawn_blocking(move || snapshot.page(&selection, budget as usize))
                    .await??;
            let mut final_read = store.readers().begin().await?;
            results::authorize(
                &mut final_read,
                &execution,
                &actor,
                params["handle"].as_str().unwrap(),
                &version,
            )
            .await?;
            Ok(result)
        }
    }
}
