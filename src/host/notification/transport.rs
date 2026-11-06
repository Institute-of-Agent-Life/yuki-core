use super::{Change, scope};
use crate::{Result, gateway::qq, host::Hub, store::Store};
use sqlx::{Row, SqliteConnection};
use std::sync::Arc;
use tokio::sync::watch;

pub(crate) async fn authorize(db: &mut SqliteConnection, execution: &str) -> Result<()> {
    let rows = sqlx::query(
        "WITH RECURSIVE family(id) AS(SELECT ? UNION SELECT o.parent FROM family f
         JOIN executions x ON x.id=f.id JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL)
         SELECT p.*,x.id AS family_execution FROM family f JOIN executions x ON x.id=f.id
         JOIN notification_publications p ON p.origin=x.origin",
    ).bind(execution).fetch_all(&mut *db).await?;
    for row in rows {
        let (_, policy) = scope::policy(db, row.get("plugin")).await?;
        let target = scope::entry(db, &policy, row.get("entry")).await?;
        if target.receiver != row.get::<String, _>("receiver")
            || target.window != row.get::<i64, _>("window")
            || row.get::<&str, _>("state") != "published"
            || (!policy.agent
                && row.get::<Option<&str>, _>("agent_execution")
                    == Some(row.get("family_execution")))
        {
            return Err("original notification grant or publication unavailable".into());
        }
        let revoked: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM notification_sources s JOIN events e ON e.id=s.source
             LEFT JOIN persons p ON p.id=e.raw_actor LEFT JOIN persons r ON r.id=coalesce(p.current_root,p.id)
             WHERE s.publication=? AND ((e.raw_actor IS NOT NULL AND (NOT p.enabled OR NOT r.enabled))
               OR EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)))",
        ).bind(row.get::<&str, _>("id")).fetch_one(&mut *db).await?;
        if revoked {
            return Err("original notification source unavailable".into());
        }
    }
    Ok(())
}

pub(crate) async fn binding(
    db: &mut SqliteConnection,
    execution: &str,
) -> Result<Option<(String, String, String)>> {
    let found = sqlx::query_as(
        "WITH RECURSIVE family(id) AS(SELECT ? UNION SELECT o.parent FROM family f
         JOIN executions x ON x.id=f.id JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL)
         SELECT p.origin,p.entry,p.receiver FROM family f JOIN executions x ON x.id=f.id
         JOIN notification_publications p ON p.origin=x.origin LIMIT 1",
    ).bind(execution).fetch_optional(&mut *db).await?;
    if found.is_some() {
        authorize(db, execution).await?;
    }
    Ok(found)
}

pub(crate) async fn qq_route(
    db: &mut SqliteConnection,
    execution: &str,
) -> Result<Option<crate::social::send::Route>> {
    let Some((origin, entry, receiver)) = binding(db, execution).await? else {
        return Ok(None);
    };
    let row = sqlx::query(
        "SELECT e.scope,e.space,e.actor_stage,a.original_person,h.native,b.platform,b.namespace,
                coalesce((SELECT generation FROM main_windows WHERE conversation=e.id),1) AS window
         FROM entries e JOIN assignments bot ON bot.id=? JOIN accounts b ON b.id=bot.account_id
         LEFT JOIN assignments a ON a.id=e.actor_stage LEFT JOIN accounts h ON h.id=a.account_id
         WHERE e.id=?",
    )
    .bind(&receiver)
    .bind(&entry)
    .fetch_one(&mut *db)
    .await?;
    if row.get::<&str, _>("platform") != "qq" {
        return Ok(None);
    }
    let group = row.get::<&str, _>("scope") == "group";
    let (target, revision) = if group {
        let address: (String, String, String) = serde_json::from_str(row.get("space"))?;
        let route = super::super::participation::qq_route(db, &address.1, &address.2)
            .await?
            .ok_or("notification group receiver unavailable")?;
        (address.2.parse()?, Some(route.1))
    } else {
        (row.get::<&str, _>("native").parse()?, None)
    };
    Ok(Some(crate::social::send::Route {
        entry,
        person: row
            .get::<Option<String>, _>("original_person")
            .unwrap_or(origin),
        stage: receiver,
        target,
        private_stage: row.get("actor_stage"),
        group,
        window: row.get("window"),
        revision,
    }))
}

pub(crate) async fn serve(
    hub: Arc<Hub>,
    store: Store,
    qq: qq::Hub,
    mut end: watch::Receiver<bool>,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
    loop {
        tokio::select! { _=end.changed()=>break, _=interval.tick()=>{} }
        let rows = sqlx::query(
            "SELECT d.publication,d.ordinal,d.execution,d.operation,p.origin,o.payload
             FROM notification_deliveries d JOIN notification_publications p ON p.id=d.publication
             JOIN operations o ON o.namespace=d.execution AND o.operation_id=d.operation
             WHERE d.state='pending' AND p.state='published'
             AND NOT EXISTS(SELECT 1 FROM notification_deliveries prior
                 WHERE prior.publication=d.publication AND prior.ordinal<d.ordinal AND prior.state!='confirmed')
             ORDER BY p.event,d.ordinal LIMIT 32",
        ).fetch_all(store.readers()).await;
        if let Ok(rows) = rows {
            for row in rows {
                let exec: &str = row.get("execution");
                let operation: &str = row.get("operation");
                let actor: &str = row.get("origin");
                let result = match hub.deliver(&store, exec, actor, operation).await {
                    Ok(Some(receipt)) => Ok(receipt),
                    Ok(None) => {
                        async {
                            crate::media::prepare_host_delivery(
                                &store,
                                exec,
                                actor,
                                row.get("payload"),
                            )
                            .await?;
                            crate::social::deliver(&store, &qq, exec, operation, actor).await
                        }
                        .await
                    }
                    Err(error) => Err(error),
                };
                if let Err(error) = &result {
                    let category = match error.to_string().as_str() {
                        "original receiver offline before dispatch" => Some("bot_unavailable"),
                        "original gateway delivery unavailable" => Some("gateway_disconnected"),
                        _ => None,
                    };
                    if let Some(category) = category {
                        let _ = super::super::write(
                            &store,
                            super::super::Change::Notification(Change::Refused {
                                publication: row.get("publication"),
                                ordinal: row.get("ordinal"),
                                category: category.into(),
                            }),
                        )
                        .await;
                    }
                }
                if let Ok(result) = result
                    && let Some(state @ ("confirmed" | "failed" | "unknown")) =
                        result["state"].as_str()
                {
                    let _ = super::super::write(
                        &store,
                        super::super::Change::Notification(Change::Delivered {
                            publication: row.get("publication"),
                            ordinal: row.get("ordinal"),
                            state: state.into(),
                        }),
                    )
                    .await;
                }
            }
        }
    }
}
