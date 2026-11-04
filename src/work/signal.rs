use super::tree;
use crate::{
    Result,
    runtime::effects,
    store::{Mutation, Store},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
#[derive(clap::Subcommand, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Status {
        execution: String,
        actor: String,
        work: String,
    },
    Suspend {
        execution: String,
        actor: String,
        work: String,
        reason: String,
    },
    Progress {
        execution: String,
        actor: String,
        operation: String,
        text: String,
    },
}
pub async fn invoke(store: &Store, act: Action) -> Result<Value> {
    if let Action::Status {
        execution,
        actor,
        work,
    } = act
    {
        let mut tx = store.readers().begin().await?;
        let s = tree::scope(&mut tx, &execution, &actor, false).await?;
        return super::wait::status(&mut tx, &s, &work).await;
    }
    store.write(Mutation::WorkSignal(act)).await
}
pub async fn apply(conn: &mut SqliteConnection, act: &Action) -> Result<Value> {
    match act {
        Action::Status { .. } => Err("status requires a read transaction".into()),
        Action::Suspend {
            execution,
            actor,
            work,
            reason,
        } => {
            if reason.trim().is_empty() {
                return Err("suspension reason required".into());
            }
            let s = tree::scope(conn, execution, actor, true).await?;
            tree::authorize(conn, &s, work).await?;
            sqlx::query("INSERT INTO work_suspensions SELECT id,? FROM work_nodes WHERE id=? AND status IN('active','need_input') ON CONFLICT(work) DO UPDATE SET reason=excluded.reason RETURNING work").bind(reason).bind(work).fetch_one(conn).await?;
            Ok(json!({"work_id":work,"suspended":true}))
        }
        Action::Progress {
            execution,
            actor,
            operation,
            text,
        } => {
            effects::authorize(conn, execution, actor, true).await?;
            let updates:i64=sqlx::query_scalar("INSERT INTO work_progress SELECT o.namespace,o.operation_id,?,1 FROM operations o JOIN work_runs r ON r.execution=o.namespace JOIN work_nodes w ON w.id=r.work WHERE o.namespace=? AND o.operation_id=? AND w.status='active' ON CONFLICT(execution,operation) DO UPDATE SET latest=excluded.latest,updates=work_progress.updates+1 RETURNING updates").bind(text).bind(execution).bind(operation).fetch_one(conn).await?;
            Ok(json!({"operation":operation,"updates":updates,"decision_requested":false}))
        }
    }
}
