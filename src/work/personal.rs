// Device-authenticated personal input may target the owner's original QQ Work.
// The private App event never enters the ordinary Main admission queue.
use super::{control, mailbox, tree};
use crate::{Result, runtime::effects};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
pub async fn owner(conn: &mut SqliteConnection, root: &str, id: &str) -> Result<String> {
    let exec:String=sqlx::query_scalar("SELECT w.owner_execution FROM work_nodes w JOIN persons p ON p.id=w.principal AND p.enabled JOIN persons r ON r.id=coalesce(p.current_root,p.id) AND r.enabled WHERE w.id=? AND r.id=? AND w.goal IS NOT NULL AND NOT EXISTS(SELECT 1 FROM work_sources s JOIN invalidated_sources i ON i.source=s.event WHERE s.work=w.id)").bind(id).bind(root).fetch_optional(&mut *conn).await?.ok_or("not_found")?;
    let actor: String =
        sqlx::query_scalar("SELECT coalesce(original_actor,origin) FROM executions WHERE id=?")
            .bind(&exec)
            .fetch_one(&mut *conn)
            .await?;
    effects::authorize(conn, &exec, &actor, false)
        .await
        .map_err(|_| "original_work_unavailable")?;
    Ok(exec)
}
pub async fn input(
    conn: &mut SqliteConnection,
    id: &str,
    manager: &str,
    key: &str,
    event: i64,
    text: &str,
) -> Result<Value> {
    let state: String = sqlx::query_scalar("SELECT status FROM work_nodes WHERE id=?")
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    if matches!(state.as_str(), "cancelled" | "completed" | "failed") {
        return Err("precondition_failed".into());
    }
    crate::runtime::execution::eligible(conn, manager).await?;
    let mail = mailbox::put(conn, id, manager, key, "human_answer", text, None).await?;
    sqlx::query("INSERT INTO work_wait_mail VALUES(?,?)")
        .bind(mail)
        .bind(event)
        .execute(&mut *conn)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO work_sources VALUES(?,?)")
        .bind(id)
        .bind(event)
        .execute(&mut *conn)
        .await?;
    let active=sqlx::query_scalar::<_,String>("SELECT x.id FROM work_runs r JOIN executions x ON x.id=r.execution WHERE r.work=? AND x.state IN ('pending','running') AND NOT x.cancelled ORDER BY r.rowid DESC LIMIT 1").bind(id).fetch_optional(&mut *conn).await?;
    // User input is an explicit resume. A live lease consumes its private mail on the same run.
    sqlx::query("DELETE FROM work_suspensions WHERE work=?")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "UPDATE work_waits SET state='cancelled' WHERE work=? AND state IN ('proposed','armed')",
    )
    .bind(id)
    .execute(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM work_user_questions WHERE work=?")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    let run = match active {
        Some(r) => r,
        None => tree::wake(conn, id, manager, key).await?,
    };
    sqlx::query("INSERT OR IGNORE INTO main_dependencies VALUES(?,?)")
        .bind(&run)
        .bind(event)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE personal_work_revisions SET revision=revision+1 WHERE work=?")
        .bind(id)
        .execute(conn)
        .await?;
    Ok(json!({"input_id":mail,"event_id":event,"replay":false}))
}
pub async fn cancel(
    conn: &mut SqliteConnection,
    id: &str,
    manager: &str,
    key: &str,
    expected: i64,
) -> Result<Value> {
    let row=sqlx::query("SELECT r.revision,w.status,coalesce(x.original_actor,x.origin) AS actor FROM personal_work_revisions r JOIN work_nodes w ON w.id=r.work JOIN executions x ON x.id=w.owner_execution WHERE w.id=?").bind(id).fetch_one(&mut *conn).await?;
    if row.get::<i64, _>("revision") != expected
        || matches!(
            row.get::<&str, _>("status"),
            "completed" | "cancelled" | "failed"
        )
    {
        return Err("precondition_failed".into());
    }
    let actor = row.get::<&str, _>("actor");
    effects::authorize(conn, manager, actor, true).await?;
    let scope = tree::scope(conn, manager, actor, true).await?;
    control::stop(
        conn,
        &scope,
        id,
        manager,
        key,
        "cancelled",
        "Cancelled by original owner",
    )
    .await?;
    let revision: i64 =
        sqlx::query_scalar("SELECT revision FROM personal_work_revisions WHERE work=?")
            .bind(id)
            .fetch_one(conn)
            .await?;
    Ok(json!({"work_id":id,"state":"cancelled","revision":revision,"replay":false}))
}
