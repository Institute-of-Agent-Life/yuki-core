use crate::{Result, runtime::effects};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub(crate) async fn root(db: &mut SqliteConnection, root: &str, limit: Option<i64>) -> Result<()> {
    sqlx::query(
        "INSERT INTO root_tool_budgets(root,max_calls) VALUES(?,?) ON CONFLICT(root) DO NOTHING",
    )
    .bind(root)
    .bind(limit)
    .execute(&mut *db)
    .await?;
    let original: Option<i64> =
        sqlx::query_scalar("SELECT max_calls FROM root_tool_budgets WHERE root=?")
            .bind(root)
            .fetch_one(db)
            .await?;
    if limit.is_some() && original != limit {
        return Err("original tool budget cannot be replaced".into());
    }
    Ok(())
}

/// The original logical operation reserves once, shared by direct and VM dispatch.
pub(crate) async fn charge(
    db: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    op: &str,
) -> Result<Value> {
    let row = sqlx::query("SELECT o.action,o.target,o.state,m.root FROM operations o JOIN budget_members m ON m.execution=o.namespace WHERE o.namespace=? AND o.operation_id=?")
        .bind(exec).bind(op).fetch_one(&mut *db).await?;
    if row.get::<&str, _>("state") != "intent" {
        return Ok(json!(true));
    }
    effects::authorize(db, exec, actor, true).await?;
    if let Some(accepted) = sqlx::query_scalar::<_, bool>(
        "SELECT accepted FROM tool_charges WHERE execution=? AND operation=?",
    )
    .bind(exec)
    .bind(op)
    .fetch_optional(&mut *db)
    .await?
    {
        return Ok(json!(accepted));
    }
    let action: &str = row.get("action");
    let target: &str = row.get("target");
    let free = action == "external"
        && matches!(
            target,
            "read_tool_artifact"
                | "task_control"
                | "subagent_start"
                | "subagent_control"
                | "complete"
        );
    let composition = if action == "external" && !free {
        sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM execution_tool_views v JOIN tool_manifests m USING(revision),json_each(m.body) t JOIN code_contracts c ON c.plugin=json_extract(t.value,'$.provider.id') AND c.revision=json_extract(t.value,'$.provider.revision') WHERE v.execution=? AND t.key=? AND json_extract(c.body,'$.tool')=t.key)")
            .bind(exec).bind(target).fetch_one(&mut *db).await?
    } else {
        false
    };
    let charged = !free && !composition;
    let root: String = row.get("root");
    let accepted = !charged || sqlx::query("UPDATE root_tool_budgets SET used_calls=used_calls+1 WHERE root=? AND (max_calls IS NULL OR used_calls<max_calls)")
        .bind(&root).execute(&mut *db).await?.rows_affected() == 1;
    sqlx::query(
        "INSERT INTO tool_charges VALUES(?,?,?,?,?,CAST(unixepoch('subsec')*1000 AS INTEGER))",
    )
    .bind(exec)
    .bind(op)
    .bind(root)
    .bind(charged && accepted)
    .bind(accepted)
    .execute(&mut *db)
    .await?;
    if !accepted {
        sqlx::query("UPDATE operations SET state='failed',result=? WHERE namespace=? AND operation_id=? AND state='intent'")
            .bind(json!({"error":"root_tool_budget_exhausted","dispatched":false}).to_string()).bind(exec).bind(op).execute(db).await?;
    }
    Ok(json!(accepted))
}
