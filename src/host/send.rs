use super::{Hub, calls};
use crate::{Result, runtime::effects, social::destination::Refused, store::Store};
use serde_json::{Value, json};
use sqlx::Row;

impl Hub {
    pub async fn deliver(
        &self,
        store: &Store,
        exec: &str,
        actor: &str,
        operation: &str,
    ) -> Result<Option<Value>> {
        let mut tx = store.readers().begin().await?;
        effects::authorize(&mut tx, exec, actor, true).await?;
        let self_scene = match super::participation::self_binding(&mut tx, exec).await? {
            Some(scene) => Some(scene),
            None => super::notification::binding(&mut tx, exec).await?,
        };
        let self_entry = self_scene.as_ref().map(|(_, entry, _)| entry.as_str());
        let self_stage = self_scene.as_ref().map(|(_, _, stage)| stage.as_str());
        let route=sqlx::query("SELECT i.plugin,i.revision,i.enabled,i.online,i.grants,e.entry_id,e.bot_stage,d.scope,d.space,d.topic,a.native AS sender,b.native AS bot,o.payload,e.raw_actor AS recipient
            FROM executions x JOIN events e ON e.id=x.source JOIN entries d ON d.id=e.entry_id JOIN accounts a ON a.id=e.account_id
            JOIN assignments s ON s.id=e.bot_stage JOIN accounts b ON b.id=s.account_id
            JOIN host_installations i ON i.platform=b.platform AND i.namespace=b.namespace AND i.bot_stage=e.bot_stage
            JOIN operations o ON o.namespace=x.id AND o.operation_id=? AND o.target='send' WHERE x.id=?2
            UNION ALL
            SELECT i.plugin,i.revision,i.enabled,i.online,i.grants,d.id,s.id,d.scope,d.space,d.topic,coalesce(person_account.native,'') AS sender,b.native AS bot,o.payload,person.original_person AS recipient
            FROM entries d JOIN assignments s ON s.id=?4 JOIN accounts b ON b.id=s.account_id
            JOIN host_installations i ON i.platform=b.platform AND i.namespace=b.namespace AND i.bot_stage=s.id
            JOIN operations o ON o.namespace=?2 AND o.operation_id=?1 AND o.target='send'
            LEFT JOIN assignments person ON person.id=d.actor_stage LEFT JOIN accounts person_account ON person_account.id=person.account_id
            JOIN main_windows w ON w.conversation=d.id AND w.generation=coalesce(
                (SELECT window FROM main_turns WHERE execution=?2),
                (SELECT window FROM notification_publications WHERE delivery_execution=?2))
            WHERE d.id=?3 AND d.closed_at IS NULL")
            .bind(operation).bind(exec).bind(self_entry).bind(self_stage).fetch_optional(&mut *tx).await?;
        let Some(row) = route else { return Ok(None) };
        let parent = effects::receipt(&mut tx, exec, operation).await?;
        if matches!(
            parent["state"].as_str(),
            Some("confirmed" | "failed" | "unknown")
        ) {
            return Ok(Some(parent));
        }
        if !row.try_get::<bool, _>("enabled")? || !row.try_get::<bool, _>("online")? {
            return Err("original gateway delivery unavailable".into());
        }
        let grants: Vec<String> = serde_json::from_str(row.try_get("grants")?)?;
        if !grants.iter().any(|g| g == "gateway.send") {
            return Err(Refused("gateway_send_not_granted").into());
        }
        let message: Value = serde_json::from_str(row.try_get("payload")?)?;
        let group: Option<String> = row.try_get("space")?;
        let native_group = group
            .as_deref()
            .map(serde_json::from_str::<(String, String, String)>)
            .transpose()?
            .map(|(_, _, native)| native);
        if let Some(value) = message.get("target").filter(|v| !v.is_null()) {
            use crate::social::content::Target;
            let valid = match serde_json::from_value::<Target>(value.clone())? {
                Target::Canonical(v) => v.matches(&mut tx, row.get("entry_id")).await?,
                Target::Legacy(v) => match (group.as_deref(), v.kind.as_str()) {
                    (None, "person") => row.get::<Option<&str>, _>("recipient") == Some(&v.id),
                    (Some(space), "space") => {
                        space == v.id || native_group.as_deref() == Some(&v.id)
                    }
                    _ => false,
                },
            };
            if !valid {
                return Err(Refused("target_entry_mismatch").into());
            }
        }
        // File/media references belong to their owning handle subsystem, not the plugin filesystem.
        if !message["file"].is_null() || !message["emoji"].is_null() {
            return Err(Refused("gateway_media_unsupported").into());
        }
        let selected = if let Some(id) = message["artifact_id"].as_str() {
            if !grants.iter().any(|g| g == "media.open") {
                return Err(Refused("gateway_media_read_not_granted").into());
            }
            let (digest, kind): (String, String) = sqlx::query_as(
                "SELECT v.digest,v.kind FROM media_versions v JOIN media_version_readers r ON r.version=v.id WHERE v.id=? AND r.reader=?",
            )
            .bind(id)
            .bind(exec)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(Refused("gateway_artifact_unavailable"))?;
            if message["attachment_kind"] != kind {
                return Err(Refused("gateway_attachment_kind_mismatch").into());
            }
            Some(json!({"artifact_id":id,"version":digest}))
        } else {
            None
        };
        let address = json!({"entry":row.try_get::<String,_>("entry_id")?,"bot":row.try_get::<String,_>("bot")?,"chat":native_group.as_deref().unwrap_or(row.try_get("sender")?),"topic":row.try_get::<Option<String>,_>("topic")?});
        let plugin: String = row.try_get("plugin")?;
        let revision: String = row.try_get("revision")?;
        drop(tx);
        let request = |ordinal: i64, method: &str, name: &str, args: Value| calls::Request {
            vision: false,
            plugin: plugin.clone(),
            revision: revision.clone(),
            execution: exec.into(),
            operation: format!("{operation}:gateway:{ordinal}"),
            parent: Some(operation.into()),
            ordinal: Some(ordinal),
            method: method.into(),
            name: name.into(),
            args,
        };
        effects::invoke(
            store,
            effects::Action::Dispatch {
                execution: exec.into(),
                id: operation.into(),
                actor: actor.into(),
            },
        )
        .await?;
        let mut planner = request(
            0,
            "gateway.plan_send",
            "gateway.plan_send",
            json!({"route":address,"message":message}),
        );
        if let Some(media) = &selected {
            planner.args["media"] = media.clone();
        }
        let planned = self.execute(store, planner.clone()).await?;
        let (state, result) = if planned["state"] == "confirmed" {
            let stored = effects::invoke(
                store,
                effects::Action::Read {
                    execution: exec.into(),
                    actor: actor.into(),
                    id: planner.operation,
                },
            )
            .await?;
            let parts = stored["result"]["parts"]
                .as_array()
                .filter(|items| !items.is_empty() && items.len() <= 64)
                .ok_or("invalid original send plan")?;
            let media_parts: Vec<_> = parts.iter().filter_map(|part| part.get("media")).collect();
            if media_parts.len() != usize::from(selected.is_some())
                || media_parts
                    .first()
                    .is_some_and(|p| Some(*p) != selected.as_ref())
            {
                let failure =
                    json!({"state":"failed","error":"gateway_plan_media_mismatch","receipts":[]});
                return Ok(Some(
                    effects::invoke(
                        store,
                        effects::Action::Settle {
                            execution: exec.into(),
                            id: operation.into(),
                            outcome: effects::Outcome::Failed,
                            result: failure.to_string(),
                        },
                    )
                    .await?,
                ));
            }
            let mut receipts = Vec::new();
            let mut state = "confirmed";
            for (i, part) in parts.iter().enumerate() {
                let mut child = request(
                    i as i64 + 1,
                    "gateway.dispatch_part",
                    "gateway.dispatch_part",
                    json!({"route":address,"part":part,"part_ordinal":i}),
                );
                if let Some(media) = part.get("media") {
                    child.args["media"] = media.clone();
                    child.args["purpose"] = json!("deliver");
                }
                let outcome = self.execute(store, child.clone()).await?;
                if !matches!(
                    outcome["state"].as_str(),
                    Some("confirmed" | "failed" | "unknown")
                ) {
                    return Err("original gateway dispatch pending".into());
                }
                let receipt = effects::invoke(
                    store,
                    effects::Action::Read {
                        execution: exec.into(),
                        actor: actor.into(),
                        id: child.operation,
                    },
                )
                .await?;
                if receipt["state"] != "confirmed" {
                    state = if receipt["state"] == "failed" {
                        "failed"
                    } else {
                        "unknown"
                    };
                }
                receipts.push(receipt);
                if state != "confirmed" {
                    break;
                }
            }
            (state, json!({"parts":receipts,"planned":parts.len()}))
        } else if matches!(planned["state"].as_str(), Some("failed" | "unknown")) {
            (
                if planned["state"] == "failed" {
                    "failed"
                } else {
                    "unknown"
                },
                json!({"plan":planned}),
            )
        } else {
            return Err("original gateway plan pending".into());
        };
        let outcome = match state {
            "confirmed" => effects::Outcome::Confirmed,
            "failed" => effects::Outcome::Failed,
            _ => effects::Outcome::Unknown,
        };
        effects::invoke(
            store,
            effects::Action::Settle {
                execution: exec.into(),
                id: operation.into(),
                outcome,
                result: result.to_string(),
            },
        )
        .await?;
        Ok(Some(json!({"state":state,"result":result})))
    }
}
