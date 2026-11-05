//! Model recalls keep the original sending Presence even when today's send route changes.
use crate::{
    Result,
    gateway::qq::Hub,
    runtime::effects::{self, Action, Outcome},
    store::Store,
    tools::registry::Tool,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub(crate) fn declaration() -> Tool {
    let fixture: Value = serde_json::from_str(include_str!("../../fixtures/social/v4-recall.json"))
        .expect("fixed original recall contract");
    let d = &fixture["declarations"][0];
    Tool {
        name: "recall_own_message".into(),
        description: d["description"].as_str().unwrap().into(),
        schema: d["parameters"].clone(),
        parallel_read: false,
        cacheable: false,
        code_direct: false,
        provider: None,
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    event_id: i64,
}
struct Original {
    execution: String,
    child: String,
    stage: String,
    locator: i64,
    kind: String,
    target: String,
    event: i64,
}
async fn original(
    db: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    op: &str,
) -> Result<Original> {
    effects::authorize(db, exec, actor, true).await?;
    let raw:String=sqlx::query_scalar("SELECT payload FROM operations WHERE namespace=? AND operation_id=? AND action='external' AND target='recall_own_message'").bind(exec).bind(op).fetch_one(&mut *db).await?;
    let request: Request = serde_json::from_str(&raw)?;
    if request.event_id < 1 {
        return Err("invalid_event_id".into());
    }
    let scope = crate::history::query::members::scope(db, exec, actor).await?;
    let row=sqlx::query("SELECT o.*,a.platform,s.status,s.kind,d.scope,coalesce(p.original_person,c.id) AS target FROM personal_deliveries e JOIN social_outbound o ON o.id=e.outbound JOIN assignments s ON s.id=o.stage JOIN accounts a ON a.id=s.account_id JOIN entries d ON d.id=o.entry LEFT JOIN assignments p ON p.id=d.actor_stage LEFT JOIN contact_spaces c ON (c.space_key=d.space OR c.id=lower(d.space)) WHERE e.event=? AND o.body!='{}' AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.event OR i.source=o.echo_event)")
        .bind(request.event_id).fetch_optional(&mut *db).await?.ok_or("not_own_message")?;
    if !scope
        .entries
        .iter()
        .any(|id| id == row.get::<&str, _>("entry"))
    {
        return Err("recall_outside_original_scope".into());
    }
    let execution: String = row.get("namespace");
    if crate::privacy::source::execution_erased(db, &execution, None).await? {
        return Err("execution source erased".into());
    }
    if row.get::<&str, _>("platform") != "qq"
        || row.get::<&str, _>("kind") != "self"
        || row.get::<&str, _>("status") != "active"
    {
        return Err("original_presence_unavailable".into());
    }
    let locator = row
        .get::<Option<String>, _>("locator")
        .ok_or("message_recall_unavailable")?
        .parse()?;
    Ok(Original {
        execution,
        child: row.get("child"),
        stage: row.get("stage"),
        locator,
        kind: if row.get::<&str, _>("scope") == "group" {
            "space"
        } else {
            "person"
        }
        .into(),
        target: row.try_get("target")?,
        event: request.event_id,
    })
}

pub(crate) async fn claim(
    db: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    op: &str,
    expected: &str,
    session: u64,
) -> Result<Value> {
    let r = original(db, exec, actor, op).await?;
    if r.stage != expected {
        return Err("original_presence_changed".into());
    }
    let state = effects::apply(
        db,
        &Action::Dispatch {
            execution: exec.into(),
            id: op.into(),
            actor: actor.into(),
        },
    )
    .await?;
    if state["dispatch_now"] != true {
        return Ok(json!({"dispatch":false}));
    }
    let granted=sqlx::query("INSERT INTO social_recalls(namespace,child,session,state) VALUES(?,?,?,'dispatched') ON CONFLICT DO NOTHING")
        .bind(&r.execution).bind(&r.child).bind(session as i64).execute(&mut *db).await?.rows_affected()==1;
    sqlx::query("INSERT INTO model_recalls VALUES(?,?,?,?,?,?,?,?)")
        .bind(exec)
        .bind(op)
        .bind(r.event)
        .bind(&r.execution)
        .bind(&r.child)
        .bind(&r.stage)
        .bind(&r.kind)
        .bind(&r.target)
        .execute(&mut *db)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO main_dependencies(execution,event) SELECT ?1,?2 UNION SELECT ?1,source FROM executions WHERE id=?3 AND source IS NOT NULL UNION SELECT ?1,event FROM main_dependencies WHERE execution=?3")
        .bind(exec).bind(r.event).bind(&r.execution).execute(&mut *db).await?;
    if !granted {
        let prior: String =
            sqlx::query_scalar("SELECT state FROM social_recalls WHERE namespace=? AND child=?")
                .bind(&r.execution)
                .bind(&r.child)
                .fetch_one(&mut *db)
                .await?;
        settle(
            db,
            &r.execution,
            &r.child,
            if prior == "dispatched" {
                "unknown"
            } else {
                &prior
            },
        )
        .await?;
    }
    Ok(
        json!({"dispatch":granted,"original_execution":r.execution,"child":r.child,"stage":r.stage,"session":session,"message_id":r.locator}),
    )
}

pub(crate) async fn settle(
    db: &mut SqliteConnection,
    original: &str,
    child: &str,
    state: &str,
) -> Result<()> {
    let rows=sqlx::query("SELECT r.* FROM model_recalls r JOIN operations o ON o.namespace=r.execution AND o.operation_id=r.operation WHERE r.original_execution=? AND r.child=? AND o.state IN ('dispatched','unknown')")
        .bind(original).bind(child).fetch_all(&mut *db).await?;
    for row in rows {
        let exec: String = row.get("execution");
        let op: String = row.get("operation");
        let (outcome, status, error) = match state {
            "confirmed" => (Outcome::Confirmed, "succeeded", None),
            "failed" => (Outcome::Failed, "failed", Some("platform_rejected")),
            _ => (Outcome::Unknown, "uncertain", Some("delivery_unknown")),
        };
        let result = json!({"operation_id":op,"action":"recall_own_message","status":status,"target":{"kind":row.get::<String,_>("target_kind"),"id":row.get::<String,_>("target_id")},"presence_id":row.get::<String,_>("presence"),"event_id":null,"original_event_id":null,"original_conversation_id":null,"error_category":error});
        effects::apply(
            db,
            &Action::Settle {
                execution: exec,
                id: op,
                outcome,
                result: result.to_string(),
            },
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn perform(
    store: &Store,
    hub: &Hub,
    exec: &str,
    actor: &str,
    op: &str,
) -> Result<()> {
    let prepared = async {
        let mut tx = store.readers().begin().await?;
        let r = original(&mut tx, exec, actor, op).await?;
        let prior: Option<String> =
            sqlx::query_scalar("SELECT state FROM social_recalls WHERE namespace=? AND child=?")
                .bind(&r.execution)
                .bind(&r.child)
                .fetch_optional(&mut *tx)
                .await?;
        // Replays can consume a retained receipt offline; they cannot acquire a new dispatch.
        let session = hub
            .session(&r.stage)
            .or_else(|| prior.map(|_| 0))
            .ok_or("original_presence_unavailable")?;
        drop(tx);
        super::write(
            store,
            super::Change::ModelRecall(exec.into(), actor.into(), op.into(), r.stage, session),
        )
        .await
    }
    .await;
    let grant = match prepared {
        Ok(grant) => grant,
        Err(error) => return crate::tools::reject(store, exec, actor, op, error.to_string()).await,
    };
    if grant["dispatch"] != true {
        return Ok(());
    }
    let stage = grant["stage"].as_str().ok_or("missing original stage")?;
    let child = grant["child"].as_str().ok_or("missing original child")?;
    let session = grant["session"]
        .as_u64()
        .ok_or("missing original session")?;
    let echo = format!("social-recall:{child}");
    let reply = hub
        .request_bound(
            stage,
            "delete_msg",
            json!({"message_id":grant["message_id"]}),
            Some((session, &echo)),
        )
        .await
        .ok();
    super::write(
        store,
        super::Change::RecallReply(
            grant["original_execution"].as_str().unwrap().into(),
            child.into(),
            super::recall_state(reply.as_ref()).into(),
        ),
    )
    .await?;
    Ok(())
}
