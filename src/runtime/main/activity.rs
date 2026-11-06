//! Process-local turn lifetimes, for read-only Client9 activity projections.
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::watch;

#[derive(Clone)]
pub(crate) struct Active(watch::Sender<BTreeMap<String, i64>>);

impl Default for Active {
    fn default() -> Self {
        Self(watch::channel(BTreeMap::new()).0)
    }
}
impl Active {
    pub(crate) fn subscribe(&self) -> watch::Receiver<BTreeMap<String, i64>> {
        self.0.subscribe()
    }
    pub(super) fn enter(&self, execution: &str) -> Turn {
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        self.0.send_modify(|turns| {
            turns.insert(execution.to_owned(), started);
        });
        Turn(self.clone(), execution.to_owned())
    }
}
pub(super) struct Turn(Active, String);
impl Drop for Turn {
    fn drop(&mut self) {
        self.0.0.send_modify(|turns| {
            turns.remove(&self.1);
        });
    }
}
