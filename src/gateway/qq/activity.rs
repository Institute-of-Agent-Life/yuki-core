use super::{Inner, Peer};
use crate::Result;
use serde_json::{Value, json};
use sqlx::Row;
use std::{collections::HashSet, sync::atomic::Ordering};

pub(super) struct Leases {
    user: i64,
    holders: HashSet<String>,
}
fn send(inner: &Inner, peer: &mut Peer, user: i64, enabled: bool) {
    let echo = format!(
        "typing-{}-{}",
        peer.session,
        inner.next.fetch_add(1, Ordering::Relaxed)
    );
    // Lost UI receipts must not block later turns or grow memory indefinitely.
    if peer.echoes.len() >= 32 {
        peer.echoes.clear();
    }
    let action = json!({"action":"set_input_status","params":{"user_id":user,"event_type":i32::from(enabled)},"echo":echo});
    if peer.tx.try_send(action.to_string()).is_ok() {
        peer.echoes.insert(echo);
    }
}

async fn current_user(inner: &Inner, entry: &str) -> Result<Option<i64>> {
    let native:Option<String>=sqlx::query_scalar("SELECT a.native FROM entries e JOIN assignments s ON e.actor_stage=s.id
       JOIN accounts a ON a.id=s.account_id JOIN assignments b ON b.id=e.bot_stage
       JOIN persons p ON p.id=s.original_person JOIN persons r ON r.id=coalesce(p.current_root,p.id)
       WHERE e.id=? AND e.closed_at IS NULL AND s.status='active' AND b.status='active' AND p.enabled AND r.enabled")
       .bind(entry).fetch_optional(inner.store.readers()).await?;
    Ok(native.map(|id| id.parse()).transpose()?)
}

pub(super) async fn refresh(inner: &Inner, bot: &str, session: u64) {
    let keys: Vec<_> = inner
        .active
        .lock()
        .unwrap()
        .get(bot)
        .filter(|p| p.session == session)
        .map(|p| p.leases.keys().cloned().collect())
        .unwrap_or_default();
    for entry in keys {
        let user = current_user(inner, &entry).await.ok().flatten();
        let mut active = inner.active.lock().unwrap();
        let Some(peer) = active.get_mut(bot).filter(|p| p.session == session) else {
            return;
        };
        if !peer.leases.contains_key(&entry) {
            continue;
        }
        if let Some(user) = user {
            send(inner, peer, user, true);
        } else {
            peer.leases.remove(&entry);
        }
    }
}

pub(super) async fn change(inner: &Inner, event: i64, lease: String, begin: bool) -> Result<Value> {
    let event=sqlx::query("SELECT e.entry_id,e.bot_stage,entry.scope FROM events e JOIN entries entry ON entry.id=e.entry_id WHERE e.id=?")
        .bind(event).fetch_one(inner.store.readers()).await?;
    let stage: String = event.try_get("bot_stage")?;
    let Some((bot, _)) = inner
        .bots
        .iter()
        .find(|(_, b)| b.spec.stage == stage && b.spec.typing_private)
    else {
        return Ok(json!({"supported":false}));
    };
    if event.try_get::<String, _>("scope")? != "private" {
        return Ok(json!({"supported":false}));
    }
    let entry: String = event.try_get("entry_id")?;
    let user = current_user(inner, &entry).await?;
    let current_is_valid = user.is_some();
    let mut active = inner.active.lock().unwrap();
    let Some(peer) = active.get_mut(bot) else {
        return Ok(json!({"supported":true,"online":false}));
    };
    if begin {
        let user = user.ok_or("typing route closed")?;
        let state = peer.leases.entry(entry).or_insert_with(|| Leases {
            user,
            holders: HashSet::new(),
        });
        let start = state.holders.is_empty();
        state.holders.insert(lease);
        if start {
            send(inner, peer, user, true)
        }
    } else if let Some(state) = peer.leases.get_mut(&entry) {
        state.holders.remove(&lease);
        if state.holders.is_empty() {
            let user = state.user;
            peer.leases.remove(&entry);
            if current_is_valid {
                send(inner, peer, user, false);
            }
        }
    }
    Ok(json!({"supported":true,"online":true}))
}
