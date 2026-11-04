use crate::{
    Result,
    identity::new_id,
    runtime::{effects, execution},
    store::{Mutation, Store},
};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::collections::HashSet;

#[derive(clap::Args, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reservation {
    pub(crate) activation: String,
    pub(crate) logical: String,
    pub(crate) input: i64,
    pub(crate) output: i64,
    #[arg(long)]
    pub(crate) safe_rate_limit_once: bool,
    #[arg(long)]
    pub(crate) native_tools: bool,
    #[arg(long)]
    pub(crate) request: String,
}

#[derive(Subcommand, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Root {
        execution: String,
        slots: i64,
        units: i64,
    },
    Child {
        execution: String,
        parent: String,
    },
    Activate {
        execution: String,
        id: String,
        profile: String,
    },
    AttachWork {
        execution: String,
        work: String,
    },
    Cancel {
        execution: String,
    },
    Reserve(Reservation),
    Dispatch {
        logical: String,
        attempt: String,
    },
    Unknown {
        attempt: String,
    },
    Hold {
        attempt: String,
        body: String,
        usage: String,
        #[arg(skip)]
        partial_bytes: Option<Vec<u8>>,
        diagnostic: String,
        #[arg(action=clap::ArgAction::Set)]
        unknown: bool,
        status: i64,
    },
    Rejected {
        attempt: String,
        status: i64,
        #[arg(long)]
        unbilled_before_model: bool,
        body: String,
        usage: String,
    },
    Response {
        attempt: String,
        body: String,
        usage: String,
        intents: String,
    },
    Pair {
        logical: String,
        ordinal: i64,
    },
    Usage {
        root: String,
    },
    Inspect {
        logical: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tool {
    call_id: String,
    action: effects::Operation,
    target: String,
    payload: Value,
}

/// A new exclusive owner cannot know whether the previous HTTP write reached a provider.
pub async fn recover_unanswered(conn: &mut SqliteConnection) -> Result<()> {
    sqlx::query(
        "UPDATE model_attempts SET state='unknown',ever_unknown=1 WHERE state='dispatched'",
    )
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn invoke(store: &Store, action: Action) -> Result<Value> {
    match action {
        Action::Usage { root } => {
            let mut tx = store.readers().begin().await?;
            usage(&mut tx, &root).await
        }
        Action::Inspect { logical } => {
            let mut tx = store.readers().begin().await?;
            inspect(&mut tx, &logical).await
        }
        other => store.write(Mutation::Model(Box::new(other))).await,
    }
}

async fn reserve(conn: &mut SqliteConnection, reservation: &Reservation) -> Result<Value> {
    let activation = reservation.activation.as_str();
    let logical = reservation.logical.as_str();
    let (input, output) = (reservation.input, reservation.output);
    let (safe, native) = (reservation.safe_rate_limit_once, reservation.native_tools);
    let body = valid_json(&reservation.request)?;
    if !body.is_object() {
        return Err("a logical model request must be an object".into());
    }
    let context = sqlx::query(
        "SELECT a.execution, m.root, e.work_id FROM activations a
         JOIN budget_members m ON m.execution=a.execution
         JOIN executions e ON e.id=a.execution WHERE a.id=?",
    )
    .bind(activation)
    .fetch_one(&mut *conn)
    .await?;
    let exec: String = context.try_get("execution")?;
    execution::eligible(conn, &exec).await?;
    let root: String = context.try_get("root")?;
    let units = input
        .checked_add(output)
        .filter(|_| input >= 0 && output >= 0)
        .ok_or("invalid requested units")?;
    let existing = sqlx::query(
        "SELECT activation, estimated_input, max_output, retry_unbilled, native_tools, request_json
         FROM model_requests WHERE id=?",
    )
    .bind(logical)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(old) = existing {
        let same = old.try_get::<String, _>("activation")? == activation
            && old.try_get::<i64, _>("estimated_input")? == input
            && old.try_get::<i64, _>("max_output")? == output
            && old.try_get::<bool, _>("retry_unbilled")? == safe
            && old.try_get::<bool, _>("native_tools")? == native
            && valid_json(old.try_get("request_json")?)? == body;
        if !same {
            return Err("logical request conflicts with original reservation".into());
        }
    } else {
        if safe && native {
            return Err("native-tool call cannot opt into an unbilled retry".into());
        }
        let unresolved: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM model_requests prior
             JOIN activations previous ON previous.id=prior.activation
             WHERE previous.execution=? AND NOT EXISTS(SELECT 1 FROM closed_local_compacts c WHERE c.logical=prior.id) AND NOT EXISTS(
                SELECT 1 FROM model_attempts t WHERE t.logical=prior.id AND
                (t.state='paired' OR (t.state='response' AND NOT EXISTS(
                  SELECT 1 FROM model_tool_intents i WHERE i.logical=prior.id)))))",
        )
        .bind(&exec)
        .fetch_one(&mut *conn)
        .await?;
        if unresolved {
            return Err("resume the original logical request before admitting a new step".into());
        }
        let taken = sqlx::query(
            "UPDATE root_budgets SET reserved_requests=reserved_requests+1, reserved_units=reserved_units+?
             WHERE root=? AND reserved_requests<max_requests AND reserved_units<=max_units-?",
        )
        .bind(units).bind(&root).bind(units).execute(&mut *conn).await?;
        if taken.rows_affected() != 1 {
            return Err("root budget exhausted".into());
        }
        sqlx::query(
            "INSERT INTO model_requests(id,activation,ordinal,root,original_work_id,estimated_input,
                 max_output,retry_unbilled,native_tools,request_json)
             VALUES(?,?,(SELECT coalesce(max(ordinal),-1)+1 FROM model_requests WHERE activation=?),?,?,?,?,?,?,?)",
        )
        .bind(logical).bind(activation).bind(activation).bind(&root)
        .bind(context.try_get::<Option<String>,_>("work_id")?)
        .bind(input).bind(output).bind(safe).bind(native).bind(&reservation.request)
        .execute(conn).await?;
    }
    Ok(json!({"logical":logical,"root":root,"estimated_input":input,"reserved_output":output}))
}

async fn dispatch(conn: &mut SqliteConnection, logical: &str, attempt: &str) -> Result<Value> {
    if let Some(old) = sqlx::query("SELECT logical,state FROM model_attempts WHERE id=?")
        .bind(attempt)
        .fetch_optional(&mut *conn)
        .await?
    {
        if old.try_get::<String, _>("logical")? != logical {
            return Err("physical attempt belongs to another request".into());
        }
        return Ok(
            json!({"attempt":attempt,"dispatch_now":false,"state":old.try_get::<String,_>("state")?}),
        );
    }
    let meta=sqlx::query("SELECT a.execution,a.profile_contract,r.retry_unbilled,r.native_tools,r.request_json FROM model_requests r JOIN activations a ON a.id=r.activation WHERE r.id=?")
        .bind(logical).fetch_one(&mut *conn).await?;
    execution::eligible(conn, meta.try_get("execution")?).await?;
    let previous=sqlx::query("SELECT ordinal,state,retry_proof FROM model_attempts WHERE logical=? ORDER BY ordinal DESC LIMIT 1")
        .bind(logical).fetch_optional(&mut *conn).await?;
    let ordinal = if let Some(row) = previous {
        if row.try_get::<i64, _>("ordinal")? != 0
            || row.try_get::<String, _>("state")? != "rejected"
            || !row.try_get::<bool, _>("retry_proof")?
            || !meta.try_get::<bool, _>("retry_unbilled")?
            || meta.try_get::<bool, _>("native_tools")?
        {
            return Err("query original attempt; retry not authorized".into());
        }
        1
    } else {
        0
    };
    sqlx::query("INSERT INTO model_attempts(id,logical,ordinal,state) VALUES(?,?,?,'dispatched')")
        .bind(attempt)
        .bind(logical)
        .bind(ordinal)
        .execute(conn)
        .await?;
    Ok(
        json!({"attempt":attempt,"logical":logical,"ordinal":ordinal,"request_raw":meta.try_get::<String,_>("request_json")?,"profile_contract":meta.try_get::<String,_>("profile_contract")?,"dispatch_now":true}),
    )
}

fn valid_json(raw: &str) -> Result<Value> {
    Ok(serde_json::from_str(raw)?)
}

async fn reject(
    conn: &mut SqliteConnection,
    attempt: &str,
    status: i64,
    proof: bool,
    raw: &str,
    usage: &str,
) -> Result<Value> {
    if !(400..600).contains(&status) {
        return Err("expected an HTTP error response".into());
    }
    if let Some(receipt) =
        crate::privacy::source::late_attempt(conn, attempt, status, usage).await?
    {
        return Ok(receipt);
    }
    let body_valid = valid_json(raw).is_ok();
    let usage_value = valid_json(usage).unwrap_or(Value::Null);
    let old = sqlx::query(
        "SELECT state,status_code,response,usage_json,retry_proof,ever_unknown FROM model_attempts WHERE id=?",
    )
    .bind(attempt)
    .fetch_one(&mut *conn)
    .await?;
    let unbilled = body_valid
        && proof
        && status == 429
        && !old.try_get::<bool, _>("ever_unknown")?
        && ["billed_microusd", "output", "reasoning", "total"]
            .iter()
            .all(|f| {
                usage_value
                    .get(*f)
                    .is_none_or(|v| v.is_null() || v.as_i64() == Some(0))
            });
    let state: String = old.try_get("state")?;
    if state == "rejected" {
        if old.try_get::<i64, _>("status_code")? != status
            || old.try_get::<String, _>("response")? != raw
            || old.try_get::<String, _>("usage_json")? != usage
            || old.try_get::<bool, _>("retry_proof")? != unbilled
        {
            return Err("original rejection conflicts".into());
        }
    } else {
        if !matches!(state.as_str(), "dispatched" | "unknown") {
            return Err("attempt cannot accept this rejection".into());
        }
        sqlx::query("UPDATE model_attempts SET state='rejected',status_code=?,response=?,usage_json=?,retry_proof=?,received_at=CURRENT_TIMESTAMP WHERE id=?")
            .bind(status).bind(raw).bind(usage).bind(unbilled).bind(attempt).execute(conn).await?;
    }
    Ok(json!({"attempt":attempt,"state":"rejected","retry_proof":unbilled}))
}

async fn response(
    conn: &mut SqliteConnection,
    attempt: &str,
    raw: &str,
    usage: &str,
    intent_text: &str,
) -> Result<Value> {
    if let Some(receipt) = crate::privacy::source::late_attempt(conn, attempt, 200, usage).await? {
        return Ok(receipt);
    }
    let body_valid = valid_json(raw).is_ok();
    let row=sqlx::query("SELECT t.logical,t.state,t.response,t.usage_json,t.intents_json,a.execution,coalesce(e.original_actor,e.origin) AS original_actor,e.cancelled
        FROM model_attempts t JOIN model_requests r ON r.id=t.logical JOIN activations a ON a.id=r.activation
        JOIN executions e ON e.id=a.execution WHERE t.id=?").bind(attempt).fetch_one(&mut *conn).await?;
    let state: String = row.try_get("state")?;
    if !matches!(state.as_str(), "dispatched" | "unknown") {
        if state != "rejected"
            && row.try_get::<Option<String>, _>("response")?.as_deref() == Some(raw)
            && row.try_get::<Option<String>, _>("usage_json")?.as_deref() == Some(usage)
            && row.try_get::<Option<String>, _>("intents_json")?.as_deref() == Some(intent_text)
        {
            return Ok(json!({"attempt":attempt,"state":state}));
        }
        return Err("physical response conflicts with retained original".into());
    }
    let tools = serde_json::from_str::<Vec<Tool>>(intent_text);
    let mut seen = HashSet::new();
    let valid = tools.as_ref().is_ok_and(|items| {
        items.iter().all(|t| {
            !t.call_id.is_empty()
                && seen.insert(t.call_id.clone())
                && !t.target.is_empty()
                && t.target.trim() == t.target
        })
    });
    let exec: String = row.try_get("execution")?;
    let actor: String = row.try_get("original_actor")?;
    let logical: String = row.try_get("logical")?;
    let permitted = effects::authorize(conn, &exec, &actor, true).await.is_ok();
    let next = if body_valid && valid && permitted {
        "response"
    } else {
        "response_held"
    };
    if next == "response" {
        for (index, tool) in tools?.into_iter().enumerate() {
            let id = new_id(conn).await?;
            effects::apply(
                conn,
                &effects::Action::Intent {
                    execution: exec.clone(),
                    id: id.clone(),
                    actor: actor.clone(),
                    action: tool.action,
                    target: tool.target,
                    payload: tool.payload.to_string(),
                    parent: None,
                    ordinal: None,
                },
            )
            .await?;
            sqlx::query("INSERT INTO model_tool_intents(logical,ordinal,call_id,execution,operation_id) VALUES(?,?,?,?,?)")
                .bind(&logical).bind(index as i64).bind(tool.call_id).bind(&exec).bind(id).execute(&mut *conn).await?;
        }
    }
    sqlx::query("UPDATE model_attempts SET state=?,status_code=200,response=?,usage_json=?,intents_json=?,received_at=CURRENT_TIMESTAMP,
        diagnostic=? WHERE id=?").bind(next).bind(raw).bind(usage).bind(intent_text).bind(if !body_valid{Some("invalid_response")}else if !valid{Some("invalid_tool_batch")}else if !permitted{Some("authority_unavailable")}else{None})
        .bind(attempt).execute(conn).await?;
    Ok(json!({"attempt":attempt,"state":next}))
}

async fn inspect(conn: &mut SqliteConnection, logical: &str) -> Result<Value> {
    let row=sqlx::query("SELECT r.activation,r.root,r.original_work_id,a.execution,a.profile_contract,e.source,coalesce(e.original_actor,e.origin) AS original_actor,e.work_id,
        t.id AS attempt,t.state,t.response FROM model_requests r JOIN activations a ON a.id=r.activation JOIN executions e ON e.id=a.execution
        LEFT JOIN model_attempts t ON t.logical=r.id WHERE r.id=? ORDER BY t.ordinal DESC LIMIT 1")
        .bind(logical).fetch_one(&mut *conn).await?;
    effects::authorize(
        conn,
        row.try_get("execution")?,
        row.try_get("original_actor")?,
        false,
    )
    .await?;
    let tools=sqlx::query("SELECT i.ordinal,i.call_id,i.operation_id,i.paired_receipt IS NOT NULL AS paired,o.state
        FROM model_tool_intents i JOIN operations o ON o.namespace=i.execution AND o.operation_id=i.operation_id
        WHERE i.logical=? ORDER BY i.ordinal").bind(logical).fetch_all(&mut *conn).await?;
    let mut intents = Vec::new();
    for tool in tools {
        intents.push(json!({"ordinal":tool.try_get::<i64,_>("ordinal")?,"call_id":tool.try_get::<String,_>("call_id")?,
        "operation":tool.try_get::<String,_>("operation_id")?,"state":tool.try_get::<String,_>("state")?,"paired":tool.try_get::<bool,_>("paired")?}));
    }
    let raw: Option<String> = row.try_get("response")?;
    Ok(
        json!({"logical":logical,"activation":row.try_get::<String,_>("activation")?,"root":row.try_get::<String,_>("root")?,
      "execution":row.try_get::<String,_>("execution")?,"source":row.try_get::<Option<i64>,_>("source")?,"actor":row.try_get::<String,_>("original_actor")?,
      "original_work":row.try_get::<Option<String>,_>("original_work_id")?,"work":row.try_get::<Option<String>,_>("work_id")?,
      "profile_contract":row.try_get::<String,_>("profile_contract")?,"attempt":row.try_get::<Option<String>,_>("attempt")?,
      "state":row.try_get::<Option<String>,_>("state")?.unwrap_or_else(||"reserved".into()),
      "response_raw":raw,"tools":intents,"response":raw.as_deref().and_then(|v|valid_json(v).ok())}),
    )
}

async fn pair(conn: &mut SqliteConnection, logical: &str, ordinal: i64) -> Result<Value> {
    let row=sqlx::query("SELECT i.execution,i.operation_id,i.call_id,i.paired_receipt,coalesce(e.original_actor,e.origin) AS original_actor,o.action,o.target,o.state
        FROM model_tool_intents i JOIN executions e ON e.id=i.execution JOIN operations o ON o.namespace=i.execution AND o.operation_id=i.operation_id
        WHERE i.logical=? AND i.ordinal=?").bind(logical).bind(ordinal).fetch_one(&mut *conn).await?;
    let exec: String = row.try_get("execution")?;
    let actor: String = row.try_get("original_actor")?;
    let id: String = row.try_get("operation_id")?;
    effects::authorize(conn, &exec, &actor, false).await?;
    let result = if let Some(raw) = row.try_get::<Option<String>, _>("paired_receipt")? {
        valid_json(&raw)?
    } else {
        // Pairing an already settled self-cancellation records its receipt. It grants no right
        // to dispatch the subsequent intent. All other pairings retain the dispatch fence.
        let cancelled_here:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM work_runs r JOIN work_nodes w ON w.id=r.work JOIN operations o ON o.namespace=r.execution WHERE r.execution=? AND o.operation_id=? AND o.state='confirmed' AND o.target IN ('task_control','subagent_control') AND json_extract(o.payload,'$.action') IN ('cancel','fail') AND w.status IN ('cancelled','failed'))").bind(&exec).bind(&id).fetch_one(&mut *conn).await?;
        if !cancelled_here {
            effects::authorize(conn, &exec, &actor, true).await?;
        }
        let earlier:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM model_tool_intents WHERE logical=? AND ordinal<? AND paired_receipt IS NULL)")
            .bind(logical).bind(ordinal).fetch_one(&mut *conn).await?;
        if earlier {
            return Err("earlier tool result has not been paired".into());
        }
        let receipt = if row.try_get::<String, _>("action")? == "external"
            && row.try_get::<String, _>("target")? == "history_query"
            && row.try_get::<String, _>("state")? != "failed"
        {
            crate::history::query::material(conn, &exec, &actor, &id).await?
        } else if row.try_get::<String, _>("action")? == "external" {
            let r = effects::receipt(conn, &exec, &id).await?;
            if !matches!(
                r["state"].as_str(),
                Some("confirmed" | "failed" | "unknown")
            ) {
                return Err("original external receipt pending".into());
            }
            r
        } else {
            effects::apply(
                conn,
                &effects::Action::Local {
                    execution: exec.clone(),
                    actor,
                    id: id.clone(),
                },
            )
            .await?
        };
        let policy: Option<(i64, i64)> = sqlx::query_as(
            "SELECT page_budget,storage_quota FROM execution_tool_views WHERE execution=?",
        )
        .bind(&exec)
        .fetch_optional(&mut *conn)
        .await?;
        let receipt = if let Some((budget, quota)) = policy {
            crate::tools::results::present(
                conn,
                &exec,
                &id,
                receipt,
                usize::try_from(budget)?,
                quota,
            )
            .await?
        } else {
            receipt
        };
        sqlx::query("UPDATE model_tool_intents SET paired_receipt=? WHERE logical=? AND ordinal=?")
            .bind(receipt.to_string())
            .bind(logical)
            .bind(ordinal)
            .execute(&mut *conn)
            .await?;
        sqlx::query("UPDATE model_attempts SET state='paired' WHERE logical=? AND state='response'
            AND NOT EXISTS(SELECT 1 FROM model_tool_intents WHERE logical=? AND paired_receipt IS NULL)")
            .bind(logical).bind(logical).execute(conn).await?;
        receipt
    };
    Ok(json!({"call_id":row.try_get::<String,_>("call_id")?,"ordinal":ordinal,"receipt":result}))
}

async fn usage(conn: &mut SqliteConnection, root: &str) -> Result<Value> {
    let budget=sqlx::query("SELECT reserved_requests,reserved_units,max_requests,max_units FROM root_budgets WHERE root=?").bind(root)
        .fetch_one(&mut *conn).await?;
    let attempts=sqlx::query("SELECT t.id,t.logical,t.ordinal,t.state,t.usage_json,t.status_code FROM model_attempts t JOIN model_requests r ON r.id=t.logical
        WHERE r.root=? ORDER BY t.logical,t.ordinal").bind(root).fetch_all(conn).await?;
    let mut receipts = Vec::new();
    for row in &attempts {
        let raw: Option<String> = row.try_get("usage_json")?;
        receipts.push(json!({"attempt":row.try_get::<String,_>("id")?,"logical":row.try_get::<String,_>("logical")?,
           "ordinal":row.try_get::<i64,_>("ordinal")?,"state":row.try_get::<String,_>("state")?,
           "status_code":row.try_get::<Option<i64>,_>("status_code")?,"usage_raw":raw,"usage":raw.as_deref().and_then(|v|valid_json(v).ok())}));
    }
    let mut fields = serde_json::Map::new();
    for field in [
        "input",
        "cache",
        "output",
        "reasoning",
        "total",
        "billed_microusd",
    ] {
        let mut sum = 0i64;
        let mut reported = 0;
        for receipt in &receipts {
            if let Some(v) = receipt["usage"][field].as_i64().filter(|v| *v >= 0) {
                sum = sum.checked_add(v).ok_or("usage overflow")?;
                reported += 1;
            }
        }
        fields.insert(field.into(),json!({"known_subtotal":(reported>0).then_some(sum),"reported":reported,"attempts":attempts.len(),
            "total":(reported>0 && reported==attempts.len()).then_some(sum)}));
    }
    let micros = fields["billed_microusd"]["total"].as_i64();
    Ok(
        json!({"root":root,"reserved_requests":budget.try_get::<i64,_>("reserved_requests")?,"reserved_units":budget.try_get::<i64,_>("reserved_units")?,
      "max_requests":budget.try_get::<i64,_>("max_requests")?,"max_units":budget.try_get::<i64,_>("max_units")?,
      "attempts":receipts.len(),"usage_attempts":receipts.iter().filter(|r|r["usage"].is_object()).count(),
      "cost_usd":micros.map(|n|format!("{}.{:06}",n/1_000_000,n%1_000_000)),"fields":fields,"physical_receipts":receipts}),
    )
}

pub async fn apply(conn: &mut SqliteConnection, action: &Action) -> Result<Value> {
    match action {
        Action::Root {
            execution,
            slots,
            units,
        } => execution::root(conn, execution, *slots, *units).await,
        Action::Child { execution, parent } => execution::child(conn, execution, parent).await,
        Action::Activate {
            execution,
            id,
            profile,
        } => execution::activate(conn, execution, id, profile).await,
        Action::AttachWork { execution, work } => execution::attach(conn, execution, work).await,
        Action::Cancel { execution } => {
            sqlx::query_scalar::<_, String>(
                "UPDATE executions SET cancelled=1,state=CASE WHEN state IN ('running','pending') THEN 'held' ELSE state END WHERE id=? RETURNING id",
            )
            .bind(execution)
            .fetch_one(conn)
            .await?;
            Ok(json!({"execution":execution,"cancelled":true}))
        }
        Action::Reserve(reservation) => reserve(conn, reservation).await,
        Action::Dispatch { logical, attempt } => dispatch(conn, logical, attempt).await,
        Action::Unknown { attempt } => {
            sqlx::query_scalar::<_,String>("UPDATE model_attempts SET state='unknown',ever_unknown=1 WHERE id=? AND state IN ('dispatched','unknown') RETURNING id")
                .bind(attempt).fetch_one(conn).await?;
            Ok(json!({"attempt":attempt,"state":"unknown"}))
        }
        Action::Hold {
            attempt,
            body,
            usage,
            partial_bytes,
            diagnostic,
            unknown,
            status,
        } => {
            if let Some(r) =
                crate::privacy::source::late_attempt(conn, attempt, *status, usage).await?
            {
                return Ok(r);
            }
            let state = if *unknown { "unknown" } else { "response_held" };
            let saved=sqlx::query("UPDATE model_attempts SET state=?,ever_unknown=ever_unknown OR ?,status_code=?,response=?,usage_json=?,diagnostic=?,partial_bytes=?,received_at=CURRENT_TIMESTAMP
              WHERE id=? AND state IN ('dispatched','unknown')").bind(state).bind(unknown).bind(status).bind(body).bind(usage).bind(diagnostic).bind(partial_bytes).bind(attempt).execute(conn).await?;
            if saved.rows_affected() != 1 {
                return Err("attempt already terminal".into());
            }
            Ok(json!({"attempt":attempt,"state":state}))
        }
        Action::Rejected {
            attempt,
            status,
            unbilled_before_model,
            body,
            usage,
        } => reject(conn, attempt, *status, *unbilled_before_model, body, usage).await,
        Action::Response {
            attempt,
            body,
            usage,
            intents,
        } => response(conn, attempt, body, usage, intents).await,
        Action::Pair { logical, ordinal } => pair(conn, logical, *ordinal).await,
        Action::Usage { .. } | Action::Inspect { .. } => Err("use snapshot reader".into()),
    }
}
