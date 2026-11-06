use super::{Change, Parent, snapshot};
use crate::{Result, identity::new_id, runtime::effects, store::Store};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub(super) async fn intent(
    db: &mut SqliteConnection,
    parent: &Parent,
    p: &Value,
    prepared: Option<&snapshot::Prepared>,
) -> Result<Value> {
    let ordinal = p["ordinal"].as_i64().ok_or("missing ordinal")?;
    let feed = p["feed_index"].as_i64().ok_or("missing feed")?;
    let checkpoint = p["checkpoint"].as_i64().ok_or("missing checkpoint")?;
    let engine = p["engine_call_id"].as_str().ok_or("missing engine call")?;
    let snapshot = p["snapshot"].as_str().ok_or("missing snapshot")?;
    let name = p["name"].as_str().ok_or("missing tool")?;
    let raw:String=sqlx::query_scalar("SELECT m.body FROM execution_tool_views v JOIN tool_manifests m USING(revision) WHERE v.execution=?")
        .bind(&parent.exec).fetch_one(&mut *db).await?;
    let manifest: crate::tools::registry::Manifest = serde_json::from_str(&raw)?;
    let tool = manifest
        .0
        .get(name)
        .ok_or("tool absent from original frozen manifest")?;
    if !jsonschema::validator_for(&tool.schema)?.is_valid(&p["arguments"]) {
        return Err("original child schema rejected arguments".into());
    }
    let normalized = crate::model::chat::intent(engine, name, p["arguments"].clone())?;
    if let Some(r)=sqlx::query("SELECT c.*,o.action,o.target,o.payload FROM code_children c JOIN operations o ON o.namespace=c.execution AND o.operation_id=c.operation WHERE c.handle=? AND c.ordinal=?")
        .bind(&parent.handle).bind(ordinal).fetch_optional(&mut *db).await? {
        if r.get::<i64,_>("feed_index")!=feed || r.get::<&str,_>("engine_call_id")!=engine || r.get::<&str,_>("snapshot")!=snapshot
            || r.get::<i64,_>("checkpoint")!=checkpoint+1 || r.get::<&str,_>("action")!=normalized["action"]
            || r.get::<&str,_>("target")!=normalized["target"] || serde_json::from_str::<Value>(r.get("payload"))?!=p["arguments"] {
            return Err("original child identity conflicts".into());
        }
        return state(db,&parent.handle,ordinal).await;
    }
    if checkpoint != parent.checkpoint
        || ordinal != parent.next_ordinal
        || feed < parent.feed_index
        || ordinal >= parent.runtime.max_suspensions
    {
        return Err("original VM position or resource boundary rejected".into());
    }
    let unresolved=sqlx::query("SELECT count(*) AS pending,coalesce(sum(o.state='unknown'),0) AS unknowns FROM code_children c JOIN operations o ON o.namespace=c.execution AND o.operation_id=c.operation WHERE c.handle=? AND c.fed_snapshot IS NULL")
        .bind(&parent.handle).fetch_one(&mut *db).await?;
    if unresolved.get::<i64, _>("unknowns") > 0 {
        return Err("original child unknown; retain partial composition".into());
    }
    if unresolved.get::<i64, _>("pending") >= parent.runtime.max_pending_futures {
        return Err("pending VM futures at capacity".into());
    }
    if let Some(provider) = &tool.provider {
        let cycle:bool=sqlx::query_scalar("WITH RECURSIVE ancestors(op) AS(SELECT ?2 UNION SELECT o.parent FROM ancestors a JOIN operations o ON o.namespace=?1 AND o.operation_id=a.op WHERE o.parent IS NOT NULL) SELECT EXISTS(SELECT 1 FROM ancestors a JOIN host_calls c ON c.execution=?1 AND c.operation=a.op WHERE c.plugin=?3)")
            .bind(&parent.exec).bind(&parent.operation).bind(&provider.id).fetch_one(&mut *db).await?;
        if cycle {
            return Err("actual plugin ancestor cycle".into());
        }
    }
    snapshot::publish(db, parent, snapshot, prepared).await?;
    let op = new_id(db).await?;
    effects::apply(
        db,
        &effects::Action::Intent {
            execution: parent.exec.clone(),
            actor: parent.actor.clone(),
            id: op.clone(),
            action: serde_json::from_value(normalized["action"].clone())?,
            target: normalized["target"]
                .as_str()
                .ok_or("missing target")?
                .into(),
            payload: p["arguments"].to_string(),
            parent: Some(parent.operation.clone()),
            ordinal: Some(ordinal),
        },
    )
    .await?;
    sqlx::query("INSERT INTO code_children(handle,ordinal,execution,operation,feed_index,engine_call_id,snapshot,checkpoint,admitted_at) VALUES(?,?,?,?,?,?,?,?,CAST(unixepoch('subsec')*1000 AS INTEGER))")
        .bind(&parent.handle).bind(ordinal).bind(&parent.exec).bind(&op).bind(feed).bind(engine).bind(snapshot).bind(checkpoint+1).execute(&mut *db).await?;
    sqlx::query("UPDATE code_parents SET checkpoint=checkpoint+1,next_ordinal=next_ordinal+1,feed_index=?,snapshot=? WHERE handle=?")
        .bind(feed).bind(snapshot).bind(&parent.handle).execute(&mut *db).await?;
    snapshot::prune(db, &parent.handle).await?;
    state(db, &parent.handle, ordinal).await
}

async fn state(db: &mut SqliteConnection, handle: &str, ordinal: i64) -> Result<Value> {
    let r=sqlx::query("SELECT c.*,o.state,p.checkpoint AS current_checkpoint,p.closed,x.cancelled FROM code_children c JOIN operations o ON o.namespace=c.execution AND o.operation_id=c.operation JOIN code_parents p ON p.handle=c.handle JOIN executions x ON x.id=c.execution WHERE c.handle=? AND c.ordinal=?")
        .bind(handle).bind(ordinal).fetch_one(db).await?;
    let receipt = r
        .get::<Option<String>, _>("receipt")
        .map(|v| serde_json::from_str::<Value>(&v))
        .transpose()?;
    let status = r.get::<&str, _>("state");
    let fed = r.get::<Option<String>, _>("fed_snapshot");
    Ok(
        json!({"handle":handle,"ordinal":ordinal,"operation":r.get::<&str,_>("operation"),"execution":r.get::<&str,_>("execution"),"state":status,
        "feed_index":r.get::<i64,_>("feed_index"),"engine_call_id":r.get::<&str,_>("engine_call_id"),"snapshot":r.get::<&str,_>("snapshot"),"checkpoint":r.get::<i64,_>("current_checkpoint"),"intent_checkpoint":r.get::<i64,_>("checkpoint")-1,
        "receipt":receipt,"fed":fed.is_some(),"fed_snapshot":fed,"closed":r.get::<bool,_>("closed"),"can_feed":matches!(status,"confirmed"|"failed") && receipt.is_some() && fed.is_none() && !r.get::<bool,_>("closed") && !r.get::<bool,_>("cancelled")}),
    )
}

pub(super) async fn enter(
    db: &mut SqliteConnection,
    parent: &Parent,
    generation: &str,
    ordinal: i64,
) -> Result<Value> {
    let row=sqlx::query("SELECT c.operation,c.entered_generation,o.state FROM code_children c JOIN operations o ON o.namespace=c.execution AND o.operation_id=c.operation WHERE c.handle=? AND c.ordinal=?")
        .bind(&parent.handle).bind(ordinal).fetch_one(&mut *db).await?;
    let entered = row.get::<Option<String>, _>("entered_generation");
    let operation: String = row.get("operation");
    let mut status: String = row.get("state");
    if status == "dispatched" && entered.as_deref() != Some(generation) {
        sqlx::query("UPDATE operations SET state='unknown',result='null' WHERE namespace=? AND operation_id=? AND state='dispatched'")
            .bind(&parent.exec).bind(&operation).execute(&mut *db).await?;
        status = "unknown".into();
    }
    let resumable = status=="unknown" && sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM code_parents WHERE execution=? AND operation=? AND snapshot IS NOT NULL AND NOT closed)")
        .bind(&parent.exec).bind(&operation).fetch_one(&mut *db).await?;
    // A saved nested composition restores its VM. Other unknown physical children remain unknown.
    let dispatch = (status == "intent" || resumable) && entered.as_deref() != Some(generation);
    if dispatch {
        sqlx::query("UPDATE code_children SET entered_at=CAST(unixepoch('subsec')*1000 AS INTEGER),entered_generation=? WHERE handle=? AND ordinal=?")
            .bind(generation).bind(&parent.handle).bind(ordinal).execute(db).await?;
    }
    Ok(
        json!({"dispatch":dispatch,"execution":parent.exec,"operation":row.get::<&str,_>("operation")}),
    )
}

pub(super) async fn drive(
    store: &Store,
    plugin: &str,
    generation: &str,
    handle: &str,
    ordinal: i64,
) -> Result<Value> {
    let enter = super::write(
        store,
        super::super::Change::Composition(Box::new(Change::Enter(
            plugin.into(),
            generation.into(),
            handle.into(),
            ordinal,
        ))),
    )
    .await?;
    if enter["dispatch"] == true {
        #[cfg(test)]
        crate::store::tests::commit_barrier("code-executor-entered").await;
        let hub = store.host.as_ref().ok_or("Host unavailable")?;
        crate::runtime::main::dispatch_child(
            store,
            enter["execution"]
                .as_str()
                .ok_or("missing original execution")?,
            enter["operation"].as_str().ok_or("missing child")?,
            &hub.qq,
        )
        .await?;
    }
    let value = super::write(
        store,
        super::super::Change::Composition(Box::new(Change::Capture(handle.into(), ordinal))),
    )
    .await?;
    #[cfg(test)]
    crate::store::tests::commit_barrier("code-T3-committed").await;
    Ok(value)
}

pub(super) async fn capture(
    db: &mut SqliteConnection,
    handle: &str,
    ordinal: i64,
) -> Result<Value> {
    let row=sqlx::query("SELECT c.execution,c.operation,c.receipt,h.plugin,coalesce(x.original_actor,x.origin) AS actor,x.source,o.state FROM code_children c JOIN host_calls h ON h.handle=c.handle JOIN executions x ON x.id=c.execution JOIN operations o ON o.namespace=c.execution AND o.operation_id=c.operation WHERE c.handle=? AND c.ordinal=?")
        .bind(handle).bind(ordinal).fetch_one(&mut *db).await?;
    let exec: String = row.get("execution");
    if crate::privacy::source::execution_erased(db, &exec, row.get("source")).await? {
        return Ok(
            json!({"handle":handle,"ordinal":ordinal,"state":row.get::<&str,_>("state"),"erased":true,"can_feed":false}),
        );
    }
    if row.get::<Option<String>, _>("receipt").is_none()
        && matches!(
            row.get::<&str, _>("state"),
            "confirmed" | "failed" | "unknown"
        )
    {
        let receipt = crate::model::attempt::tool_receipt(
            db,
            &exec,
            row.get("actor"),
            row.get("operation"),
            Some((yuki_plugin_sdk::MAX_FRAME_BYTES - 8192) / 4),
        )
        .await?;
        let sources:Vec<i64>=sqlx::query_scalar("SELECT id FROM events WHERE id IN(SELECT source FROM executions WHERE id=?1 UNION SELECT event FROM main_dependencies WHERE execution=?1)").bind(&exec).fetch_all(&mut *db).await?;
        for event in sources {
            crate::privacy::source::acquire(db, row.get("plugin"), handle, event).await?;
        }
        sqlx::query(
            "UPDATE code_children SET receipt=? WHERE handle=? AND ordinal=? AND receipt IS NULL",
        )
        .bind(receipt.to_string())
        .bind(handle)
        .bind(ordinal)
        .execute(&mut *db)
        .await?;
    }
    state(db, handle, ordinal).await
}

pub(super) async fn ack(
    db: &mut SqliteConnection,
    parent: &Parent,
    p: &Value,
    prepared: Option<&snapshot::Prepared>,
) -> Result<Value> {
    let ordinal = p["ordinal"].as_i64().ok_or("missing original ordinal")?;
    let original = state(db, &parent.handle, ordinal).await?;
    if original["feed_index"] != p["feed_index"]
        || original["engine_call_id"] != p["engine_call_id"]
    {
        return Err("original VM call position conflicts".into());
    }
    if p["ack"].is_null() {
        return capture(db, &parent.handle, ordinal).await;
    }
    let a = &p["ack"];
    let snapshot = a["snapshot"].as_str().ok_or("missing fed snapshot")?;
    let checkpoint = a["checkpoint"].as_i64().ok_or("missing checkpoint")?;
    let feed = a["feed_index"].as_i64().ok_or("missing feed index")?;
    let old=sqlx::query("SELECT fed_snapshot,fed_checkpoint,fed_feed_index,feed_index,receipt FROM code_children WHERE handle=? AND ordinal=?")
        .bind(&parent.handle).bind(ordinal).fetch_one(&mut *db).await?;
    if let Some(fed) = old.get::<Option<String>, _>("fed_snapshot") {
        if fed != snapshot
            || old.get::<Option<i64>, _>("fed_checkpoint") != Some(checkpoint + 1)
            || old.get::<Option<i64>, _>("fed_feed_index") != Some(feed)
        {
            return Err("feed acknowledgement conflicts".into());
        }
        return state(db, &parent.handle, ordinal).await;
    }
    let current = capture(db, &parent.handle, ordinal).await?;
    if current["can_feed"] != true
        || checkpoint != parent.checkpoint
        || feed < parent.feed_index
        || feed < old.get::<i64, _>("feed_index")
    {
        return Err("original receipt or VM feed checkpoint unavailable".into());
    }
    snapshot::publish(db, parent, snapshot, prepared).await?;
    sqlx::query(
        "UPDATE code_children SET fed_snapshot=?,fed_checkpoint=?,fed_feed_index=? WHERE handle=? AND ordinal=?",
    )
    .bind(snapshot)
    .bind(checkpoint + 1)
    .bind(feed)
    .bind(&parent.handle)
    .bind(ordinal)
    .execute(&mut *db)
    .await?;
    sqlx::query(
        "UPDATE code_parents SET snapshot=?,checkpoint=checkpoint+1,feed_index=? WHERE handle=?",
    )
    .bind(snapshot)
    .bind(feed)
    .bind(&parent.handle)
    .execute(&mut *db)
    .await?;
    snapshot::prune(db, &parent.handle).await?;
    state(db, &parent.handle, ordinal).await
}

pub(super) async fn summary(
    db: &mut SqliteConnection,
    handle: &str,
    pending_only: bool,
) -> Result<Vec<Value>> {
    let ordinals: Vec<i64> =
        sqlx::query_scalar("SELECT ordinal FROM code_children WHERE handle=? AND (NOT ? OR fed_snapshot IS NULL) ORDER BY ordinal LIMIT 16")
            .bind(handle)
            .bind(pending_only)
            .fetch_all(&mut *db)
            .await?;
    let mut rows = Vec::new();
    for ordinal in ordinals {
        let mut row = state(db, handle, ordinal).await?;
        row.as_object_mut()
            .ok_or("missing child metadata")?
            .remove("receipt");
        rows.push(row);
    }
    Ok(rows)
}

pub(super) async fn rebind(
    db: &mut SqliteConnection,
    handle: &str,
    generation: &str,
) -> Result<Value> {
    let row=sqlx::query("SELECT p.*,c.plugin,c.revision,c.generation,c.state AS call_state,i.revision AS installed,coalesce(x.original_actor,x.origin) AS actor FROM code_parents p JOIN host_calls c ON c.handle=p.handle JOIN host_installations i ON i.plugin=c.plugin JOIN executions x ON x.id=p.execution WHERE p.handle=?")
        .bind(handle).fetch_one(&mut *db).await?;
    super::domain::active(db, row.get("plugin"), generation, "composition.execute").await?;
    let exec: String = row.get("execution");
    effects::authorize(db, &exec, row.get("actor"), true).await?;
    let Some(snapshot) = row.get::<Option<String>, _>("snapshot") else {
        return Err(Box::new(super::Unavailable(
            "code_original_snapshot_unavailable",
        )));
    };
    if row.get::<bool, _>("closed")
        || row.get::<&str, _>("revision") != row.get::<&str, _>("installed")
        || !matches!(
            row.get::<&str, _>("call_state"),
            "unknown" | "not_dispatched"
        )
    {
        return Err("original Code call cannot resume".into());
    }
    let available:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM code_snapshots WHERE handle=? AND id=? AND complete AND length(bytes)=size)").bind(handle).bind(&snapshot).fetch_one(&mut *db).await?;
    if !available {
        return Err(Box::new(super::Unavailable(
            "code_original_snapshot_unavailable",
        )));
    }
    sqlx::query("UPDATE host_calls SET generation=?,state='queued' WHERE handle=?")
        .bind(generation)
        .bind(handle)
        .execute(&mut *db)
        .await?;
    // This is restoration of a checkpoint. The original parent remains dispatched/unknown until its receipt.
    sqlx::query("INSERT INTO code_resume_attempts VALUES(?,?,'queued') ON CONFLICT DO NOTHING")
        .bind(handle)
        .bind(generation)
        .execute(&mut *db)
        .await?;
    let parent = super::parent(db, row.get("plugin"), generation, handle, true).await?;
    let children = summary(db, handle, true).await?;
    Ok(
        json!({"handle":handle,"operation":parent.operation,"checkpoint":parent.checkpoint,"snapshot":snapshot,"contract":parent.runtime,"children":children}),
    )
}
