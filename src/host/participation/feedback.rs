use super::{Change, Policy, sources};
use crate::Result;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub(super) async fn read(db: &mut SqliteConnection, c: &Change, policy: &Policy) -> Result<Value> {
    let executions: Vec<String> = if let Some(event) = c.params["event"].as_i64() {
        sources::checked(db, &c.plugin, policy, event).await?;
        if !sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM participation_promotions WHERE plugin=? AND event=?)",
        )
        .bind(&c.plugin)
        .bind(event)
        .fetch_one(&mut *db)
        .await?
        {
            return Err("event not promoted by original plugin".into());
        }
        sqlx::query_scalar("SELECT execution FROM main_inputs WHERE event=?1 UNION SELECT w.owner_execution FROM work_wait_routed r JOIN work_nodes w ON w.id=r.work WHERE r.event=?1 UNION SELECT w.wake_execution FROM work_wait_terms t JOIN work_waits w ON w.id=t.wait WHERE t.event=?1 AND w.wake_execution IS NOT NULL")
            .bind(event).fetch_all(&mut *db).await?
    } else {
        let key = c.params["key"]
            .as_str()
            .ok_or("original feedback key required")?;
        vec![
            sqlx::query_scalar(
                "SELECT execution FROM participation_proposals WHERE plugin=? AND proposal=?",
            )
            .bind(&c.plugin)
            .bind(key)
            .fetch_one(&mut *db)
            .await?,
        ]
    };
    let mut items = vec![];
    for execution in executions {
        let row=sqlx::query("SELECT x.state,x.cancelled,coalesce(x.original_actor,x.origin) AS actor FROM executions x WHERE id=?")
            .bind(&execution).fetch_one(&mut *db).await?;
        crate::runtime::effects::authorize(db, &execution, row.get("actor"), false).await?;
        let operations=sqlx::query("SELECT operation_id,state FROM operations WHERE namespace=? AND target='send' AND parent IS NULL ORDER BY operation_id")
            .bind(&execution).fetch_all(&mut *db).await?;
        let mut sends = vec![];
        for op in operations {
            let id: &str = op.get("operation_id");
            let confirmed:i64=sqlx::query_scalar("SELECT count(*) FROM social_parts WHERE namespace=? AND operation_id=? AND state='confirmed'")
                .bind(&execution).bind(id).fetch_one(&mut *db).await?;
            // External gateway receipts carry immutable per-part results in the parent receipt.
            let result: Option<String> = sqlx::query_scalar(
                "SELECT result FROM operations WHERE namespace=? AND operation_id=?",
            )
            .bind(&execution)
            .bind(id)
            .fetch_one(&mut *db)
            .await?;
            let receipt: Value = result
                .as_deref()
                .map(serde_json::from_str)
                .transpose()?
                .unwrap_or(Value::Null);
            let external = receipt["parts"].as_array().map_or(0, |parts| {
                parts.iter().filter(|p| p["state"] == "confirmed").count()
            });
            sends.push(json!({"operation":id,"state":op.get::<String,_>("state"),"confirmed_parts":confirmed+external as i64}));
        }
        let quiet = row.get::<String, _>("state") == "completed"
            && !row.get::<bool, _>("cancelled")
            && sends.is_empty();
        items.push(json!({"execution":execution,"state":row.get::<String,_>("state"),"cancelled":row.get::<bool,_>("cancelled"),"quiet":quiet,"sends":sends}));
    }
    Ok(json!({"scope":"original_execution","executions":items}))
}
