//! Admin manages the retained Work, without substituting the operator for its source.
use crate::Result;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub(crate) async fn recover(db: &mut SqliteConnection) -> Result<()> {
    // A process death has no reliable elapsed endpoint. Never count downtime as work.
    sqlx::query("UPDATE work_run_activity SET active_ms=NULL,running_since=NULL WHERE running_since IS NOT NULL").execute(db).await?;
    Ok(())
}
pub(crate) async fn publishable(db: &mut SqliteConnection, ids: &[String]) -> Result<()> {
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM work_nodes w WHERE w.id IN(SELECT value FROM json_each(?)) AND w.goal IS NOT NULL AND NOT EXISTS(SELECT 1 FROM work_sources ws JOIN invalidated_sources i ON i.source=ws.event WHERE ws.work=w.id)")
        .bind(serde_json::to_string(ids)?).fetch_one(db).await?;
    if count as usize != ids.len() {
        return Err("not_found".into());
    }
    Ok(())
}

const SELECT:&str="SELECT w.*,m.created_ms,m.generation,m.reason AS management_reason,r.revision,s.reason AS suspension,
 x.source AS source_event,o.kind AS original_kind,t.diagnostic,
 (SELECT state FROM executions e JOIN work_runs run ON run.execution=e.id WHERE run.work=w.id ORDER BY run.rowid DESC LIMIT 1) AS run_state,
 EXISTS(SELECT 1 FROM work_waits wait WHERE wait.work=w.id AND wait.state IN('armed','proposed')) AS waiting
 FROM work_nodes w JOIN personal_work_revisions r ON r.work=w.id LEFT JOIN work_admin_metadata m ON m.work=w.id
 JOIN executions x ON x.id=w.owner_execution LEFT JOIN main_origins o ON o.ref=x.origin LEFT JOIN main_turns t ON t.execution=(SELECT execution FROM work_runs WHERE work=w.id ORDER BY rowid DESC LIMIT 1)
 LEFT JOIN work_suspensions s ON s.work=w.id WHERE w.goal IS NOT NULL
 AND NOT EXISTS(SELECT 1 FROM work_sources ws JOIN invalidated_sources i ON i.source=ws.event WHERE ws.work=w.id)";
fn state(row: &sqlx::sqlite::SqliteRow) -> Result<&str> {
    let state: &str = row.try_get("status")?;
    if matches!(state, "completed" | "cancelled" | "failed") {
        return Ok(state);
    }
    if row.try_get::<Option<&str>, _>("suspension")?.is_some()
        || row.try_get::<Option<&str>, _>("run_state")? == Some("held")
    {
        return Ok("suspended");
    }
    Ok(match state {
        "need_input" => "waiting_user",
        "active" if row.try_get("waiting")? => "waiting_external",
        "active" if row.try_get::<Option<&str>, _>("run_state")? == Some("pending") => "queued",
        "active" => "running",
        other => other,
    })
}
async fn original_run(db: &mut SqliteConnection, id: &str) -> Result<String> {
    sqlx::query_scalar("SELECT execution FROM work_runs WHERE work=? ORDER BY rowid DESC LIMIT 1")
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| "state_mismatch".into())
}
async fn blocker(
    db: &mut SqliteConnection,
    row: &sqlx::sqlite::SqliteRow,
) -> Result<Option<&'static str>> {
    if !matches!(
        state(row)?,
        "suspended" | "waiting_user" | "waiting_external" | "failed"
    ) {
        return Ok(Some("precondition_failed"));
    }
    let generation: Option<i64> =
        sqlx::query_scalar("SELECT generation FROM main_windows WHERE conversation=?")
            .bind(row.try_get::<&str, _>("entry")?)
            .fetch_optional(&mut *db)
            .await?;
    if generation.is_none() || generation != row.try_get::<Option<i64>, _>("generation")? {
        return Ok(Some("precondition_failed"));
    }
    if row.try_get::<Option<&str>, _>("run_state")? == Some("running") {
        return Ok(Some("precondition_failed"));
    }
    let id: &str = row.try_get("id")?;
    let run = original_run(db, id).await?;
    if crate::runtime::effects::authorize_resume(db, &run)
        .await
        .is_err()
    {
        return Ok(Some("precondition_failed"));
    }
    let intact:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM executions x JOIN main_turns t ON t.execution=x.id WHERE x.id=? AND NOT x.cancelled AND (t.material IS NOT NULL OR x.source IS NOT NULL) AND NOT EXISTS(SELECT 1 FROM model_requests mr JOIN activations a ON a.id=mr.activation WHERE a.execution=x.id AND (mr.request_json IS NULL OR mr.request_json='{}')))")
        .bind(run).fetch_one(&mut *db).await?;
    Ok(if intact { None } else { Some("state_mismatch") })
}
async fn view(
    db: &mut SqliteConnection,
    row: &sqlx::sqlite::SqliteRow,
    detail: bool,
) -> Result<Value> {
    let id: &str = row.try_get("id")?;
    let status = state(row)?;
    let source = json!({"origin":if row.try_get::<Option<i64>,_>("source_event")?.is_some(){Some("user_message")}else{match row.try_get::<Option<&str>,_>("original_kind")?{Some("self")=>Some("self_initiative"),_=>None}},
        "owner":match row.try_get::<Option<&str>,_>("original_kind")?{Some("plugin")=>Some("plugin_invocation"),Some("schedule")=>Some("automation"),_=>None},"initiative_run_id":null});
    let models:i64=sqlx::query_scalar("SELECT count(*) FROM model_requests m JOIN activations a ON a.id=m.activation JOIN work_runs r ON r.execution=a.execution WHERE r.work=?").bind(id).fetch_one(&mut *db).await?;
    let tools:i64=sqlx::query_scalar("SELECT coalesce(sum(c.charged),0) FROM tool_charges c JOIN work_runs r ON r.execution=c.execution WHERE r.work=?").bind(id).fetch_one(&mut *db).await?;
    let sent:i64=sqlx::query_scalar("SELECT count(*) FROM social_outbound o JOIN work_runs r ON r.execution=o.namespace WHERE r.work=? AND o.observed_at IS NOT NULL").bind(id).fetch_one(&mut *db).await?;
    let active:Option<i64>=sqlx::query_scalar("SELECT CASE WHEN count(*)=count(a.active_ms) THEN sum(a.active_ms+CASE WHEN a.running_since IS NULL THEN 0 ELSE max(0,CAST(unixepoch('subsec')*1000 AS INTEGER)-a.running_since) END) END FROM work_runs r LEFT JOIN work_run_activity a ON a.execution=r.execution WHERE r.work=?")
        .bind(id).fetch_one(&mut *db).await?;
    let reason = row
        .try_get::<Option<&str>, _>("suspension")?
        .or(
            if row.try_get::<Option<&str>, _>("run_state")? == Some("held") {
                row.try_get::<Option<&str>, _>("diagnostic")?
            } else {
                None
            },
        )
        .or(row.try_get::<Option<&str>, _>("management_reason")?)
        .or(row.try_get::<Option<&str>, _>("diagnostic")?);
    let mut result = json!({"id":id,"parent_work_id":row.try_get::<Option<&str>,_>("parent")?,"conversation_id":row.try_get::<&str,_>("entry")?,
        "generation":row.try_get::<Option<i64>,_>("generation")?,"goal":row.try_get::<Option<&str>,_>("goal")?,"revision":row.try_get::<i64,_>("revision")?,
        "state":status,"reason":reason,"model_requests":models,"tool_calls":tools,"sent_messages":sent,"active_seconds":active.map(|v|v as f64/1000.0),
        "created":row.try_get::<Option<i64>,_>("created_ms")?.map(|v|v as f64/1000.0),"updated":row.try_get::<i64,_>("updated")? as f64/1000.0,"source":source});
    if detail {
        let terminal = matches!(status, "completed" | "failed" | "cancelled");
        let blocked = if terminal && status != "failed" {
            None
        } else {
            blocker(db, row).await?
        };
        result["pause_reason"] = if matches!(status, "suspended" | "waiting_user" | "failed") {
            json!(reason)
        } else {
            Value::Null
        };
        let signal=sqlx::query("SELECT w.id,w.state,w.delivered_at,(SELECT max(id) FROM work_mail m WHERE m.work=w.work AND m.source_key='wait:'||w.id) AS mail,(SELECT watermark FROM work_consumption WHERE work=w.work) AS watermark FROM work_waits w WHERE work=? AND state IN('woke','expired') ORDER BY created DESC,id DESC LIMIT 1")
            .bind(id).fetch_optional(&mut *db).await?;
        result["signal"] = if let Some(s) = signal {
            let mail: Option<i64> = s.try_get("mail")?;
            let consumed: Option<i64> = s.try_get("watermark")?;
            json!({"wait_id":s.try_get::<&str,_>("id")?,"status":if s.try_get::<&str,_>("state")?=="woke"{"delivered"}else{"expired"},"delivered":s.try_get::<Option<i64>,_>("delivered_at")?.map(|v|v as f64/1000.0),
            "input_state":if status=="cancelled"{Some("cancelled")}else{mail.map(|m|if consumed.is_some_and(|c|m<=c){"consumed"}else{"pending"})}})
        } else {
            Value::Null
        };
        result["actions"] = json!({"resume":(!terminal||status=="failed")&&blocked.is_none(),"resume_blocked_by":blocked,"cancel":!terminal});
    }
    Ok(result)
}
pub(crate) async fn read(db: &mut SqliteConnection, method: &str, args: &Value) -> Result<Value> {
    if method == "work.get" {
        let id = args["id"].as_str().ok_or("validation_error")?;
        crate::host::personal::credentials::uuid(id)?;
        let row = sqlx::query(&format!("{SELECT} AND w.id=?"))
            .bind(id)
            .fetch_optional(&mut *db)
            .await?
            .ok_or("not_found")?;
        return view(db, &row, true).await;
    }
    let limit = args["limit"].as_i64().unwrap_or(100);
    if !(1..=250).contains(&limit) {
        return Err("validation_error".into());
    }
    let filter = args["state"].as_str();
    let mut items = Vec::new();
    let rows=sqlx::query(&format!("{SELECT} AND w.id>?1 AND (?2 IS NULL OR (CASE WHEN w.status IN('completed','cancelled','failed') THEN w.status WHEN s.reason IS NOT NULL OR run_state='held' THEN 'suspended' WHEN w.status='need_input' THEN 'waiting_user' WHEN w.status='active' AND EXISTS(SELECT 1 FROM work_waits wait WHERE wait.work=w.id AND wait.state IN('armed','proposed')) THEN 'waiting_external' WHEN w.status='active' AND run_state='pending' THEN 'queued' WHEN w.status='active' THEN 'running' ELSE w.status END)=?2) ORDER BY w.id LIMIT ?3"))
        .bind(args["after"].as_str().unwrap_or("")).bind(filter).bind(limit+1).fetch_all(&mut *db).await?;
    for row in &rows {
        if filter.is_none_or(|s| state(row).is_ok_and(|v| v == s)) {
            items.push(view(db, row, false).await?);
            if items.len() > limit as usize {
                break;
            }
        }
    }
    let next = if items.len() > limit as usize {
        items.pop();
        items.last().map(|v| v["id"].clone())
    } else {
        None
    };
    Ok(json!({"items":items,"next_cursor":next}))
}
pub(crate) async fn mutate(
    db: &mut SqliteConnection,
    id: &str,
    action: &str,
    key: &str,
    expected: i64,
) -> Result<Value> {
    let row = sqlx::query(&format!("{SELECT} AND w.id=?"))
        .bind(id)
        .fetch_optional(&mut *db)
        .await?
        .ok_or("not_found")?;
    if row.try_get::<i64, _>("revision")? != expected {
        return Err("version_conflict".into());
    }
    let status = state(&row)?;
    let manager: &str = row.try_get("owner_execution")?;
    let result = match action {
        "work.cancel" => {
            if matches!(status, "completed" | "cancelled" | "failed") {
                return Err("precondition_failed".into());
            }
            let nodes:Vec<String>=sqlx::query_scalar("WITH RECURSIVE subtree(id,depth) AS(SELECT ?,0 UNION ALL SELECT n.id,s.depth+1 FROM work_nodes n JOIN subtree s ON n.parent=s.id) SELECT id FROM subtree ORDER BY depth DESC,id")
                .bind(id).fetch_all(&mut *db).await?;
            for node in nodes {
                sqlx::query("UPDATE executions SET cancelled=1,state=CASE WHEN state IN('pending','running') THEN 'held' ELSE state END WHERE id IN(SELECT execution FROM work_runs WHERE work=?)").bind(&node).execute(&mut *db).await?;
                super::tree::terminal(
                    db,
                    &node,
                    manager,
                    &format!("{key}:{node}"),
                    "cancelled",
                    "operator_cancelled",
                )
                .await?;
            }
            "cancelled"
        }
        "work.resume" => {
            if let Some(reason) = blocker(db, &row).await? {
                return Err(reason.into());
            }
            let run = original_run(db, id).await?;
            sqlx::query("DELETE FROM work_suspensions WHERE work=?")
                .bind(id)
                .execute(&mut *db)
                .await?;
            // This authenticates the retained execution; operator capabilities never
            // become an actor on a model/effect request. Failure rolls this writer back.
            crate::runtime::execution::eligible(db, &run)
                .await
                .map_err(|_| "precondition_failed")?;
            sqlx::query("UPDATE work_waits SET state='cancelled' WHERE work=? AND state IN('proposed','armed')").bind(id).execute(&mut *db).await?;
            super::mailbox::put(
                db,
                id,
                &run,
                key,
                "operator_control",
                "Resume the original Work",
                None,
            )
            .await?;
            sqlx::query("UPDATE executions SET state='pending' WHERE id=? AND NOT cancelled")
                .bind(&run)
                .execute(&mut *db)
                .await?;
            sqlx::query("UPDATE work_nodes SET status='active',updated=? WHERE id=?")
                .bind(super::tree::tick()?)
                .bind(id)
                .execute(&mut *db)
                .await?;
            "queued"
        }
        _ => return Err("validation_error".into()),
    };
    sqlx::query("UPDATE personal_work_revisions SET revision=? WHERE work=?")
        .bind(expected + 1)
        .bind(id)
        .execute(&mut *db)
        .await?;
    sqlx::query("UPDATE work_admin_metadata SET reason=? WHERE work=?")
        .bind(if action == "work.cancel" {
            "operator_cancelled"
        } else {
            "operator_resume"
        })
        .bind(id)
        .execute(db)
        .await?;
    Ok(
        json!({"success":true,"resource_id":id,"revision":expected+1,"audit_id":key,"effective_state":{"resource":id,"revision":expected+1,"status":result},"operation":null}),
    )
}

#[cfg(test)]
mod publication_tests {
    use super::*;
    use sqlx::Connection;

    #[tokio::test]
    async fn an_erasure_between_snapshot_and_publication_closes_the_response() {
        let mut db = SqliteConnection::connect("sqlite::memory:").await.unwrap();
        sqlx::raw_sql("CREATE TABLE work_nodes(id TEXT PRIMARY KEY,goal TEXT); CREATE TABLE work_sources(work TEXT,event INTEGER); CREATE TABLE invalidated_sources(source INTEGER); INSERT INTO work_nodes VALUES('a','retained goal'),('b','private goal'); INSERT INTO work_sources VALUES('a',1),('b',2);").execute(&mut db).await.unwrap();
        let snapshot: Vec<String> = sqlx::query_scalar("SELECT id FROM work_nodes ORDER BY id")
            .fetch_all(&mut db)
            .await
            .unwrap();
        publishable(&mut db, &snapshot).await.unwrap();
        // The reader already has b's goal when the erasure commits on the writer.
        sqlx::query("INSERT INTO invalidated_sources VALUES(2)")
            .execute(&mut db)
            .await
            .unwrap();
        assert_eq!(
            publishable(&mut db, &snapshot)
                .await
                .unwrap_err()
                .to_string(),
            "not_found"
        );
        publishable(&mut db, &["a".into()]).await.unwrap();
        sqlx::query("UPDATE work_nodes SET goal=NULL WHERE id='a'")
            .execute(&mut db)
            .await
            .unwrap();
        assert!(publishable(&mut db, &["a".into()]).await.is_err());
        publishable(&mut db, &[]).await.unwrap();
    }
}
