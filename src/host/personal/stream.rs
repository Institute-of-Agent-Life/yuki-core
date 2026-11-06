//! Commit hints have no payload. Each projection is read from the authorized owner.
use super::{
    credentials::{self, Principal, Purpose},
    http,
    selection::{self, Selection},
    views,
};
use crate::{Result, store::Store};
use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use futures_util::stream;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::{
    collections::{BTreeMap, VecDeque},
    convert::Infallible,
    time::Duration,
};
use tokio::sync::{broadcast, watch};

#[derive(Deserialize)]
pub(super) struct Filter {
    works: Option<String>,
}
type Frames = VecDeque<(&'static str, Value)>;
struct Subscription {
    store: Store,
    device: String,
    entry: String,
    active: bool,
    hints: broadcast::Receiver<u64>,
    shutdown: watch::Receiver<bool>,
    turns: watch::Receiver<BTreeMap<String, i64>>,
    previous: Value,
    pending: Frames,
    seq: u64,
}
async fn snapshot(
    db: &mut SqliteConnection,
    who: &Principal,
    active: bool,
    control: Option<&crate::operator::domain::Service>,
    live: &BTreeMap<String, i64>,
) -> Result<Value> {
    let ids:Vec<String>=sqlx::query_scalar("SELECT e.id FROM entries e JOIN assignments s ON s.id=e.actor_stage JOIN persons p ON p.id=s.original_person AND p.enabled WHERE e.scope='private' AND coalesce(p.current_root,p.id)=? ORDER BY s.id,e.id").bind(&who.root).fetch_all(&mut *db).await?;
    let mut conversations = Vec::new();
    let mut activity = Vec::new();
    for id in ids {
        conversations.push(views::entry(db, who, &id).await?);
        let trace:Option<(i64,String)>=sqlx::query_as("SELECT c.id,c.kind FROM control_trace_entries c JOIN main_turns t ON t.execution=c.execution WHERE t.conversation=? AND c.expires>unixepoch() ORDER BY c.id DESC LIMIT 1").bind(&id).fetch_optional(&mut *db).await?;
        let mut sources = Vec::new();
        if !live.is_empty() {
            let rows=sqlx::query("SELECT x.id,coalesce(o.kind,'user_message') AS origin FROM main_turns t JOIN executions x ON x.id=t.execution LEFT JOIN main_origins o ON o.ref=x.origin WHERE t.conversation=? AND NOT x.cancelled AND x.id IN (SELECT key FROM json_each(?)) ORDER BY x.id")
                .bind(&id).bind(serde_json::to_string(live)?).fetch_all(&mut *db).await?;
            for row in rows {
                let started = live[row.get::<&str, _>("id")];
                sources.push(json!({"origin":row.get::<&str,_>("origin"),"kind":"turn","started_at":credentials::stamp(started)?}));
            }
        }
        let step = trace.as_ref().map_or("turn", |(_, k)| match k.as_str() {
            "tool" => "tool",
            "model" => "model",
            _ => "turn",
        });
        activity.push(json!({"conversation_id":id,"revision":trace.as_ref().map_or(0,|r|r.0),"evidence":if sources.is_empty(){"insufficient"}else{"executing"},"step_kind":(!sources.is_empty()).then_some(step),"sources":sources}));
    }
    let rows=sqlx::query("SELECT w.id,r.revision,(SELECT count(*) FROM tool_charges c JOIN work_runs u ON u.execution=c.execution WHERE u.work=w.id) AS steps,(SELECT coalesce(max(c.id),0) FROM control_trace_entries c JOIN work_runs u ON u.execution=c.execution WHERE u.work=w.id AND c.kind='tool') AS last_step FROM work_nodes w JOIN personal_work_revisions r ON r.work=w.id JOIN persons p ON p.id=w.principal AND p.enabled WHERE coalesce(p.current_root,p.id)=? AND w.goal IS NOT NULL AND (NOT ? OR (w.parent IS NULL AND w.status NOT IN ('completed','cancelled','failed'))) AND NOT EXISTS(SELECT 1 FROM work_sources s JOIN invalidated_sources i ON i.source=s.event WHERE s.work=w.id) ORDER BY w.id").bind(&who.root).bind(active).fetch_all(&mut *db).await?;
    let works:Vec<Value>=rows.iter().map(|r|json!({"work_id":r.get::<&str,_>("id"),"revision":r.get::<i64,_>("revision"),"steps_token":format!("{}:{}",r.get::<i64,_>("steps"),r.get::<i64,_>("last_step"))})).collect();
    let mut view = json!({"conversations":conversations,"activity":activity,"works":works});
    if let Some(profile) = crate::operator::domain::get(db, "character-profile").await? {
        view["character"] = json!({"id":profile["id"],"revision":profile["revision"],"avatar_revision":profile["avatar_revision"]});
    }
    if view["character"].is_object()
        && let Some(control) = control
        && let Some(soul) =
            crate::operator::domain::personal_soul_revisions(db, control, &who.owner).await?
    {
        view["character"]["soul"] = soul;
    }
    Ok(view)
}
fn changes(old: &Value, new: &Value) -> Frames {
    let mut frames = Frames::new();
    if old == new {
        frames.push_back(("refresh", json!({})));
        return frames;
    }
    if old["character"] != new["character"] {
        frames.push_back(("character", new["character"].clone()));
    }
    for (category, key, kind) in [
        ("conversations", "conversation_id", "conversation"),
        ("works", "work_id", "work"),
        ("activity", "conversation_id", "activity"),
    ] {
        let map = |v: &Value| -> BTreeMap<String, Value> {
            v[category]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|row| Some((row[key].as_str()?.to_owned(), row.clone())))
                .collect()
        };
        let previous = map(old);
        let current = map(new);
        if previous.keys().any(|id| !current.contains_key(id)) {
            return Frames::from([("sync", new.clone())]);
        }
        for (id, row) in current {
            if previous.get(&id) != Some(&row) {
                frames.push_back((kind, row));
            }
        }
    }
    frames
}
#[derive(Debug, PartialEq)]
enum Ready {
    Emit,
    Snapshot,
}
/// A lagged receiver must disconnect even when it still has a local frame
/// queued. The lost commit could have revoked this device or hidden a row.
async fn ready(
    pending: bool,
    hints: &mut broadcast::Receiver<u64>,
    turns: &mut watch::Receiver<BTreeMap<String, i64>>,
    shutdown: &mut watch::Receiver<bool>,
) -> Option<Ready> {
    if *shutdown.borrow() {
        return None;
    }
    if !pending {
        tokio::select! {
            _=shutdown.changed()=>return None,
            result=hints.recv()=>{result.ok()?;},
            result=turns.changed()=>{result.ok()?;}
        }
    } else if hints.is_empty() && !turns.has_changed().ok()? {
        return Some(Ready::Emit);
    }
    loop {
        match hints.try_recv() {
            Ok(_) => (),
            Err(broadcast::error::TryRecvError::Empty) => return Some(Ready::Snapshot),
            Err(_) => return None,
        }
    }
}

impl Subscription {
    async fn read(&mut self) -> Result<Value> {
        let live = self.turns.borrow_and_update().clone();
        let mut tx = self.store.readers().begin().await?;
        let who = selection::delegated(
            &mut tx,
            &self.device,
            Purpose::Chat,
            Some(&self.entry),
            false,
        )
        .await?;
        snapshot(
            &mut tx,
            &who,
            self.active,
            self.store.control.as_deref(),
            &live,
        )
        .await
    }
    async fn next(&mut self) -> Option<Event> {
        loop {
            if ready(
                !self.pending.is_empty(),
                &mut self.hints,
                &mut self.turns,
                &mut self.shutdown,
            )
            .await?
                == Ready::Emit
            {
                let (kind, value) = self.pending.pop_front()?;
                self.seq += 1;
                return Event::default()
                    .event(kind)
                    .id(self.seq.to_string())
                    .json_data(&value)
                    .ok();
            }
            let fresh = self.read().await.ok()?;
            if !self.pending.is_empty() {
                // A change between frames can revoke a visible item. Resync the
                // remaining batch from the new snapshot; never send stale rows.
                if fresh != self.previous {
                    self.pending = Frames::from([("sync", fresh.clone())]);
                }
            } else {
                self.pending = changes(&self.previous, &fresh);
            }
            self.previous = fresh;
        }
    }
}
pub(super) async fn open(
    State(store): State<Store>,
    h: HeaderMap,
    Query(filter): Query<Filter>,
) -> Response {
    async fn start(store: Store, h: HeaderMap, filter: Filter) -> Result<Subscription> {
        let active = match filter.works.as_deref() {
            None => false,
            Some("active") => true,
            _ => return Err("validation_error".into()),
        };
        let shutdown = store.draining_http.subscribe();
        let hints = store.subscribe_commits();
        let mut turns = store.active_turns.subscribe();
        let live = turns.borrow_and_update().clone();
        let mut tx = store.readers().begin().await?;
        let who = selection::authenticate(
            &mut tx,
            &http::bearer(&h)?,
            Purpose::Chat,
            &Selection::headers(&h)?,
            false,
        )
        .await?;
        let view = snapshot(&mut tx, &who, active, store.control.as_deref(), &live).await?;
        Ok(Subscription {
            store,
            device: who.device,
            entry: who.entry.ok_or("chat_entry_required")?,
            active,
            hints,
            shutdown,
            turns,
            previous: view.clone(),
            pending: Frames::from([("sync", view)]),
            seq: 0,
        })
    }
    match start(store, h, filter).await {
        Err(e) => http::reply(Err(e), true),
        Ok(sub) => Sse::new(stream::unfold(sub, |mut s| async move {
            let event = s.next().await?;
            Some((Ok::<_, Infallible>(event), s))
        }))
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keepalive"),
        )
        .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn a_lagged_subscriber_cannot_flush_a_buffered_private_frame() {
        let (tx, mut slow) = broadcast::channel(2);
        let mut fast = tx.subscribe();
        let (_live, mut turns) = watch::channel(BTreeMap::new());
        let (_drain, mut shutdown) = watch::channel(false);
        for revision in 1..=3 {
            tx.send(revision).unwrap();
            assert_eq!(
                ready(true, &mut fast, &mut turns, &mut shutdown).await,
                Some(Ready::Snapshot)
            );
        }
        assert_eq!(
            ready(true, &mut slow, &mut turns, &mut shutdown).await,
            None
        );
        assert_eq!(
            ready(true, &mut fast, &mut turns, &mut shutdown).await,
            Some(Ready::Emit)
        );
    }
}
