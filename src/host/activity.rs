//! Best-effort activity leases are scoped to a live turn and its original gateway.
use super::{domain, process::Session};
use crate::{Result, runtime::effects, store::Store};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::{collections::BTreeMap, time::Duration};

#[derive(Clone)]
pub struct Gate {
    pub execution: String,
    pub route: Value,
    pub enabled: bool,
}

async fn original(conn: &mut SqliteConnection, plugin: &str, execution: &str) -> Result<Value> {
    let row = sqlx::query("SELECT coalesce(x.original_actor,x.origin) AS actor,d.id AS entry,d.space,d.topic,a.native AS sender,b.native AS bot FROM executions x JOIN events e ON e.id=x.source JOIN entries d ON d.id=e.entry_id AND d.closed_at IS NULL JOIN accounts a ON a.id=e.account_id JOIN assignments s ON s.id=e.bot_stage AND s.kind='self' AND s.status='active' JOIN accounts b ON b.id=s.account_id JOIN host_installations i ON i.plugin=?1 AND i.bot_stage=s.id AND i.namespace=b.namespace AND i.platform=b.platform WHERE x.id=?2 AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)")
        .bind(plugin).bind(execution).fetch_one(&mut *conn).await?;
    effects::authorize(conn, execution, row.try_get("actor")?, true).await?;
    let group = row
        .try_get::<Option<String>, _>("space")?
        .map(|raw| serde_json::from_str::<(String, String, String)>(&raw))
        .transpose()?
        .map(|(_, _, native)| native);
    Ok(json!({"entry":row.try_get::<String,_>("entry")?,
        "bot":row.try_get::<String,_>("bot")?,
        "chat":group.unwrap_or(row.try_get("sender")?),
        "topic":row.try_get::<Option<String>,_>("topic")?}))
}

pub(super) async fn authorize(
    conn: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    gate: &Gate,
) -> Result<Value> {
    domain::active(conn, plugin, generation, "gateway.activity").await?;
    if gate.enabled && original(conn, plugin, &gate.execution).await? != gate.route {
        return Err("activity route changed".into());
    }
    Ok(Value::Null)
}

pub(super) async fn serve(store: Store, plugin: String, session: Session) {
    let mut live = store.active_turns.subscribe();
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut previous: BTreeMap<String, Value> = BTreeMap::new();
    loop {
        tokio::select! {
            changed = live.changed() => if changed.is_err() { break; },
            _ = tick.tick() => {},
        }
        if session.stopped() {
            break;
        }
        let executions: Vec<_> = live.borrow().keys().cloned().collect();
        let Ok(mut conn) = store.readers().acquire().await else {
            continue;
        };
        if domain::active(&mut conn, &plugin, &session.generation, "gateway.activity")
            .await
            .is_err()
        {
            previous.clear();
            continue;
        }
        let mut current = BTreeMap::new();
        for execution in executions {
            if let Ok(route) = original(&mut conn, &plugin, &execution).await {
                current.insert(execution, route);
            }
        }
        drop(conn);
        for (execution, route) in &previous {
            if !current.contains_key(execution) {
                session.activity(Gate {
                    execution: execution.clone(),
                    route: route.clone(),
                    enabled: false,
                });
            }
        }
        for (execution, route) in &current {
            session.activity(Gate {
                execution: execution.clone(),
                route: route.clone(),
                enabled: true,
            });
        }
        previous = current;
    }
}
