use super::*;
use std::{collections::BTreeMap, sync::Mutex};
use tokio::task::{AbortHandle, JoinHandle};

#[derive(Default)]
pub(super) struct Workers(Mutex<BTreeMap<&'static str, (bool, Option<AbortHandle>)>>);
impl Service {
    pub(crate) fn worker(&self, name: &'static str, enabled: bool, task: Option<&JoinHandle<()>>) {
        self.workers
            .0
            .lock()
            .expect("worker registry")
            .insert(name, (enabled, task.map(JoinHandle::abort_handle)));
    }
}
pub(super) async fn read(store: &Store, service: &Service) -> Result<Value> {
    let now =
        crate::host::personal::credentials::stamp(crate::host::personal::credentials::now()?)?;
    let mut rows:Vec<Value>=service.workers.0.lock().expect("worker registry").iter().map(|(name,(enabled,handle))|json!({
        "name":name,"enabled":enabled,"running":handle.as_ref().is_some_and(|h|!h.is_finished()),"healthy":null,"checked_at":now
    })).collect();
    let h = store.health().await;
    rows.push(json!({"name":"database","enabled":true,"running":null,"healthy":h.reader_ok&&h.writer_accepting,
        "read_available":h.reader_ok,"writer_accepting":h.writer_accepting,"checked_at":now}));
    if let Some(host) = &store.host {
        for mut row in host.runtime_health(store).await? {
            row["checked_at"] = json!(now);
            rows.push(row);
        }
    }
    // Core has no evidence of a remote execd completion receiver's process health.
    rows.push(json!({"name":"persistent_environment","enabled":null,"running":null,"healthy":null,
        "completion_receiver_running":null,"error_category":"external_health_not_reported","checked_at":now}));
    Ok(json!(rows))
}
