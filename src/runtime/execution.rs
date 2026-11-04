use crate::{Result, runtime::effects::authorize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub async fn eligible(conn: &mut SqliteConnection, execution: &str) -> Result<String> {
    let actor: String =
        sqlx::query_scalar("SELECT coalesce(original_actor,origin) FROM executions WHERE id=?")
            .bind(execution)
            .fetch_one(&mut *conn)
            .await?;
    authorize(conn, execution, &actor, true).await
}

pub async fn root(
    conn: &mut SqliteConnection,
    execution: &str,
    slots: i64,
    units: i64,
) -> Result<Value> {
    eligible(conn, execution).await?;
    sqlx::query("INSERT INTO root_budgets(root,max_requests,max_units) VALUES(?,?,?) ON CONFLICT(root) DO NOTHING")
        .bind(execution).bind(slots).bind(units).execute(&mut *conn).await?;
    let old = sqlx::query("SELECT max_requests,max_units FROM root_budgets WHERE root=?")
        .bind(execution)
        .fetch_one(&mut *conn)
        .await?;
    if old.try_get::<i64, _>("max_requests")? != slots
        || old.try_get::<i64, _>("max_units")? != units
    {
        return Err("root budget cannot be replaced".into());
    }
    bind(conn, execution, execution).await
}

async fn bind(conn: &mut SqliteConnection, execution: &str, root: &str) -> Result<Value> {
    sqlx::query(
        "INSERT INTO budget_members(execution,root) VALUES(?,?) ON CONFLICT(execution) DO NOTHING",
    )
    .bind(execution)
    .bind(root)
    .execute(&mut *conn)
    .await?;
    let found: String = sqlx::query_scalar("SELECT root FROM budget_members WHERE execution=?")
        .bind(execution)
        .fetch_one(conn)
        .await?;
    if found != root {
        return Err("execution already belongs to another root".into());
    }
    Ok(json!({"execution":execution,"root":root}))
}

pub async fn child(conn: &mut SqliteConnection, execution: &str, parent: &str) -> Result<Value> {
    if eligible(conn, execution).await? != eligible(conn, parent).await? {
        return Err("different original execution authority".into());
    }
    let root: String = sqlx::query_scalar("SELECT root FROM budget_members WHERE execution=?")
        .bind(parent)
        .fetch_one(&mut *conn)
        .await?;
    bind(conn, execution, &root).await
}

pub async fn activate(
    conn: &mut SqliteConnection,
    execution: &str,
    id: &str,
    contract: &str,
) -> Result<Value> {
    eligible(conn, execution).await?;
    sqlx::query("INSERT INTO activations(id,execution,profile_contract) VALUES(?,?,?) ON CONFLICT(id) DO NOTHING")
        .bind(id).bind(execution).bind(contract).execute(&mut *conn).await?;
    let old = sqlx::query("SELECT execution,profile_contract FROM activations WHERE id=?")
        .bind(id)
        .fetch_one(conn)
        .await?;
    if old.try_get::<String, _>("execution")? != execution
        || old.try_get::<String, _>("profile_contract")? != contract
    {
        return Err("activation contract conflict".into());
    }
    Ok(json!({"activation":id,"execution":execution,"profile_contract":contract}))
}

pub async fn attach(conn: &mut SqliteConnection, execution: &str, work: &str) -> Result<Value> {
    eligible(conn, execution).await?;
    sqlx::query_scalar::<_,String>("UPDATE executions SET work_id=? WHERE id=? AND (work_id IS NULL OR work_id=?) RETURNING id")
        .bind(work).bind(execution).bind(work).fetch_one(conn).await?;
    Ok(json!({"execution":execution,"work":work}))
}
