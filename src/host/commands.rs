use super::{Change, Hub, write};
use crate::{Result, gateway::qq, runtime::effects, store::Store};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::watch;

async fn report(
    hub: &Hub,
    store: &Store,
    qq: &qq::Hub,
    event: i64,
    path: &str,
    exec: &str,
    actor: &str,
) -> Result<()> {
    let operation = format!("command-unavailable:{event}");
    let notice = json!({"text":format!("{path} 暫時不可用。")});
    effects::invoke(
        store,
        effects::Action::Intent {
            execution: exec.into(),
            id: operation.clone(),
            actor: actor.into(),
            action: effects::Operation::External,
            target: "send".into(),
            payload: notice.to_string(),
            parent: None,
            ordinal: None,
        },
    )
    .await?;
    let receipt = match hub.deliver(store, exec, actor, &operation).await? {
        Some(out) => out,
        None => crate::social::deliver(store, qq, exec, &operation, actor).await?,
    };
    if let Some(state @ ("confirmed" | "unknown" | "failed")) = receipt["state"].as_str() {
        write(store, Change::Reported(event, state.into())).await?;
    }
    Ok(())
}
pub async fn serve(hub: Arc<Hub>, store: Store, qq: qq::Hub, mut end: watch::Receiver<bool>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
    loop {
        tokio::select! {_=end.changed()=>break,_=interval.tick()=>{}}
        let rows=sqlx::query_as::<_,(i64,String,String,String)>("SELECT c.event,c.path,c.execution,e.raw_actor FROM host_command_events c JOIN events e ON e.id=c.event
          WHERE c.state='unavailable' AND c.reply_state IS NULL AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id) ORDER BY c.event LIMIT 32").fetch_all(store.readers()).await;
        if let Ok(rows) = rows {
            for (event, path, exec, actor) in rows {
                let _ = report(&hub, &store, &qq, event, &path, &exec, &actor).await;
            }
        }
    }
}
