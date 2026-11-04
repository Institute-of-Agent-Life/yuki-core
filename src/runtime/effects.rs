use crate::{
    Result,
    store::{Mutation, Store},
};
use clap::{Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

#[derive(Clone, Copy, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Get,
    Put,
    External,
}
impl Operation {
    fn name(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::Put => "put",
            Self::External => "external",
        }
    }
}

#[derive(Clone, Copy, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Confirmed,
    Failed,
    Unknown,
}
impl Outcome {
    fn name(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Subcommand, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Intent {
        execution: String,
        id: String,
        actor: String,
        action: Operation,
        target: String,
        payload: String,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        ordinal: Option<i64>,
    },
    Local {
        execution: String,
        id: String,
        actor: String,
    },
    Dispatch {
        execution: String,
        id: String,
        actor: String,
    },
    /// Owner reconciliation. Recording a receipt does not grant access to its body.
    Settle {
        execution: String,
        id: String,
        outcome: Outcome,
        result: String,
    },
    Read {
        execution: String,
        id: String,
        actor: String,
    },
    Value {
        execution: String,
        actor: String,
        target: String,
    },
}

pub async fn invoke(store: &Store, action: Action) -> Result<Value> {
    match action {
        Action::Read {
            execution,
            id,
            actor,
        } => {
            let mut tx = store.readers().begin().await?;
            authorize(&mut tx, &execution, &actor, false).await?;
            receipt(&mut tx, &execution, &id).await
        }
        Action::Value {
            execution,
            actor,
            target,
        } => {
            let mut tx = store.readers().begin().await?;
            let original = authorize(&mut tx, &execution, &actor, false).await?;
            value(&mut tx, &original, &target).await
        }
        command => store.write(Mutation::Effect(command)).await,
    }
}

/// One SQLite snapshot covers permission and result retrieval. Dispatch also requires the
/// original route and both original assignment stages to still be live.
pub(crate) async fn authorize(
    conn: &mut SqliteConnection,
    execution: &str,
    actor: &str,
    dispatch: bool,
) -> Result<String> {
    let row=sqlx::query("SELECT p.id AS original,(p.enabled AND root.enabled AND requester.enabled AND
        coalesce(requester.current_root,requester.id)=root.id) AS readable,
        (x.cancelled=0 AND route.closed_at IS NULL AND a.status='active' AND b.status='active') AS routable
        FROM executions x JOIN events e ON e.id=x.source JOIN entries route ON route.id=e.entry_id
        JOIN persons p ON p.id=x.original_actor JOIN persons root ON root.id=coalesce(p.current_root,p.id)
        JOIN persons requester ON requester.id=? JOIN assignments a ON a.id=e.actor_stage
        JOIN assignments b ON b.id=e.bot_stage WHERE x.id=?")
        .bind(actor).bind(execution).fetch_one(conn).await?;
    let original: String = row.try_get("original")?;
    if !row.try_get::<bool, _>("readable")?
        || (dispatch && (actor != original || !row.try_get::<bool, _>("routable")?))
    {
        return Err("current original authority denied".into());
    }
    Ok(original)
}

pub(crate) async fn receipt(
    conn: &mut SqliteConnection,
    execution: &str,
    id: &str,
) -> Result<Value> {
    let row = sqlx::query(
        "SELECT state,result,parent,ordinal FROM operations WHERE namespace=? AND operation_id=?",
    )
    .bind(execution)
    .bind(id)
    .fetch_one(conn)
    .await?;
    let result: Option<String> = row.try_get("result")?;
    Ok(
        json!({"namespace":execution,"id":id,"state":row.try_get::<String,_>("state")?,
        "parent":row.try_get::<Option<String>,_>("parent")?,"ordinal":row.try_get::<Option<i64>,_>("ordinal")?,
        "result":result.as_deref().map(serde_json::from_str::<Value>).transpose()?}),
    )
}

async fn value(conn: &mut SqliteConnection, original: &str, target: &str) -> Result<Value> {
    let row = sqlx::query(
        "SELECT value,revision FROM local_effect_values WHERE original_actor=? AND target=?",
    )
    .bind(original)
    .bind(target)
    .fetch_optional(conn)
    .await?;
    match row {
        Some(row) => Ok(
            json!({"value":serde_json::from_str::<Value>(row.try_get("value")?)?,
                             "revision":row.try_get::<i64,_>("revision")?}),
        ),
        None => Ok(Value::Null),
    }
}

pub async fn apply(conn: &mut SqliteConnection, command: &Action) -> Result<Value> {
    match command {
        Action::Intent {
            execution,
            id,
            actor,
            action,
            target,
            payload,
            parent,
            ordinal,
        } => {
            authorize(conn, execution, actor, false).await?;
            let canonical = serde_json::from_str::<Value>(payload)?.to_string();
            if let Some(old)=sqlx::query("SELECT action,target,payload,parent,ordinal FROM operations WHERE namespace=? AND operation_id=?")
                .bind(execution).bind(id).fetch_optional(&mut *conn).await?{
                if old.try_get::<String,_>("action")?!=action.name() || old.try_get::<String,_>("target")?!=*target ||
                   old.try_get::<String,_>("payload")?!=canonical || old.try_get::<Option<String>,_>("parent")?!=*parent ||
                   old.try_get::<Option<i64>,_>("ordinal")?!=*ordinal {return Err("original operation conflicts with new content".into());}
            }else{
                authorize(conn,execution,actor,true).await?;
                sqlx::query("INSERT INTO operations(namespace,operation_id,action,target,payload,parent,ordinal,state)
                    VALUES(?,?,?,?,?,?,?,'intent')").bind(execution).bind(id).bind(action.name()).bind(target)
                    .bind(canonical).bind(parent).bind(ordinal).execute(&mut *conn).await?;
            }
            receipt(conn, execution, id).await
        }
        Action::Local {
            execution,
            id,
            actor,
        } => {
            let original = authorize(conn, execution, actor, false).await?;
            let op=sqlx::query("SELECT action,target,payload,state FROM operations WHERE namespace=? AND operation_id=?")
                .bind(execution).bind(id).fetch_one(&mut *conn).await?;
            let action: String = op.try_get("action")?;
            if action == "external" {
                return Err("external operation has no local action".into());
            }
            if op.try_get::<String, _>("state")? == "intent" {
                authorize(conn, execution, actor, true).await?;
                let target: String = op.try_get("target")?;
                if action == "put" {
                    sqlx::query("INSERT INTO local_effect_values(original_actor,target,value,revision) VALUES(?,?,?,1)
                        ON CONFLICT(original_actor,target) DO UPDATE SET value=excluded.value,revision=local_effect_values.revision+1")
                        .bind(&original).bind(&target).bind(op.try_get::<String,_>("payload")?).execute(&mut *conn).await?;
                }
                let result = value(conn, &original, &target).await?;
                sqlx::query("UPDATE operations SET state='confirmed',result=? WHERE namespace=? AND operation_id=?")
                    .bind(result.to_string()).bind(execution).bind(id).execute(&mut *conn).await?;
            }
            receipt(conn, execution, id).await
        }
        Action::Dispatch {
            execution,
            id,
            actor,
        } => {
            authorize(conn, execution, actor, true).await?;
            let op = sqlx::query(
                "SELECT action,state FROM operations WHERE namespace=? AND operation_id=?",
            )
            .bind(execution)
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            if op.try_get::<String, _>("action")? != "external" {
                return Err("only external intents cross the boundary".into());
            }
            let prior: String = op.try_get("state")?;
            let grant = prior == "intent";
            if grant {
                sqlx::query("UPDATE operations SET state='dispatched' WHERE namespace=? AND operation_id=? AND state='intent'")
                    .bind(execution).bind(id).execute(conn).await?;
            }
            Ok(
                json!({"namespace":execution,"id":id,"state":if grant{"dispatched"}else{&prior},"dispatch_now":grant}),
            )
        }
        Action::Settle {
            execution,
            id,
            outcome,
            result,
        } => {
            let result = serde_json::from_str::<Value>(result)?.to_string();
            let row = sqlx::query(
                "SELECT action,state,result FROM operations WHERE namespace=? AND operation_id=?",
            )
            .bind(execution)
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
            let old: String = row.try_get("state")?;
            if row.try_get::<String, _>("action")? != "external" {
                return Err("cannot settle a local operation across a boundary".into());
            }
            if old == outcome.name()
                && row.try_get::<Option<String>, _>("result")?.as_deref() == Some(result.as_str())
            {
                return Ok(json!({"id":id,"state":old}));
            }
            if !matches!(old.as_str(), "dispatched" | "unknown") || old == outcome.name() {
                return Err("receipt conflicts with the original state or result".into());
            }
            sqlx::query(
                "UPDATE operations SET state=?,result=? WHERE namespace=? AND operation_id=?",
            )
            .bind(outcome.name())
            .bind(result)
            .bind(execution)
            .bind(id)
            .execute(conn)
            .await?;
            Ok(json!({"id":id,"state":outcome.name()}))
        }
        Action::Read { .. } | Action::Value { .. } => {
            Err("read command belongs on the reader".into())
        }
    }
}
