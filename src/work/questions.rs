use super::{mailbox, tree};
use crate::{
    Result,
    ingress::admit::{Frame, Segment},
};
use serde_json::json;
use sqlx::{Row, SqliteConnection};

pub(super) async fn register(conn: &mut SqliteConnection, work: &str, exec: &str) -> Result<()> {
    // Bind the latest actual Social question from this execution, not an unconfirmed plan or a forwarded locator.
    sqlx::query("INSERT INTO work_user_questions(work,execution,generation,source_event,outbound) SELECT ?,t.execution,t.window,coalesce((SELECT m.event FROM work_request_marks m JOIN work_proposals p ON p.logical=m.logical WHERE p.execution=t.execution AND p.kind='need_input' ORDER BY p.rowid DESC LIMIT 1),0),
      (SELECT o.id FROM social_outbound o JOIN operations c ON c.namespace=o.namespace AND c.operation_id=o.child AND c.state='confirmed' WHERE o.namespace=t.execution AND o.entry=t.conversation AND o.locator IS NOT NULL AND NOT o.recalled AND o.body!='{}' ORDER BY o.id DESC LIMIT 1) FROM main_turns t WHERE t.execution=? ON CONFLICT(work) DO UPDATE SET execution=excluded.execution,generation=excluded.generation,source_event=excluded.source_event,outbound=excluded.outbound")
        .bind(work).bind(exec).execute(&mut *conn).await?;
    let events:Vec<i64>=sqlx::query_scalar("SELECT e.id FROM events e JOIN accepted_sources a ON a.event_id=e.id JOIN work_user_questions q ON q.work=? AND e.id>q.source_event JOIN work_nodes w ON w.id=q.work AND e.entry_id=w.entry ORDER BY e.id").bind(work).fetch_all(&mut *conn).await?;
    for event in events {
        if route(conn, event, None).await? {
            break;
        }
    }
    Ok(())
}
pub(super) async fn parent_question(
    conn: &mut SqliteConnection,
    work: &str,
    exec: &str,
    key: &str,
    text: &str,
) -> Result<Option<serde_json::Value>> {
    let row=sqlx::query("SELECT p.id,p.status,p.owner_execution,EXISTS(SELECT 1 FROM work_suspensions s WHERE s.work=p.id) AS suspended FROM work_nodes w JOIN work_nodes p ON p.id=w.parent WHERE w.id=?").bind(work).fetch_optional(&mut *conn).await?;
    let Some(row) = row else { return Ok(None) };
    let parent: String = row.try_get("id")?;
    let id = mailbox::put(conn, &parent, exec, key, "question", text, None).await?;
    let status: String = row.try_get("status")?;
    if matches!(status.as_str(), "completed" | "cancelled" | "failed") {
        return Ok(Some(
            json!({"work_id":parent,"message_id":id.to_string(),"parent_state":"terminal","wait":false}),
        ));
    }
    let suspended: bool = row.try_get("suspended")?;
    if !suspended {
        let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM work_runs r JOIN executions x ON x.id=r.execution WHERE r.work=? AND x.state IN('pending','running') AND NOT x.cancelled)").bind(&parent).fetch_one(&mut *conn).await?;
        if !active {
            sqlx::query("UPDATE work_waits SET state='cancelled' WHERE work=? AND state='armed'")
                .bind(&parent)
                .execute(&mut *conn)
                .await?;
            tree::wake(
                conn,
                &parent,
                row.try_get("owner_execution")?,
                &format!("question:{id}"),
            )
            .await?;
        }
    }
    Ok(Some(
        json!({"work_id":parent,"message_id":id.to_string(),"parent_state":if row.try_get::<bool,_>("suspended")?{"suspended"}else{"live"},"wait":true}),
    ))
}
pub async fn route(
    conn: &mut SqliteConnection,
    event: i64,
    explicit: Option<&str>,
) -> Result<bool> {
    let e=sqlx::query("SELECT e.envelope,e.entry_id,e.bot_stage,e.raw_actor,a.platform FROM events e JOIN accepted_sources s ON s.event_id=e.id JOIN accounts a ON a.id=e.account_id WHERE e.id=? AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)").bind(event).fetch_one(&mut *conn).await?;
    let platform: String = e.try_get("platform")?;
    if !matches!(platform.as_str(), "qq" | "app" | "acp") {
        return Ok(false);
    }
    let frame: Frame = serde_json::from_str(e.try_get("envelope")?)?;
    let reply = frame.segments.iter().find_map(|s| {
        if let Segment::Reply { native_id } = s {
            Some(native_id.as_str())
        } else {
            None
        }
    });
    if platform == "qq" && (reply.is_none() || explicit.is_some()) {
        return Ok(false);
    }
    let q=sqlx::query("SELECT n.id,n.entry,q.execution,q.outbound FROM work_user_questions q JOIN work_nodes n ON n.id=q.work JOIN persons owner ON owner.id=n.principal AND owner.enabled JOIN persons sender ON sender.id=?1 AND sender.enabled WHERE n.goal IS NOT NULL AND n.status='need_input' AND q.source_event<?2 AND q.generation=coalesce((SELECT generation FROM main_windows WHERE conversation=n.entry),1) AND coalesce(owner.current_root,owner.id)=coalesce(sender.current_root,sender.id)
     AND ((?3 IS NULL AND n.entry=?4) OR (n.id=?3 AND ?5 IN('app','acp'))) AND NOT EXISTS(SELECT 1 FROM work_suspensions WHERE work=n.id)
     AND (?5!='qq' OR EXISTS(SELECT 1 FROM social_outbound last JOIN operations last_op ON last_op.namespace=last.namespace AND last_op.operation_id=last.child JOIN social_outbound part ON part.namespace=last.namespace AND part.entry=last.entry AND part.stage=last.stage JOIN operations part_op ON part_op.namespace=part.namespace AND part_op.operation_id=part.child WHERE last.id=q.outbound AND part_op.parent=last_op.parent AND part_op.state='confirmed' AND part.stage=?6 AND part.locator=?7 AND NOT part.recalled AND part.body!='{}')) ORDER BY q.rowid DESC LIMIT 1")
        .bind(e.try_get::<String,_>("raw_actor")?).bind(event).bind(explicit).bind(e.try_get::<String,_>("entry_id")?).bind(&platform).bind(e.try_get::<String,_>("bot_stage")?).bind(reply).fetch_optional(&mut *conn).await?;
    let Some(q) = q else { return Ok(false) };
    let work: String = q.try_get("id")?;
    let manager: String = q.try_get("execution")?;
    crate::runtime::execution::eligible(conn, &manager).await?;
    let key = format!("human-answer:{event}");
    let text = crate::runtime::main::event_text(e.try_get("envelope")?)?;
    let mail = mailbox::put(conn, &work, &manager, &key, "human_answer", &text, None).await?;
    sqlx::query("INSERT INTO work_wait_mail VALUES(?,?)")
        .bind(mail)
        .bind(event)
        .execute(&mut *conn)
        .await?;
    let wake = tree::wake(conn, &work, &manager, &key).await?;
    sqlx::query("INSERT OR IGNORE INTO main_dependencies VALUES(?,?)")
        .bind(wake)
        .bind(event)
        .execute(&mut *conn)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO work_wait_routed VALUES(?,?)")
        .bind(event)
        .bind(&work)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM work_user_questions WHERE work=?")
        .bind(work)
        .execute(conn)
        .await?;
    Ok(true)
}

pub(crate) async fn explicit(conn: &mut SqliteConnection, event: i64, work: &str) -> Result<()> {
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM work_explicit_answers WHERE event=?)",
    )
    .bind(event)
    .fetch_one(&mut *conn)
    .await?
    {
        return Ok(());
    }
    let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM events e JOIN accounts a ON a.id=e.account_id WHERE e.id=? AND a.platform IN('app','acp'))").bind(event).fetch_one(&mut *conn).await?;
    if !allowed {
        return Err("explicit Work replies require an authenticated App or ACP origin".into());
    }
    let matched = route(conn, event, Some(work)).await?;
    sqlx::query("INSERT INTO work_explicit_answers VALUES(?,?,?)")
        .bind(event)
        .bind(work)
        .bind(matched)
        .execute(conn)
        .await?;
    Ok(())
}
