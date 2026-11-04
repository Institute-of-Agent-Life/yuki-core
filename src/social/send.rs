use super::content;
use crate::{Result, runtime::effects};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub struct Route {
    pub entry: String,
    pub person: String,
    pub stage: String,
    pub target: i64,
    pub private_stage: Option<String>,
    pub group: bool,
    pub window: i64,
    pub revision: Option<i64>,
}
pub async fn route(conn: &mut SqliteConnection, exec: &str, actor: &str) -> Result<Route> {
    effects::authorize(conn, exec, actor, true).await?;
    let r=sqlx::query("WITH RECURSIVE owners(id) AS(SELECT ? UNION SELECT o.parent FROM owners f JOIN executions x ON x.id=f.id JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL)
        SELECT e.id,e.entry_id,e.bot_stage,e.actor_stage,e.envelope,e.raw_actor,entry.scope,entry.space,ac.native AS human,bot.namespace,bot.native AS receiver,
        coalesce(t.window,w.generation,1) AS generation,coalesce(w.generation,1) AS current_window
        FROM owners f JOIN executions x ON x.id=f.id JOIN events e ON e.id=x.source JOIN entries entry ON entry.id=e.entry_id AND entry.closed_at IS NULL
        JOIN accounts ac ON ac.id=e.account_id JOIN assignments bs ON bs.id=e.bot_stage JOIN accounts bot ON bot.id=bs.account_id AND bot.platform='qq'
        LEFT JOIN main_turns t ON t.execution=? LEFT JOIN main_windows w ON w.conversation=entry.id")
        .bind(exec).bind(exec).fetch_one(&mut *conn).await?;
    let window: i64 = r.try_get("generation")?;
    if window != r.try_get::<i64, _>("current_window")? {
        return Err("original conversation generation changed".into());
    }
    let group = r.try_get::<String, _>("scope")? == "group";
    let envelope: Value = serde_json::from_str(r.try_get("envelope")?)?;
    let destination = envelope["reply_bot"]
        .as_str()
        .ok_or("no unique original receiver")?;
    let stage:String=sqlx::query_scalar("SELECT s.id FROM assignments s JOIN accounts a ON a.id=s.account_id WHERE a.platform='qq' AND a.namespace=? AND a.native=? AND s.status='active' AND s.kind='self'")
        .bind(r.try_get::<String,_>("namespace")?).bind(destination).fetch_one(&mut *conn).await?;
    if !group && stage != r.try_get::<String, _>("bot_stage")? {
        return Err("private receiver changed".into());
    }
    let (target, revision) = if group {
        let space: Vec<String> = serde_json::from_str(r.try_get("space")?)?;
        if space.len() != 3
            || space[0] != "qq"
            || space[1] != r.try_get::<String, _>("namespace")?
        {
            return Err("invalid original QQ space".into());
        }
        let route = crate::ingress::group_recovery::route(conn, &space[1], &space[2]).await?;
        if route
            .as_ref()
            .is_some_and(|(pin, _, enabled)| !enabled || pin != &stage)
        {
            return Err("group route changed".into());
        }
        (space[2].parse()?, route.map(|(_, rev, _)| rev))
    } else {
        (r.try_get::<String, _>("human")?.parse()?, None)
    };
    Ok(Route {
        entry: r.try_get("entry_id")?,
        person: r.try_get("raw_actor")?,
        stage,
        target,
        private_stage: if group {
            None
        } else {
            r.try_get("actor_stage")?
        },
        group,
        window,
        revision,
    })
}

async fn prepare(
    conn: &mut SqliteConnection,
    exec: &str,
    id: &str,
    actor: &str,
    r: &Route,
    limits: super::split::Limits,
) -> Result<()> {
    let op = sqlx::query(
        "SELECT action,target,payload,state FROM operations WHERE namespace=? AND operation_id=?",
    )
    .bind(exec)
    .bind(id)
    .fetch_one(&mut *conn)
    .await?;
    if op.try_get::<String, _>("action")? != "external"
        || op.try_get::<String, _>("target")? != "send"
    {
        return Err("original operation is not a send".into());
    }
    if op.try_get::<String, _>("state")? != "intent" {
        return Err("original operation already has a dispatch owner".into());
    }
    let mut msg: content::Message = serde_json::from_str(op.try_get("payload")?)?;
    msg.text = content::outbound_text(&msg.text);
    if let Some(target) = &msg.target
        && ((r.group && (target.kind != "space" || target.id != r.entry))
            || (!r.group && (target.kind != "person" || target.id != r.person)))
    {
        return Err("target differs from the authorized original conversation".into());
    }
    let mut parts = content::plan(conn, exec, r, &msg, limits).await?;
    if let Some(source) = msg.reply_to_event_id {
        let locator:String=sqlx::query_scalar("SELECT l.native_id FROM event_locators l JOIN events e ON e.id=l.event_id WHERE e.id=? AND l.receiver=? AND e.entry_id=?
          AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)").bind(source).bind(&r.stage).bind(&r.entry).fetch_one(&mut *conn).await?;
        let first = parts
            .first_mut()
            .and_then(|p| p.params["message"].as_array_mut())
            .ok_or("reply needs message content")?;
        first.insert(0, json!({"type":"reply","data":{"id":locator}}));
    }
    sqlx::query("INSERT INTO social_plans(namespace,operation_id,actor,entry,stage,recipient_stage,native_target,scope,window,route_revision) VALUES(?,?,?,?,?,?,?,?,?,?)")
        .bind(exec).bind(id).bind(actor).bind(&r.entry).bind(&r.stage).bind(&r.private_stage).bind(r.target.to_string())
        .bind(if r.group{"group"}else{"private"}).bind(r.window).bind(r.revision).execute(&mut *conn).await?;
    for (i, p) in parts.iter().enumerate() {
        let child = crate::identity::new_id(conn).await?;
        sqlx::query("INSERT INTO operations(namespace,operation_id,action,target,payload,parent,ordinal,state) VALUES(?,?,'external',?,?,?,?,'intent')")
            .bind(exec).bind(&child).bind(&p.action).bind(p.params.to_string()).bind(id).bind(i as i64).execute(&mut *conn).await?;
        sqlx::query("INSERT INTO social_parts(namespace,operation_id,ordinal,child,wire_action,params,state) VALUES(?,?,?,?,?,?,'prepared')")
            .bind(exec).bind(id).bind(i as i64).bind(child).bind(&p.action).bind(p.params.to_string()).execute(&mut *conn).await?;
    }
    let selected:Option<(String,String)>=sqlx::query_as("SELECT s.owner,s.digest FROM social_image_selection s JOIN source_owners o ON o.id=s.owner AND o.enabled WHERE s.singleton=1").fetch_optional(&mut *conn).await?;
    if msg.emoji.is_some() && selected.is_none() && parts.is_empty() {
        return Err("no selected image owner".into());
    }
    if let (Some(img), Some((owner, digest))) = (msg.emoji, selected) {
        let child = crate::identity::new_id(conn).await?;
        sqlx::query("INSERT INTO operations(namespace,operation_id,action,target,payload,parent,ordinal,state) VALUES(?,?,'external','selected_image',?,?,?,'intent')")
            .bind(exec).bind(&child).bind(serde_json::to_string(&img)?).bind(id).bind(parts.len() as i64).execute(&mut *conn).await?;
        sqlx::query("INSERT INTO social_images(namespace,operation_id,child,goal,emotion,actor,stage,entry,window,owner,digest) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
            .bind(exec).bind(id).bind(child).bind(img.goal).bind(img.emotion).bind(actor).bind(&r.stage).bind(&r.entry).bind(r.window).bind(&owner).bind(&digest).execute(&mut *conn).await?;
    }
    sqlx::query("UPDATE operations SET state='dispatched' WHERE namespace=? AND operation_id=? AND state='intent'").bind(exec).bind(id).execute(conn).await?;
    Ok(())
}

pub async fn inspect(
    conn: &mut SqliteConnection,
    exec: &str,
    id: &str,
    actor: &str,
) -> Result<Value> {
    effects::authorize(conn, exec, actor, false).await?;
    let plan=sqlx::query("SELECT p.entry,p.stage,p.window,o.state FROM social_plans p JOIN operations o ON o.namespace=p.namespace AND o.operation_id=p.operation_id WHERE p.namespace=? AND p.operation_id=?")
        .bind(exec).bind(id).fetch_one(&mut *conn).await?;
    let rows=sqlx::query("SELECT ordinal,child,state,locator,error FROM social_parts WHERE namespace=? AND operation_id=? ORDER BY ordinal")
        .bind(exec).bind(id).fetch_all(&mut *conn).await?;
    let parts=rows.iter().map(|r|Ok(json!({"ordinal":r.try_get::<i64,_>("ordinal")?,"child":r.try_get::<String,_>("child")?,"state":r.try_get::<String,_>("state")?,"locator":r.try_get::<Option<String>,_>("locator")?,"error":r.try_get::<Option<String>,_>("error")?}))).collect::<Result<Vec<_>>>()?;
    let image=sqlx::query("SELECT child,goal,actor,stage,entry,window,state,owner,digest FROM social_images WHERE namespace=? AND operation_id=?")
        .bind(exec).bind(id).fetch_optional(conn).await?.map(|row|->Result<Value>{Ok(json!({"child":row.try_get::<String,_>("child")?,"goal":row.try_get::<String,_>("goal")?,
            "actor":row.try_get::<String,_>("actor")?,"stage":row.try_get::<String,_>("stage")?,"entry":row.try_get::<String,_>("entry")?,"window":row.try_get::<i64,_>("window")?,"owner":row.try_get::<String,_>("owner")?,"digest":row.try_get::<String,_>("digest")?,"state":row.try_get::<String,_>("state")?}))}).transpose()?;
    Ok(
        json!({"operation":id,"execution":exec,"entry":plan.try_get::<String,_>("entry")?,"stage":plan.try_get::<String,_>("stage")?,"window":plan.try_get::<i64,_>("window")?,"state":plan.try_get::<String,_>("state")?,"parts":parts,"image":image}),
    )
}

pub async fn next(
    conn: &mut SqliteConnection,
    exec: &str,
    id: &str,
    actor: &str,
    session: Option<u64>,
    limits: super::split::Limits,
) -> Result<Value> {
    let frozen=sqlx::query("SELECT stage,recipient_stage,native_target,entry,window,route_revision FROM social_plans WHERE namespace=? AND operation_id=?")
        .bind(exec).bind(id).fetch_optional(&mut *conn).await?;
    if frozen.is_some() {
        let receipt = inspect(conn, exec, id, actor).await?;
        if receipt["state"] != "dispatched"
            || receipt["parts"]
                .as_array()
                .is_some_and(|p| p.iter().any(|x| x["state"] == "dispatched"))
        {
            return Ok(json!({"receipt":receipt}));
        }
    }
    let route = route(conn, exec, actor).await?;
    if let Some(row) = frozen {
        if row.try_get::<String, _>("stage")? != route.stage
            || row.try_get::<String, _>("entry")? != route.entry
            || row.try_get::<Option<String>, _>("recipient_stage")? != route.private_stage
            || row.try_get::<i64, _>("window")? != route.window
            || row.try_get::<Option<i64>, _>("route_revision")? != route.revision
            || row.try_get::<String, _>("native_target")? != route.target.to_string()
        {
            return Err("original frozen social route changed".into());
        }
    } else {
        prepare(conn, exec, id, actor, &route, limits).await?;
    }
    let part=sqlx::query("SELECT ordinal,child,wire_action,params FROM social_parts WHERE namespace=? AND operation_id=? AND state='prepared' ORDER BY ordinal LIMIT 1")
        .bind(exec).bind(id).fetch_optional(&mut *conn).await?;
    if let Some(row) = part {
        let ordinal: i64 = row.try_get("ordinal")?;
        let child: String = row.try_get("child")?;
        sqlx::query("UPDATE social_parts SET state='dispatched',session=? WHERE namespace=? AND operation_id=? AND ordinal=?")
            .bind(session.map(|n|n as i64)).bind(exec).bind(id).bind(ordinal).execute(&mut *conn).await?;
        sqlx::query("UPDATE operations SET state='dispatched' WHERE namespace=? AND operation_id=? AND state='intent'").bind(exec).bind(&child).execute(&mut *conn).await?;
        Ok(
            json!({"dispatch":true,"ordinal":ordinal,"child":child,"stage":route.stage,"action":row.try_get::<String,_>("wire_action")?,"params":serde_json::from_str::<Value>(row.try_get("params")?)?}),
        )
    } else {
        aggregate(conn, exec, id).await?;
        Ok(json!({"receipt":inspect(conn,exec,id,actor).await?}))
    }
}

pub async fn aggregate(conn: &mut SqliteConnection, exec: &str, id: &str) -> Result<()> {
    let states: Vec<String> = sqlx::query_scalar(
        "SELECT state FROM social_parts WHERE namespace=? AND operation_id=? ORDER BY ordinal",
    )
    .bind(exec)
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let state = if states.iter().any(|s| s == "unknown") {
        "unknown"
    } else if states.iter().any(|s| s == "failed") {
        "failed"
    } else if states.iter().all(|s| s == "confirmed") {
        "confirmed"
    } else {
        // A later positive ACK resolves only its own original part. Remaining ordinals
        // can be claimed by a fresh authorized caller; no network call occurs here.
        sqlx::query("UPDATE operations SET state='dispatched',result=NULL WHERE namespace=? AND operation_id=? AND state='unknown'")
            .bind(exec).bind(id).execute(conn).await?;
        return Ok(());
    };
    let image: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM social_images WHERE namespace=? AND operation_id=?)",
    )
    .bind(exec)
    .bind(id)
    .fetch_one(&mut *conn)
    .await?;
    let result = json!({"parts":states,"public_delivery_confirmed":!states.is_empty() && states.iter().all(|s|s=="confirmed"),"image_intent_accepted":image,"image_delivered":false});
    sqlx::query("UPDATE operations SET state=?,result=? WHERE namespace=? AND operation_id=?")
        .bind(state)
        .bind(result.to_string())
        .bind(exec)
        .bind(id)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn settle(
    conn: &mut SqliteConnection,
    exec: &str,
    id: &str,
    ordinal: i64,
    reply: Option<&Value>,
) -> Result<Value> {
    let row=sqlx::query("SELECT part.child,part.params,part.wire_action,part.state,part.locator,p.stage,p.entry,p.actor,x.source FROM social_parts part JOIN social_plans p USING(namespace,operation_id)
        JOIN executions x ON x.id=p.namespace WHERE p.namespace=? AND p.operation_id=? AND part.ordinal=?")
        .bind(exec).bind(id).bind(ordinal).fetch_one(&mut *conn).await?;
    let prior: String = row.try_get("state")?;
    let reported_locator =
        reply.and_then(|v| crate::gateway::qq::native_id(&v["data"]["message_id"]));
    let file = row
        .try_get::<String, _>("wire_action")?
        .starts_with("upload_");
    let state = match reply {
        Some(v)
            if v["status"] == "ok" && v["retcode"] == 0 && (file || reported_locator.is_some()) =>
        {
            "confirmed"
        }
        Some(v) if v["status"] == "failed" && v["retcode"].as_i64().is_some_and(|n| n != 0) => {
            "failed"
        }
        _ => "unknown",
    };
    let collision:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM social_outbound WHERE stage=? AND entry=? AND locator=? AND (namespace!=? OR child!=?))")
        .bind(row.try_get::<String,_>("stage")?).bind(row.try_get::<String,_>("entry")?).bind(&reported_locator)
        .bind(exec).bind(row.try_get::<String,_>("child")?).fetch_one(&mut *conn).await?;
    let state = if collision { "unknown" } else { state };
    let locator = if state == "confirmed" {
        reported_locator.clone()
    } else {
        None
    };
    if !matches!(prior.as_str(), "dispatched" | "unknown") {
        if state == "unknown"
            || (prior == state && row.try_get::<Option<String>, _>("locator")? == locator)
        {
            return Ok(json!({"status":prior,"ordinal":ordinal}));
        }
        return Err("physical receipt conflicts with original terminal result".into());
    }
    let child: String = row.try_get("child")?;
    let stage: String = row.try_get("stage")?;
    let entry: String = row.try_get("entry")?;
    let erased =
        crate::privacy::source::execution_erased(conn, exec, row.try_get("source")?).await?;
    let echoes:Vec<i64>=sqlx::query_scalar("SELECT e.id FROM event_locators l JOIN events e ON e.id=l.event_id
      WHERE l.receiver=? AND l.native_id=? AND e.entry_id=? AND json_extract(e.envelope,'$.kind')='echo' ORDER BY e.id LIMIT 2")
      .bind(&stage).bind(&locator).bind(&entry).fetch_all(&mut *conn).await?;
    let echo = match echoes.as_slice() {
        [one] => Some(*one),
        _ => None,
    };
    if state == "confirmed" {
        sqlx::query("INSERT INTO social_outbound(stage,entry,locator,namespace,child,original_actor,body,echo_event) VALUES(?,?,?,?,?,?,?,?)")
            .bind(&stage).bind(&entry).bind(&locator).bind(exec).bind(&child).bind(row.try_get::<String,_>("actor")?)
            .bind(if erased{"{}"}else{row.try_get("params")?}).bind(echo).execute(&mut *conn).await?;
    }
    if erased && let Some(echo) = echo {
        sqlx::query("UPDATE events SET envelope='{}',mentions='[]' WHERE id=?")
            .bind(echo)
            .execute(&mut *conn)
            .await?;
    }
    sqlx::query("UPDATE social_parts SET state=?,locator=?,confirmed_event=?,error=? WHERE namespace=? AND operation_id=? AND ordinal=?")
        .bind(state).bind(&locator).bind(echo).bind(collision.then_some("locator_collision")).bind(exec).bind(id).bind(ordinal).execute(&mut *conn).await?;
    let receipt = json!({"status":state,"locator":locator,"ordinal":ordinal});
    sqlx::query("UPDATE operations SET state=?,result=? WHERE namespace=? AND operation_id=?")
        .bind(state)
        .bind(receipt.to_string())
        .bind(exec)
        .bind(&child)
        .execute(&mut *conn)
        .await?;
    sqlx::query("INSERT INTO social_part_receipts(namespace,child,state,locator) VALUES(?,?,?,?) ON CONFLICT(namespace,child,state) DO NOTHING").bind(exec).bind(&child).bind(state).bind(&reported_locator).execute(&mut *conn).await?;
    aggregate(conn, exec, id).await?;
    Ok(receipt)
}

pub async fn recover(conn: &mut SqliteConnection) -> Result<()> {
    let mut tx = sqlx::Connection::begin(conn).await?;
    let outstanding: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT namespace,operation_id,ordinal FROM social_parts WHERE state='dispatched'",
    )
    .fetch_all(&mut *tx)
    .await?;
    for (exec, id, ordinal) in outstanding {
        settle(&mut tx, &exec, &id, ordinal, None).await?;
    }
    sqlx::query("UPDATE social_recalls SET state='unknown' WHERE state='dispatched'")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn erase(conn: &mut SqliteConnection, event: i64) -> Result<()> {
    sqlx::query("DELETE FROM qq_social_names WHERE source=?")
        .bind(event)
        .execute(&mut *conn)
        .await?;
    let affected:Vec<String>=sqlx::query_scalar("SELECT id FROM executions WHERE source=?1 OR id IN(SELECT execution FROM main_dependencies WHERE event=?1)").bind(event).fetch_all(&mut *conn).await?;
    for exec in affected {
        sqlx::query("UPDATE social_parts SET params='{}' WHERE namespace=?")
            .bind(&exec)
            .execute(&mut *conn)
            .await?;
        sqlx::query("UPDATE events SET envelope='{}',mentions='[]' WHERE id IN(SELECT echo_event FROM social_outbound WHERE namespace=?)").bind(&exec).execute(&mut *conn).await?;
        sqlx::query("UPDATE social_outbound SET body='{}' WHERE namespace=?")
            .bind(&exec)
            .execute(&mut *conn)
            .await?;
        sqlx::query("UPDATE social_images SET goal='',emotion='',state='erased' WHERE namespace=?")
            .bind(&exec)
            .execute(&mut *conn)
            .await?;
        sqlx::query("DELETE FROM social_media_bindings WHERE execution=?")
            .bind(exec)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}
