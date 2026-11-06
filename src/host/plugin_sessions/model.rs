use super::{Change, content, scope, writer};
use crate::{Result, host::aux_model, identity::new_id, privacy::source};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

async fn turn(db: &mut SqliteConnection, c: &Change) -> Result<sqlx::sqlite::SqliteRow> {
    let row = sqlx::query(
        "SELECT * FROM plugin_session_turns WHERE plugin=? AND session=? AND original_key=?",
    )
    .bind(&c.plugin)
    .bind(c.params["session"].as_str())
    .bind(writer::text(&c.params, "key")?)
    .fetch_one(db)
    .await?;
    if row.get::<&str, _>("digest") != writer::digest(c) {
        return Err("original isolated model turn conflicts".into());
    }
    Ok(row)
}
async fn status(
    db: &mut SqliteConnection,
    c: &Change,
    row: &sqlx::sqlite::SqliteRow,
) -> Result<Value> {
    let message: Option<(String, String)> = if let Some(sequence) =
        row.get::<Option<i64>, _>("assistant_sequence")
    {
        sqlx::query_as("SELECT body,digest FROM plugin_session_messages WHERE plugin=? AND session=? AND sequence=? AND body IS NOT NULL").bind(&c.plugin).bind(row.get::<&str,_>("session")).bind(sequence).fetch_optional(&mut *db).await?
    } else {
        None
    };
    let mut result = json!({"session":row.get::<String,_>("session"),"turn":row.get::<String,_>("id"),"epoch":row.get::<i64,_>("epoch"),"state":row.get::<String,_>("state"),"request":row.get::<Option<String>,_>("request"),"user_sequence":row.get::<i64,_>("user_sequence"),"assistant_sequence":row.get::<Option<i64>,_>("assistant_sequence")});
    if let Some((body, digest)) = message {
        result["text"] = json!(body.chars().take(512).collect::<String>());
        result["complete"] = json!(body.chars().count() <= 512);
        result["sha256"] = json!(digest);
        result["size"] = json!(body.len());
    }
    result["sources"] = content::sources_summary(db, &c.plugin, row.get("session")).await?;
    Ok(result)
}
pub(super) async fn prepare(
    db: &mut SqliteConnection,
    c: &Change,
    s: &sqlx::sqlite::SqliteRow,
) -> Result<Value> {
    content::active(s)?;
    if writer::original(db, c).await?.is_some() {
        return Err("original session key belongs to another mutation".into());
    }
    let id: &str = s.get("id");
    scope::retain(db, &c.plugin, id).await?;
    let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_session_turns WHERE plugin=? AND session=? AND original_key=?)").bind(&c.plugin).bind(id).bind(writer::text(&c.params,"key")?).fetch_one(&mut *db).await?;
    if exists {
        let original = turn(db, c).await?;
        if let Some(params) = original.get::<Option<&str>, _>("model_params")
            && original.get::<&str, _>("state") != "confirmed"
        {
            return Ok(json!({"model_params":serde_json::from_str::<Value>(params)?}));
        }
        return status(db, c, &original).await;
    }
    content::epoch(c, s)?;
    writer::idle(db, &c.plugin, id).await?;
    let authority = writer::fresh(db, c, s).await?;
    scope::bind(db, c, id, &authority).await?;
    let turn_id = new_id(db).await?;
    let text = writer::material(&c.params, "user_input")?;
    let sequence = writer::message(
        db,
        c,
        s,
        "user",
        authority.actor.as_deref(),
        text,
        Some(&turn_id),
    )
    .await?;
    let params = json!({"origin":{"session":turn_id},"call_id":"original-turn","route":s.get::<&str,_>("route"),"purpose":s.get::<&str,_>("purpose"),"text":text});
    sqlx::query("INSERT INTO plugin_session_turns(id,plugin,session,epoch,original_key,digest,authority,model_params,parent,event,entry,user_sequence,state) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,'prepared')")
        .bind(&turn_id).bind(&c.plugin).bind(id).bind(s.get::<i64,_>("epoch")).bind(writer::text(&c.params,"key")?).bind(writer::digest(c)).bind(c.params["authority"].to_string()).bind(params.to_string()).bind(&authority.parent).bind(authority.event).bind(&authority.entry).bind(sequence).execute(&mut *db).await?;
    let request = aux_model::apply(
        db,
        &aux_model::Change::Request {
            plugin: c.plugin.clone(),
            generation: c.generation.clone(),
            params: params.clone(),
        },
    )
    .await?;
    sqlx::query("UPDATE plugin_session_turns SET request=? WHERE id=?")
        .bind(request["request"].as_str())
        .bind(&turn_id)
        .execute(db)
        .await?;
    Ok(json!({"model_params":params}))
}
pub(super) async fn finish(
    db: &mut SqliteConnection,
    c: &Change,
    s: &sqlx::sqlite::SqliteRow,
) -> Result<Value> {
    content::active(s)?;
    scope::retain(db, &c.plugin, s.get("id")).await?;
    let row = turn(db, c).await?;
    if matches!(row.get::<&str, _>("state"), "confirmed" | "redacted") {
        return status(db, c, &row).await;
    }
    if row.get::<i64, _>("epoch") != s.get::<i64, _>("epoch") {
        return Err("isolated turn belongs to an earlier session epoch".into());
    }
    let request: &str = row.get("request");
    let state:Option<String>=sqlx::query_scalar("SELECT a.state FROM host_aux_requests q JOIN model_attempts a ON a.logical=q.logical WHERE q.id=? ORDER BY a.ordinal DESC LIMIT 1").bind(request).fetch_optional(&mut *db).await?;
    match state.as_deref() {
        Some("response" | "paired") => {
            let paid = aux_model::visible_receipt(db, &c.plugin, &c.generation, request).await?;
            let Some(body) = paid["result"].as_str() else {
                sqlx::query("UPDATE plugin_session_turns SET state='failed' WHERE id=?")
                    .bind(row.get::<&str, _>("id"))
                    .execute(&mut *db)
                    .await?;
                let row = turn(db, c).await?;
                return status(db, c, &row).await;
            };
            let sequence =
                writer::message(db, c, s, "assistant", None, body, Some(row.get("id"))).await?;
            sqlx::query(
                "UPDATE plugin_session_turns SET state='confirmed',assistant_sequence=? WHERE id=?",
            )
            .bind(sequence)
            .bind(row.get::<&str, _>("id"))
            .execute(&mut *db)
            .await?;
        }
        Some("unknown") => {
            sqlx::query("UPDATE plugin_session_turns SET state='unknown' WHERE id=?")
                .bind(row.get::<&str, _>("id"))
                .execute(&mut *db)
                .await?;
        }
        Some("rejected" | "response_held") => {
            sqlx::query("UPDATE plugin_session_turns SET state='failed' WHERE id=?")
                .bind(row.get::<&str, _>("id"))
                .execute(&mut *db)
                .await?;
        }
        _ => {}
    }
    let row = turn(db, c).await?;
    status(db, c, &row).await
}

pub(in crate::host) async fn resolve_origin(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    id: &str,
    route: &str,
) -> Result<aux_model::scope::Origin> {
    let row=sqlx::query("SELECT t.*,s.revision,s.route,s.purpose,s.context_profile,s.state AS session_state,s.epoch AS current_epoch FROM plugin_session_turns t JOIN plugin_sessions s ON s.plugin=t.plugin AND s.id=t.session WHERE t.id=? AND t.plugin=?")
        .bind(id).bind(plugin).fetch_one(&mut *db).await?;
    super::super::domain::active(db, plugin, generation, "session.run").await?;
    if row.get::<&str, _>("session_state") != "active"
        || row.get::<&str, _>("state") == "redacted"
        || row.get::<i64, _>("epoch") != row.get::<i64, _>("current_epoch")
        || row.get::<&str, _>("route") != route
    {
        return Err("original isolated session turn is unavailable".into());
    }
    let c = Change {
        plugin: plugin.into(),
        generation: generation.into(),
        method: "session.run".into(),
        params: json!({"session":row.get::<&str,_>("session"),"authority":serde_json::from_str::<Value>(row.get("authority"))?}),
        finish: false,
    };
    let (_, authority) = scope::current(db, &c, false).await?;
    scope::context(&authority, row.get("context_profile"))?;
    // Authorization can run on a reader connection. It never acquires new holds here.
    let revoked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_session_sources s JOIN events e ON e.id=s.source JOIN persons p ON p.id=e.raw_actor JOIN persons r ON r.id=coalesce(p.current_root,p.id) WHERE s.plugin=? AND s.session=? AND (NOT p.enabled OR NOT r.enabled OR EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)))")
        .bind(plugin).bind(row.get::<&str,_>("session")).fetch_one(db).await?;
    if revoked {
        return Err("isolated session source unavailable".into());
    }
    Ok(aux_model::scope::Origin {
        original: json!({"session":id}).to_string(),
        entry: row.get("entry"),
        parent: row.get("parent"),
        event: row.get("event"),
    })
}
pub(in crate::host) async fn model_input(
    db: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    id: &str,
    params: &Value,
) -> Result<Vec<Value>> {
    let row=sqlx::query("SELECT t.*,s.instructions,s.context_profile FROM plugin_session_turns t JOIN plugin_sessions s ON s.plugin=t.plugin AND s.id=t.session WHERE t.id=? AND t.plugin=?").bind(id).bind(plugin).fetch_one(&mut *db).await?;
    let original: Value = serde_json::from_str(row.get("model_params"))?;
    if original != *params {
        return Err("auxiliary input differs from original isolated turn".into());
    }
    let c = Change {
        plugin: plugin.into(),
        generation: generation.into(),
        method: "session.run".into(),
        params: json!({"session":row.get::<&str,_>("session"),"authority":serde_json::from_str::<Value>(row.get("authority"))?}),
        finish: false,
    };
    let (_, authority) = scope::current(db, &c, false).await?;
    let mut messages = vec![
        json!({"role":"system","content":format!("This is an isolated plugin session with no tools or main-chat access. Plugin instructions define its task.\n{}",row.get::<&str,_>("instructions"))}),
    ];
    if let Some(context) = scope::context(&authority, row.get("context_profile"))? {
        messages.push(context);
    }
    let history:Vec<(String,String)>=sqlx::query_as("SELECT role,body FROM (SELECT sequence,role,body FROM plugin_session_messages WHERE plugin=? AND session=? AND epoch=? AND sequence<=? AND body IS NOT NULL AND role IN('user','assistant') ORDER BY sequence DESC LIMIT 200) ORDER BY sequence")
        .bind(plugin).bind(row.get::<&str,_>("session")).bind(row.get::<i64,_>("epoch")).bind(row.get::<i64,_>("user_sequence")).fetch_all(db).await?;
    messages.extend(
        history
            .into_iter()
            .map(|(role, body)| json!({"role":role,"content":body})),
    );
    Ok(messages)
}
pub(in crate::host) async fn bind_execution(
    db: &mut SqliteConnection,
    plugin: &str,
    turn: &str,
    execution: &str,
) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO main_dependencies SELECT ?1,s.source FROM plugin_session_sources s JOIN plugin_session_turns t ON t.plugin=s.plugin AND t.session=s.session WHERE t.plugin=?2 AND t.id=?3")
        .bind(execution).bind(plugin).bind(turn).execute(db).await?;
    Ok(())
}
pub(super) async fn invalidate(
    db: &mut SqliteConnection,
    plugin: &str,
    id: &str,
    reason: &str,
    all: bool,
) -> Result<()> {
    let rows=sqlx::query("SELECT t.id,o.execution FROM plugin_session_turns t LEFT JOIN host_aux_requests q ON q.id=t.request LEFT JOIN host_aux_origins o ON o.plugin=q.plugin AND o.original=q.origin WHERE t.plugin=? AND t.session=? AND (? OR t.state!='confirmed')")
        .bind(plugin).bind(id).bind(all).fetch_all(&mut *db).await?;
    for row in rows {
        if let Some(exec) = row.get::<Option<&str>, _>("execution") {
            sqlx::query("INSERT OR IGNORE INTO plugin_session_redactions VALUES(?,?)")
                .bind(exec)
                .bind(reason)
                .execute(&mut *db)
                .await?;
            sqlx::query("UPDATE executions SET cancelled=1 WHERE id=?")
                .bind(exec)
                .execute(&mut *db)
                .await?;
            sqlx::query("UPDATE model_requests SET request_json='{}' WHERE activation IN(SELECT id FROM activations WHERE execution=?)").bind(exec).execute(&mut *db).await?;
            let attempts:Vec<(String,Option<String>)>=sqlx::query_as("SELECT a.id,a.usage_json FROM model_attempts a JOIN model_requests r ON r.id=a.logical JOIN activations v ON v.id=r.activation WHERE v.execution=?").bind(exec).fetch_all(&mut *db).await?;
            for (attempt, usage) in attempts {
                sqlx::query("UPDATE model_attempts SET response=NULL,partial_bytes=NULL,intents_json=NULL,retry_proof=0,usage_json=?,diagnostic=? WHERE id=?")
                    .bind(usage.as_deref().map(source::accounting_only)).bind(reason).bind(attempt).execute(&mut *db).await?;
            }
        }
        sqlx::query(
            "UPDATE plugin_session_turns SET state='redacted',model_params=NULL WHERE id=?",
        )
        .bind(row.get::<&str, _>("id"))
        .execute(&mut *db)
        .await?;
    }
    Ok(())
}
pub(crate) async fn scrub_inactive(db: &mut SqliteConnection) -> Result<()> {
    let sessions: Vec<(String, String, String)> =
        sqlx::query_as("SELECT plugin,id,state FROM plugin_sessions WHERE state!='active'")
            .fetch_all(&mut *db)
            .await?;
    for (plugin, id, state) in sessions {
        invalidate(
            db,
            &plugin,
            &id,
            &format!("session_{state}"),
            state != "closed",
        )
        .await?;
        if state != "closed" {
            sqlx::query(
                "UPDATE plugin_session_messages SET body=NULL WHERE plugin=? AND session=?",
            )
            .bind(&plugin)
            .bind(&id)
            .execute(&mut *db)
            .await?;
            sqlx::query(
                "UPDATE plugin_sessions SET name=NULL,instructions=NULL WHERE plugin=? AND id=?",
            )
            .bind(plugin)
            .bind(id)
            .execute(&mut *db)
            .await?;
        }
    }
    Ok(())
}

/// Recheck session policy at the shared ledger's dispatch/publication boundary.
/// Parent execution and route checks remain in the common effects authorizer.
pub(crate) async fn authorize(db: &mut SqliteConnection, execution: &str) -> Result<()> {
    let row=sqlx::query("SELECT t.*,s.state AS session_state,s.epoch AS current_epoch,s.revision,s.route,s.context_profile,i.revision AS installed,i.generation,p.body FROM host_aux_origins o JOIN plugin_session_turns t ON t.id=json_extract(o.original,'$.session') AND t.plugin=o.plugin JOIN plugin_sessions s ON s.plugin=t.plugin AND s.id=t.session JOIN host_installations i ON i.plugin=t.plugin LEFT JOIN host_session_policies p ON p.plugin=i.plugin AND p.revision=i.revision WHERE o.execution=?")
        .bind(execution).fetch_optional(&mut *db).await?;
    let Some(row) = row else { return Ok(()) };
    let body = row
        .get::<Option<&str>, _>("body")
        .ok_or("original session policy unavailable")?;
    let policy: super::Policy = serde_json::from_str(body)?;
    let generation = row
        .get::<Option<&str>, _>("generation")
        .ok_or("original session instance unavailable")?;
    for method in ["session.run", "llm.complete"] {
        crate::host::domain::active(db, row.get("plugin"), generation, method).await?;
    }
    let authority: Value = serde_json::from_str(row.get("authority"))?;
    if row.get::<&str, _>("revision") != row.get::<&str, _>("installed")
        || row.get::<&str, _>("session_state") != "active"
        || row.get::<&str, _>("state") == "redacted"
        || row.get::<i64, _>("epoch") != row.get::<i64, _>("current_epoch")
        || !policy.routes.contains(row.get::<&str, _>("route"))
        || !policy
            .context_profiles
            .contains(row.get::<&str, _>("context_profile"))
        || (authority.is_null() && !policy.background)
    {
        return Err("original session grant unavailable".into());
    }
    let revoked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_session_sources s JOIN events e ON e.id=s.source JOIN persons p ON p.id=e.raw_actor JOIN persons r ON r.id=coalesce(p.current_root,p.id) WHERE s.plugin=? AND s.session=? AND (NOT p.enabled OR NOT r.enabled OR EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)))")
        .bind(row.get::<&str,_>("plugin")).bind(row.get::<&str,_>("session")).fetch_one(&mut *db).await?;
    if revoked {
        return Err("original session source unavailable".into());
    }
    if let Some(event) = row.get::<Option<i64>, _>("event") {
        let live:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM events e JOIN assignments a ON a.id=e.actor_stage JOIN assignments b ON b.id=e.bot_stage JOIN entries d ON d.id=e.entry_id WHERE e.id=? AND a.status='active' AND b.status='active' AND d.closed_at IS NULL)").bind(event).fetch_one(db).await?;
        if !live {
            return Err("original session event authority ended".into());
        }
    }
    Ok(())
}
