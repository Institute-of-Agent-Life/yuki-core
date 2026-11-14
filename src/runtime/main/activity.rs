//! Process-local turn lifetimes, for read-only Client9 activity projections.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::watch;

#[derive(Clone)]
pub(crate) struct Active {
    turns: watch::Sender<BTreeMap<String, i64>>,
    foreground: Arc<Mutex<Foreground>>,
}

struct Foreground {
    running: usize,
    last_activity: Instant,
}

impl Default for Active {
    fn default() -> Self {
        Self {
            turns: watch::channel(BTreeMap::new()).0,
            foreground: Arc::new(Mutex::new(Foreground {
                running: 0,
                last_activity: Instant::now(),
            })),
        }
    }
}
impl Active {
    pub(crate) fn subscribe(&self) -> watch::Receiver<BTreeMap<String, i64>> {
        self.turns.subscribe()
    }

    pub(crate) fn foreground_idle(&self, seconds: u32) -> bool {
        let foreground = self.foreground.lock().expect("foreground state");
        foreground.running == 0
            && foreground.last_activity.elapsed() >= Duration::from_secs(u64::from(seconds))
    }

    pub(super) fn enter(&self, execution: &str, foreground: bool) -> Turn {
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        if foreground {
            let mut activity = self.foreground.lock().expect("foreground state");
            activity.running += 1;
            activity.last_activity = Instant::now();
        }
        self.turns.send_modify(|turns| {
            turns.insert(execution.to_owned(), started);
        });
        Turn(self.clone(), execution.to_owned(), foreground)
    }
}
pub(super) struct Turn(Active, String, bool);
impl Drop for Turn {
    fn drop(&mut self) {
        if self.2 {
            let mut activity = self.0.foreground.lock().expect("foreground state");
            activity.running -= 1;
            activity.last_activity = Instant::now();
        }
        self.0.turns.send_modify(|turns| {
            turns.remove(&self.1);
        });
    }
}
