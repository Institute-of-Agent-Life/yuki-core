use super::{Inner, frame::id};
use crate::{
    Result,
    ingress::group_recovery::{self as domain, Recovery, Source},
    store::Mutation,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tokio::sync::oneshot;

pub(super) fn command(packet: &Value, bot: &str) -> bool {
    if packet["post_type"] != "message" {
        return false;
    }
    let text = match &packet["message"] {
        Value::String(s) => s.clone(),
        Value::Array(parts) => {
            let mut text = String::new();
            let mut addressed = false;
            for part in parts {
                match part["type"].as_str() {
                    Some("text") => text.push_str(part["data"]["text"].as_str().unwrap_or("")),
                    Some("at")
                        if !addressed
                            && text.trim().is_empty()
                            && id(&part["data"]["qq"]).as_deref() == Some(bot) =>
                    {
                        addressed = true
                    }
                    _ => return false,
                }
            }
            text
        }
        _ => return false,
    };
    text.split_whitespace()
        .map(str::to_ascii_lowercase)
        .eq(["/ai", "admin", "group", "current", "enable"])
}

type Topology = BTreeMap<String, u64>;
fn topology(inner: &Inner) -> Topology {
    inner
        .active
        .lock()
        .unwrap()
        .iter()
        .map(|(bot, p)| (bot.clone(), p.session))
        .collect()
}

async fn eligible(inner: &Inner, online: &Topology) -> Result<Topology> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT s.id FROM assignments s JOIN accounts a ON a.id=s.account_id WHERE s.kind='self'
      AND s.status='active' AND a.platform='qq' AND a.namespace=?",
    )
    .bind(&inner.namespace)
    .fetch_all(inner.store.readers())
    .await?;
    Ok(online
        .iter()
        .filter(|(b, _)| rows.contains(&inner.bots[*b].spec.stage))
        .map(|(b, s)| (b.clone(), *s))
        .collect())
}

async fn members(inner: &Inner, peers: &Topology, group: &str) -> Result<Vec<String>> {
    let group_num: i64 = group.parse()?;
    let mut pending = Vec::new();
    for (bot, session) in peers {
        let echo = format!(
            "member-{session}-{}",
            inner.next.fetch_add(1, Ordering::Relaxed)
        );
        let action = json!({"action":"get_group_member_info","params":{"user_id":bot.parse::<i64>()?,"group_id":group_num,"no_cache":true},"echo":echo});
        let (sender, reply) = oneshot::channel();
        let mut active = inner.active.lock().unwrap();
        let peer = active
            .get_mut(bot)
            .filter(|p| p.session == *session)
            .ok_or("connection changed")?;
        // Bound lost responses independently of socket lifetime.
        peer.probes.retain(|_, tx| !tx.is_closed());
        if peer.probes.len() >= 32 {
            return Err("probe limit".into());
        }
        peer.tx.try_send(action.to_string())?;
        peer.probes.insert(echo, sender);
        pending.push((bot, reply));
    }
    let mut found = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        for (bot, reply) in pending {
            let v: Value = reply.await?;
            if v["status"] != "ok" || v["retcode"] != 0 {
                return Err("membership unavailable".into());
            }
            let member = v["data"].as_object().ok_or("invalid membership result")?;
            if member.is_empty() {
                continue;
            }
            if id(&v["data"]["user_id"]).as_ref() != Some(bot)
                || (member.contains_key("group_id")
                    && id(&v["data"]["group_id"]).as_deref() != Some(group))
            {
                return Err("membership response belongs to a different target".into());
            }
            found.push(bot.clone());
        }
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await??;
    Ok(found)
}

async fn recover(inner: &Inner, s: Source) -> Result<&'static str> {
    let mut tx = inner.store.readers().begin().await?;
    let Some((actor_stage, person)) = domain::principal(&mut tx, &s).await? else {
        return Ok("denied");
    };
    if domain::replay(&mut tx, &s, &actor_stage, &person).await? {
        return Ok("enabled");
    }
    let before = domain::route(&mut tx, &s.namespace, &s.group).await?;
    drop(tx);
    let online = topology(inner);
    if online.get(&s.bot).copied() != Some(s.session as u64) {
        return Ok("conflict");
    }
    let candidates = eligible(inner, &online).await?;
    let pin = before
        .as_ref()
        .and_then(|(stage, _, _)| {
            inner
                .bots
                .iter()
                .find(|(_, e)| e.spec.stage == *stage)
                .map(|(id, _)| id.as_str())
        })
        .or_else(|| {
            inner
                .groups
                .contains_key(&s.group)
                .then_some(inner.designated.as_str())
        });
    let mut remaining = candidates.clone();
    let mut found = Vec::new();
    if let Some(pin) = pin
        && let Some(generation) = remaining.remove(pin)
    {
        let primary = Topology::from([(pin.to_owned(), generation)]);
        found = members(inner, &primary, &s.group).await?;
    }
    if found.is_empty() {
        found = members(inner, &remaining, &s.group).await?;
    }
    let chosen = match found.as_slice() {
        [one] => one,
        [] => return Ok("unavailable"),
        _ => return Ok("ambiguous"),
    };
    if chosen != &s.bot {
        return Ok("not_ingest");
    }
    // Serialize only the final topology check and local writer commit with socket replacement.
    let _pin = inner.topology.read().await;
    if topology(inner) != online {
        return Ok("conflict");
    }
    let stages = candidates
        .keys()
        .map(|id| inner.bots[id].spec.stage.clone())
        .collect();
    let result = inner
        .store
        .write(Mutation::RecoverGroup(Box::new(Recovery {
            source: s,
            actor_stage,
            person,
            before,
            online_stages: online
                .keys()
                .map(|id| inner.bots[id].spec.stage.clone())
                .collect(),
            eligible_stages: stages,
        })))
        .await?;
    Ok(match result.as_str() {
        Some("enabled") => "enabled",
        Some("denied") => "denied",
        _ => "conflict",
    })
}

pub(super) async fn handle(inner: Arc<Inner>, bot: String, session: u64, packet: Value) {
    // Personal or synthetic envelopes never acquire a native group control target.
    if packet["message_type"] != "group" {
        return;
    }
    let (Some(group), Some(sender), Some(message), Some(occurred)) = (
        id(&packet["group_id"]),
        id(&packet["user_id"]),
        id(&packet["message_id"]),
        packet["time"].as_i64().and_then(|n| n.checked_mul(1000)),
    ) else {
        return;
    };
    let Some(group_id) = group.parse::<i64>().ok().filter(|id| *id > 0) else {
        return;
    };
    if inner.bots.contains_key(&sender) || packet["sender"]["is_bot"] == true {
        return;
    }
    let source = Source {
        namespace: inner.namespace.clone(),
        group: group.clone(),
        bot: bot.clone(),
        stage: inner.bots[&bot].spec.stage.clone(),
        sender,
        native: message,
        occurred,
        session: session as i64,
    };
    let result = recover(&inner, source).await.unwrap_or("unavailable");
    if result == "not_ingest" {
        return;
    }
    if let Some(peer) = inner
        .active
        .lock()
        .unwrap()
        .get(&bot)
        .filter(|p| p.session == session)
    {
        let payload = json!({"action":"send_group_msg","params":{"group_id":group_id,"message":format!("group recovery: {result}")},
            "echo":format!("control-{session}-{}",inner.next.fetch_add(1,Ordering::Relaxed))});
        let _ = peer.tx.try_send(payload.to_string());
    }
}
