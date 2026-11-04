use super::Hub;
use crate::Result;
use serde_json::{Value, json};
use std::{sync::atomic::Ordering, time::Duration};
use tokio::sync::oneshot;

impl Hub {
    pub fn reply_limits(&self) -> crate::social::split::Limits {
        self.0.as_ref().map(|inner| inner.reply).unwrap_or_default()
    }
    pub fn session(&self, stage: &str) -> Option<u64> {
        let inner = self.0.as_ref()?;
        inner
            .active
            .lock()
            .unwrap()
            .iter()
            .find(|(bot, _)| inner.bots[*bot].spec.stage == stage)
            .map(|(_, p)| p.session)
    }
    /// Echo and response belong to one authenticated socket generation. A broken socket
    /// drops just its own waiters and never redirects an operation to a replacement bot.
    pub async fn request(&self, stage: &str, action: &str, params: Value) -> Result<Value> {
        self.request_bound(stage, action, params, None).await
    }

    pub async fn request_bound(
        &self,
        stage: &str,
        action: &str,
        params: Value,
        binding: Option<(u64, &str)>,
    ) -> Result<Value> {
        let inner = self.0.as_ref().ok_or("QQ not configured")?;
        let (bot, _) = inner
            .bots
            .iter()
            .find(|(_, e)| e.spec.stage == stage)
            .ok_or("original receiver unavailable")?;
        let reply = {
            let mut active = inner.active.lock().unwrap();
            let peer = active.get_mut(bot).ok_or("original receiver offline")?;
            peer.probes.retain(|_, tx| !tx.is_closed());
            if peer.probes.len() >= 32 {
                return Err("receiver busy".into());
            }
            let echo = if let Some((session, echo)) = binding {
                if peer.session != session {
                    return Err("original connection replaced".into());
                }
                echo.to_owned()
            } else {
                format!(
                    "rpc-{}-{}",
                    peer.session,
                    inner.next.fetch_add(1, Ordering::Relaxed)
                )
            };
            let (tx, rx) = oneshot::channel();
            peer.tx
                .try_send(json!({"action":action,"params":params,"echo":echo}).to_string())?;
            peer.probes.insert(echo, tx);
            rx
        };
        Ok(tokio::time::timeout(Duration::from_secs(3), reply).await??)
    }
}
