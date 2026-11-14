//! Process-local turn lifetimes, for read-only Client9 activity projections.
use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::watch;

#[derive(Clone)]
pub(crate) struct Active {
    turns: watch::Sender<BTreeMap<String, i64>>,
    foreground: watch::Sender<Foreground>,
}

struct Foreground {
    running: usize,
    last_activity: Instant,
    generation: u64,
}

impl Default for Active {
    fn default() -> Self {
        Self {
            turns: watch::channel(BTreeMap::new()).0,
            foreground: watch::channel(Foreground {
                running: 0,
                last_activity: Instant::now(),
                generation: 0,
            })
            .0,
        }
    }
}
impl Active {
    pub(crate) fn subscribe(&self) -> watch::Receiver<BTreeMap<String, i64>> {
        self.turns.subscribe()
    }

    pub(crate) fn foreground_idle(&self, seconds: u32) -> bool {
        let foreground = self.foreground.borrow();
        foreground.running == 0
            && foreground.last_activity.elapsed() >= Duration::from_secs(u64::from(seconds))
    }

    pub(crate) fn foreground_generation(&self) -> u64 {
        self.foreground.borrow().generation
    }

    pub(crate) async fn foreground_since(&self, generation: u64) {
        let mut watch = self.foreground.subscribe();
        let _ = watch.wait_for(|state| state.generation != generation).await;
    }

    pub(super) fn enter(&self, execution: &str, foreground: bool) -> Turn {
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        if foreground {
            self.foreground.send_modify(|activity| {
                activity.running += 1;
                activity.last_activity = Instant::now();
                activity.generation = activity.generation.wrapping_add(1);
            });
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
            self.0.foreground.send_modify(|activity| {
                activity.running -= 1;
                activity.last_activity = Instant::now();
            });
        }
        self.0.turns.send_modify(|turns| {
            turns.remove(&self.1);
        });
    }
}
