mod control;
mod mailbox;
mod note;
pub(crate) mod questions;
pub mod signal;
mod tree;
pub(crate) mod wait;
use crate::{
    Result,
    runtime::{effects, main::Config},
    store::{Mutation, Store},
    tools::registry::Tool,
};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::sync::LazyLock;

static DECLARATIONS: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("work/v4-declarations.json"))
        .expect("original Work tool declarations")
});
pub fn declaration(name: &str) -> Option<Tool> {
    let spec = DECLARATIONS["tools"]
        .as_array()?
        .iter()
        .find(|v| v["name"] == name)?;
    let schema = spec["schema"].clone();
    let description = spec["description"].as_str()?.to_owned();
    Some(Tool {
        name: name.into(),
        description,
        schema,
        cacheable: false,
        parallel_read: false,
        code_direct: true,
        provider: None,
    })
}
pub struct Call {
    cfg: Config,
    exec: String,
    actor: String,
    op: String,
}
pub async fn perform(store: &Store, cfg: &Config, exec: &str, actor: &str, op: &str) -> Result<()> {
    store
        .write(Mutation::Work(Box::new(Call {
            cfg: cfg.clone(),
            exec: exec.into(),
            actor: actor.into(),
            op: op.into(),
        })))
        .await?;
    Ok(())
}
pub async fn apply(conn: &mut SqliteConnection, call: &Call) -> Result<Value> {
    let Call {
        cfg,
        exec,
        actor,
        op,
    } = call;
    effects::authorize(conn, exec, actor, false).await?;
    let row=sqlx::query("SELECT target,payload,state FROM operations WHERE namespace=? AND operation_id=? AND action='external'").bind(exec).bind(op).fetch_one(&mut *conn).await?;
    if row.try_get::<String, _>("state")? != "intent" {
        return effects::receipt(conn, exec, op).await;
    }
    let scope = tree::scope(conn, exec, actor, true).await?;
    let args = serde_json::from_str(row.try_get("payload")?)?;
    sqlx::query("SAVEPOINT work_action")
        .execute(&mut *conn)
        .await?;
    let result = control::run(conn, cfg, &scope, exec, op, row.try_get("target")?, &args).await;
    let (status, body) = match result {
        Ok(v) => ("confirmed", v),
        Err(e) => {
            sqlx::query("ROLLBACK TO work_action")
                .execute(&mut *conn)
                .await?;
            ("failed", json!({"error":e.to_string()}))
        }
    };
    sqlx::query("RELEASE work_action")
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE operations SET state=?,result=? WHERE namespace=? AND operation_id=? AND state='intent'").bind(status).bind(body.to_string()).bind(exec).bind(op).execute(&mut *conn).await?;
    effects::receipt(conn, exec, op).await
}

pub async fn bound(conn: &mut SqliteConnection, exec: &str) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT work FROM work_runs WHERE execution=?")
            .bind(exec)
            .fetch_optional(conn)
            .await?,
    )
}
pub async fn context(conn: &mut SqliteConnection, exec: &str) -> Result<Value> {
    let Some(id) = bound(conn, exec).await? else {
        return Ok(json!({"state":"no_active_work","state_scope":"current_activation"}));
    };
    let row=sqlx::query("SELECT w.id,w.root,w.parent,w.goal,w.note,w.status,r.kind FROM work_nodes w JOIN work_runs r ON r.work=w.id WHERE r.execution=? AND w.goal IS NOT NULL")
        .bind(exec).fetch_one(&mut *conn).await?;
    let mut context = json!({"state_scope":"current_activation","work_id":id,"root_work_id":row.try_get::<String,_>("root")?,"parent_work_id":row.try_get::<Option<String>,_>("parent")?,"goal":row.try_get::<String,_>("goal")?,
        "kind":row.try_get::<String,_>("kind")?,"state":row.try_get::<String,_>("status")?,"context_note":row.try_get::<Option<String>,_>("note")?.as_deref().map(serde_json::from_str::<Value>).transpose()?});
    crate::context::compact_exec::restore_work(conn, exec, &id, &mut context).await?;
    Ok(context)
}
pub async fn private_input(conn: &mut SqliteConnection, exec: &str) -> Result<Vec<Value>> {
    match bound(conn, exec).await? {
        Some(work) => mailbox::consume(conn, &work, exec).await,
        None => Ok(Vec::new()),
    }
}
pub async fn has_mail(conn: &mut SqliteConnection, exec: &str) -> Result<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM work_runs r JOIN work_mail m ON m.work=r.work WHERE r.execution=? AND m.body IS NOT NULL AND m.id>coalesce((SELECT watermark FROM work_consumption WHERE work=r.work),0))").bind(exec).fetch_one(conn).await?)
}
pub async fn settle(
    conn: &mut SqliteConnection,
    exec: &str,
    logical: &str,
    final_result: Option<&Value>,
) -> Result<bool> {
    let Some(id) = bound(conn, exec).await? else {
        return Ok(false);
    };
    let proposal=sqlx::query("SELECT operation,summary,kind FROM work_proposals WHERE execution=? AND logical=? ORDER BY rowid DESC LIMIT 1").bind(exec).bind(logical).fetch_optional(&mut *conn).await?;
    let (op, summary, kind) = if let Some(row) = proposal {
        (
            row.try_get("operation")?,
            row.try_get::<Option<String>, _>("summary")?
                .unwrap_or_default(),
            row.try_get::<String, _>("kind")?,
        )
    } else if let Some(result) = final_result {
        (
            logical.to_owned(),
            result
                .as_str()
                .or(result["content"].as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    if result["role"] == "assistant" && result["content"].is_null() {
                        String::new()
                    } else {
                        result.to_string()
                    }
                }),
            "completed".into(),
        )
    } else {
        return Ok(false);
    };
    tree::terminal(conn, &id, exec, &op, &kind, &summary).await?;
    if kind == "need_input"
        && !sqlx::query_scalar::<_, bool>("SELECT parent IS NOT NULL FROM work_nodes WHERE id=?")
            .bind(&id)
            .fetch_one(&mut *conn)
            .await?
    {
        questions::register(conn, &id, exec).await?;
    }
    sqlx::query("UPDATE main_turns SET result=? WHERE execution=?")
        .bind(json!(summary).to_string())
        .bind(exec)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE executions SET state='completed' WHERE id=?")
        .bind(exec)
        .execute(conn)
        .await?;
    Ok(true)
}
