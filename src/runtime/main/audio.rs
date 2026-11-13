use crate::{Result, gateway::qq::Hub, store::Store};
use std::collections::BTreeSet;
use tokio::time::Instant;

pub(super) const MISSING: &str =
    "SELECT i.event,s.slot,x.original_actor,
       s.state IN('unprepared','cached') AND coalesce(s.expires>unixepoch('subsec')*1000,1) AS eligible
     FROM main_inputs i JOIN executions x ON x.id=i.execution
     JOIN media_sources s ON s.event=i.event
     WHERE i.execution=? AND NOT i.consumed AND x.original_actor IS NOT NULL
     AND s.kind IN('audio','record')
     AND NOT EXISTS(SELECT 1 FROM invalidated_sources f WHERE f.source=i.event)
     AND NOT EXISTS(SELECT 1 FROM history_transcripts t WHERE t.source=s.event AND t.segment=s.segment)
     ORDER BY i.event,s.slot";

/// Only fresh audio in an admitted human turn uses the decoder or paid route.
/// Keep the bounded wait outside the writer and out of other Main lanes.
pub(super) async fn prepare(
    store: &Store,
    gateway: &Hub,
    exec: &str,
    processor: &str,
    deadline: Instant,
) -> Result<BTreeSet<i64>> {
    let inputs: Vec<(i64, i64, String, bool)> = sqlx::query_as(MISSING)
        .bind(exec)
        .fetch_all(store.readers())
        .await?;
    let expected = inputs.iter().map(|item| item.0).collect();
    if inputs.is_empty()
        || Instant::now() >= deadline
        || crate::host::grants::methods()["methods"]
            .get("media.process")
            .is_none()
    {
        return Ok(expected);
    }
    let Some(host) = store.host.as_ref() else {
        return Ok(expected);
    };
    // An unavailable processor must not fetch an attachment or claim a model slot.
    let revision: Option<String> = sqlx::query_scalar(
        "SELECT i.revision FROM host_installations i
         JOIN events e ON e.id=(SELECT source FROM executions WHERE id=?1)
         WHERE i.plugin=?2 AND i.online AND i.enabled
         AND NOT EXISTS(SELECT 1 FROM owner_erasures f WHERE f.owner=i.plugin AND f.proof IS NULL)
         AND (SELECT count(DISTINCT g.value) FROM json_each(i.grants) g
              WHERE g.value IN('media.process','media.open','llm.complete','llm.result','forget.owner'))=5
         AND EXISTS(SELECT 1 FROM host_aux_routes r JOIN json_each(r.body,'$.entries') entries
              WHERE r.plugin=i.plugin AND r.revision=i.revision AND entries.value=e.entry_id
              AND EXISTS(SELECT 1 FROM json_each(r.body,'$.origins') v WHERE v.value='call')
              AND EXISTS(SELECT 1 FROM json_each(r.body,'$.purposes') v WHERE v.value='transcript')
              AND EXISTS(SELECT 1 FROM json_each(r.body,'$.media') v WHERE v.value='audio'))",
    )
    .bind(exec)
    .bind(processor)
    .fetch_optional(store.readers())
    .await?;
    let Some(revision) = revision else {
        return Ok(expected);
    };
    for (event, slot, actor, _) in inputs.into_iter().filter(|item| item.3) {
        // Leave a slot for the foreground request when ASR is optional.
        let spare_slot: bool = sqlx::query_scalar(
            "SELECT b.max_requests-b.reserved_requests>1
             FROM budget_members m JOIN root_budgets b USING(root) WHERE m.execution=?",
        )
        .bind(exec)
        .fetch_one(store.readers())
        .await?;
        if !spare_slot || Instant::now() >= deadline {
            break;
        }
        let Ok(permit) = store.audio_preparation.clone().try_acquire_owned() else {
            break;
        };
        let selected = tokio::time::timeout_at(deadline, async {
            let mut read = store.readers().begin().await?;
            crate::history::authorize_audio_source(&mut read, exec, &actor, event).await?;
            read.rollback().await?;
            crate::media::acquire(store, gateway, exec, &actor, event, slot, None).await
        })
        .await;
        match selected {
            Ok(Ok(_)) => {}
            Ok(Err(_)) => continue,
            Err(_) => break,
        }
        let host = host.clone();
        let store = store.clone();
        let processor = processor.to_owned();
        let revision = revision.clone();
        let exec = exec.to_owned();
        // Main may stop waiting while the original Host call still owns an
        // in-flight model request. Hold its slot through native settlement.
        let task = tokio::spawn(async move {
            let _permit = permit;
            let _ = host
                .transcribe(&store, &processor, revision, &exec, event, slot)
                .await;
        });
        if !matches!(tokio::time::timeout_at(deadline, task).await, Ok(Ok(()))) {
            break;
        }
    }
    Ok(expected)
}
