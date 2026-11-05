use super::mailbox;
use crate::{
    Result,
    identity::new_id,
    runtime::{effects, execution, main::Config},
};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub struct Scope {
    pub principal: String,
    pub entry: String,
    pub work: Option<String>,
    pub child: bool,
}
pub async fn scope(
    conn: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    write: bool,
) -> Result<Scope> {
    effects::authorize(conn, exec, actor, write).await?;
    let (principal,entry):(String,String)=sqlx::query_as("WITH RECURSIVE lineage(id,n) AS (SELECT ?,0 UNION ALL SELECT o.parent,l.n+1 FROM lineage l JOIN executions x ON x.id=l.id JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL)
      SELECT coalesce(x.original_actor,x.origin),coalesce(e.entry_id,o.conversation) FROM lineage l JOIN executions x ON x.id=l.id LEFT JOIN events e ON e.id=x.source LEFT JOIN main_origins o ON o.ref=x.origin ORDER BY l.n DESC LIMIT 1")
       .bind(exec).fetch_one(&mut *conn).await?;
    let (work,child):(Option<String>,bool)=sqlx::query_as("SELECT x.work_id,coalesce(o.kind='child',0) FROM executions x LEFT JOIN main_origins o ON o.ref=x.origin WHERE x.id=?").bind(exec).fetch_one(conn).await?;
    Ok(Scope {
        principal,
        entry,
        work,
        child,
    })
}
fn tick() -> Result<i64> {
    Ok(i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis(),
    )?)
}
pub async fn authorize(conn: &mut SqliteConnection, s: &Scope, target: &str) -> Result<()> {
    let found:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM work_nodes w JOIN executions owner ON owner.id=w.owner_execution WHERE w.id=? AND w.entry=? AND w.principal=? AND w.goal IS NOT NULL
        AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=owner.source OR i.source IN(SELECT event FROM main_dependencies WHERE execution=owner.id)))")
        .bind(target).bind(&s.entry).bind(&s.principal).fetch_one(&mut *conn).await?;
    if !found {
        return Err("Work not accessible from this original source".into());
    }
    if s.child {
        let subtree:bool=sqlx::query_scalar("WITH RECURSIVE tree(id) AS(SELECT ? UNION SELECT w.id FROM work_nodes w JOIN tree t ON w.parent=t.id) SELECT EXISTS(SELECT 1 FROM tree WHERE id=?)")
            .bind(&s.work).bind(target).fetch_one(conn).await?;
        if !subtree {
            return Err("worker cannot manage a foreign branch".into());
        }
    }
    Ok(())
}
pub(super) async fn provenance(
    conn: &mut SqliteConnection,
    work: &str,
    source: &str,
) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO work_sources(work,event) SELECT ?1,id FROM events WHERE id IN(SELECT source FROM executions WHERE id=?2 UNION SELECT event FROM main_dependencies WHERE execution=?2)").bind(work).bind(source).execute(conn).await?;
    Ok(())
}
pub async fn detail(conn: &mut SqliteConnection, s: &Scope, id: &str) -> Result<Value> {
    authorize(conn, s, id).await?;
    let r =
        sqlx::query("SELECT id,root,parent,goal,status,note,summary FROM work_nodes WHERE id=?")
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
    let receipts=sqlx::query("SELECT a.rowid AS sequence,a.execution,a.ref,a.kind FROM work_activities a JOIN executions x ON x.id=a.execution WHERE a.work=? AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=x.source OR i.source IN(SELECT event FROM main_dependencies WHERE execution=x.id)) ORDER BY a.rowid DESC LIMIT 32")
        .bind(id).fetch_all(&mut *conn).await?;
    let mut activity = Vec::new();
    for item in receipts {
        activity.push(json!({"sequence":item.try_get::<i64,_>("sequence")?,"execution":item.try_get::<String,_>("execution")?,
            "operation":item.try_get::<String,_>("ref")?,"state":item.try_get::<String,_>("kind")?}));
    }
    Ok(
        json!({"work_id":id,"root_work_id":r.try_get::<String,_>("root")?,"parent_work_id":r.try_get::<Option<String>,_>("parent")?,"goal":r.try_get::<Option<String>,_>("goal")?,
        "status":r.try_get::<String,_>("status")?,"context_note":r.try_get::<Option<String>,_>("note")?.as_deref().map(serde_json::from_str::<Value>).transpose()?,"result":r.try_get::<Option<String>,_>("summary")?,"recent_activity":activity}),
    )
}
pub async fn create(
    conn: &mut SqliteConnection,
    cfg: &Config,
    exec: &str,
    s: &Scope,
    goal: &str,
    parent: Option<&str>,
    queued: bool,
) -> Result<String> {
    if goal.trim().is_empty() {
        return Err("Work goal is required".into());
    }
    let id = new_id(conn).await?;
    let (root, budget, profile) = if let Some(parent) = parent {
        authorize(conn, s, parent).await?;
        sqlx::query_as::<_,(String,String,String)>("SELECT root,budget,profile FROM work_nodes WHERE id=? AND status IN('active','need_input')").bind(parent).fetch_one(&mut *conn).await?
    } else {
        (
            id.clone(),
            sqlx::query_scalar("SELECT root FROM budget_members WHERE execution=?")
                .bind(exec)
                .fetch_one(&mut *conn)
                .await?,
            serde_json::to_string(cfg)?,
        )
    };
    sqlx::query("INSERT INTO work_nodes(id,root,parent,owner_execution,principal,entry,budget,profile,goal,status,updated) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
        .bind(&id).bind(root).bind(parent).bind(exec).bind(&s.principal).bind(&s.entry).bind(budget).bind(profile).bind(goal).bind(if queued{"queued"}else{"active"}).bind(tick()?).execute(&mut *conn).await?;
    provenance(conn, &id, exec).await?;
    Ok(id)
}
pub async fn attach(
    conn: &mut SqliteConnection,
    work: &str,
    exec: &str,
    key: &str,
    kind: &str,
) -> Result<()> {
    execution::attach(conn, exec, work).await?;
    sqlx::query("INSERT INTO work_runs VALUES(?,?,?,?)")
        .bind(exec)
        .bind(work)
        .bind(key)
        .bind(kind)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn wake(
    conn: &mut SqliteConnection,
    work: &str,
    manager: &str,
    occurrence: &str,
) -> Result<String> {
    if let Some(exec) =
        sqlx::query_scalar("SELECT execution FROM work_runs WHERE work=? AND occurrence=?")
            .bind(work)
            .bind(occurrence)
            .fetch_optional(&mut *conn)
            .await?
    {
        return Ok(exec);
    }
    let r=sqlx::query("SELECT owner_execution,parent,entry,budget,profile,goal FROM work_nodes WHERE id=? AND goal IS NOT NULL").bind(work).fetch_one(&mut *conn).await?;
    let cfg: Config = serde_json::from_str(r.try_get("profile")?)?;
    let delegated = r.try_get::<Option<String>, _>("parent")?.is_some();
    let registration = crate::runtime::main::Action::Register {
        kind: if delegated {
            "child".into()
        } else {
            "work".into()
        },
        id: format!("work:{work}:{manager}"),
        conversation: r.try_get("entry")?,
        parent: Some(manager.into()),
    };
    let exec =
        crate::runtime::main::delegate(conn, &cfg, registration, occurrence, r.try_get("goal")?)
            .await?;
    sqlx::query("INSERT OR IGNORE INTO main_dependencies(execution,event) SELECT ?1,id FROM events WHERE id IN (SELECT source FROM executions WHERE id=?2 UNION SELECT event FROM main_dependencies WHERE execution=?2)").bind(&exec).bind(r.try_get::<String,_>("owner_execution")?).execute(&mut *conn).await?;
    sqlx::query("UPDATE budget_members SET root=? WHERE execution=? AND NOT EXISTS(SELECT 1 FROM activations WHERE execution=?)").bind(r.try_get::<String,_>("budget")?).bind(&exec).bind(&exec).execute(&mut *conn).await?;
    sqlx::query("INSERT OR IGNORE INTO main_dependencies(execution,event) SELECT ?,event FROM work_sources WHERE work=?").bind(&exec).bind(work).execute(&mut *conn).await?;
    // Inherit the entire frozen execution view from the original parent, including hidden schemas.
    sqlx::query("INSERT INTO execution_tool_views SELECT ?,revision,code_projection,page_budget,storage_quota,timezone FROM execution_tool_views WHERE execution=? ON CONFLICT(execution) DO NOTHING").bind(&exec).bind(r.try_get::<String,_>("owner_execution")?).execute(&mut *conn).await?;
    attach(
        conn,
        work,
        &exec,
        occurrence,
        if delegated { "child" } else { "resume" },
    )
    .await?;
    sqlx::query("UPDATE work_nodes SET status='active',updated=? WHERE id=?")
        .bind(tick()?)
        .bind(work)
        .execute(conn)
        .await?;
    Ok(exec)
}
pub async fn terminal(
    conn: &mut SqliteConnection,
    id: &str,
    exec: &str,
    key: &str,
    status: &str,
    summary: &str,
) -> Result<()> {
    let row = sqlx::query("SELECT parent,status FROM work_nodes WHERE id=?")
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    if matches!(
        row.try_get::<String, _>("status")?.as_str(),
        "completed" | "failed" | "cancelled"
    ) {
        return Ok(());
    }
    sqlx::query(
        "UPDATE work_nodes SET status=?,summary=?,updated=?,terminal_operation=? WHERE id=?",
    )
    .bind(status)
    .bind(summary)
    .bind(tick()?)
    .bind(key)
    .bind(id)
    .execute(&mut *conn)
    .await?;
    sqlx::query("INSERT OR IGNORE INTO work_activities SELECT ?,o.namespace,o.state,o.operation_id FROM operations o JOIN work_runs r ON r.execution=o.namespace WHERE r.work=? AND o.state!='intent'").bind(id).bind(id).execute(&mut *conn).await?;
    provenance(conn, id, exec).await?;
    if status != "need_input"
        && let Some(parent) = row.try_get::<Option<String>, _>("parent")?
    {
        mailbox::put(conn, &parent, exec, key, "child_terminal", summary, None).await?;
    }
    Ok(())
}
