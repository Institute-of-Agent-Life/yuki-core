use super::credentials::{self, Principal};
use crate::{Result, work::personal as domain};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

pub async fn action_event(
    conn: &mut SqliteConnection,
    who: &Principal,
    key: &str,
    kind: &str,
) -> Result<i64> {
    let entry = who.entry.as_deref().ok_or("chat_entry_required")?;
    let envelope = json!({"kind":"personal_action","action":kind}).to_string();
    Ok(sqlx::query_scalar("INSERT INTO events(source_scope,source_key,entry_id,account_id,raw_actor,actor_stage,bot_stage,observed_at,envelope,mentions) SELECT ?1,?2,e.id,a.account_id,a.original_person,a.id,e.bot_stage,CAST(unixepoch('subsec')*1000 AS INTEGER),?3,'[]' FROM entries e JOIN assignments a ON a.id=e.actor_stage WHERE e.id=?4 AND a.original_person=?5 RETURNING id").bind(format!("personal:{entry}")).bind(key).bind(envelope).bind(entry).bind(&who.request_person).fetch_one(conn).await?)
}
pub async fn apply(
    conn: &mut SqliteConnection,
    who: &Principal,
    id: &str,
    body: &Value,
    cancel: bool,
) -> Result<Value> {
    let manager = domain::owner(conn, &who.root, id).await?;
    let key = body["request_id"].as_str().ok_or("invalid_request_id")?;
    credentials::uuid(key)?;
    let kind = if cancel {
        "client.work_cancel"
    } else {
        "client.work_input"
    };
    let mut payload = if cancel {
        json!({"expected_revision":body["expected_revision"]})
    } else {
        json!({"text":body["text"].as_str().ok_or("validation_error")?.trim()})
    };
    if let Some(target) = body["agent_target"].as_object() {
        let fields: serde_json::Map<String, Value> = target
            .iter()
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        payload["agent_target"] = Value::Object(fields);
    }
    let digest = credentials::fingerprint(kind, id, &payload);
    if let Some(old) = super::receipts::before_mutation(conn, who, key).await? {
        if old.get::<&str, _>("digest") != digest {
            return Err("request_conflict".into());
        }
        let mut v: Value = serde_json::from_str(old.get("result"))?;
        if v.as_object().is_none_or(|m| m.is_empty()) {
            return Err("source_erased".into());
        }
        v["replay"] = json!(true);
        return Ok(v);
    }
    let event = action_event(conn, who, key, kind).await?;
    // Like the original Client9 message_key, private Work mail is named by the
    // selected external account and (for ACP) conversation, never the raw UUID alone.
    let external: String = sqlx::query_scalar(
        "SELECT a.native FROM events e JOIN accounts a ON a.id=e.account_id WHERE e.id=?",
    )
    .bind(event)
    .fetch_one(&mut *conn)
    .await?;
    let source = match &who.thread {
        Some(_) => format!(
            "client-input:{external}:{}:{key}",
            who.entry.as_deref().ok_or("chat_entry_required")?
        ),
        None => format!("client-input:{external}:{key}"),
    };
    let result = if cancel {
        domain::cancel(
            conn,
            id,
            &manager,
            &source,
            body["expected_revision"]
                .as_i64()
                .ok_or("validation_error")?,
        )
        .await?
    } else {
        let text = body["text"].as_str().ok_or("validation_error")?.trim();
        if text.is_empty() || text.len() > 1024 * 1024 {
            return Err("validation_error".into());
        }
        let target = &body["agent_target"];
        let content = if target.is_null() {
            text.to_string()
        } else {
            // Only the authenticated owner's registered device may receive a target.
            super::native::bind(conn, who, id, target).await?;
            json!({"text":text,"agent_target":target}).to_string()
        };
        domain::input(conn, id, &manager, &source, event, &content).await?
    };
    sqlx::query("INSERT INTO personal_receipts VALUES(?,?,?,?,?,?,?,?)")
        .bind(&who.request_person)
        .bind(who.request_scope())
        .bind(key)
        .bind(kind)
        .bind(id)
        .bind(digest)
        .bind(result.to_string())
        .bind(event)
        .execute(conn)
        .await?;
    Ok(result)
}
pub async fn receipt(conn: &mut SqliteConnection, who: &Principal, op: &str) -> Result<Value> {
    let row = super::receipts::find(conn, who, op).await?;
    let code = credentials::code_receipt(conn, who, op).await?;
    if row.is_some() && code.is_some() {
        return Err("request_conflict".into());
    }
    let Some(r) = row else {
        return Ok(code.unwrap_or(json!({"found":false})));
    };
    if r.get::<&str, _>("class") == "client.upload" {
        return super::uploads::project_receipt(conn, who, &r).await;
    }
    if let Some(id) = r.try_get::<Option<&str>, _>("target")? {
        match r.get::<&str, _>("class") {
            "client.work_cancel" | "client.work_input" => {
                domain::owner(conn, &who.root, id).await?;
            }
            "client.automation" => {
                sqlx::query_scalar::<_,i64>("SELECT s.id FROM schedules s JOIN persons p ON p.id=s.owner AND p.enabled JOIN schedule_versions v ON v.schedule=s.id AND v.version=s.version AND v.goal IS NOT NULL WHERE s.id=? AND coalesce(p.current_root,p.id)=?").bind(id).bind(&who.root).fetch_optional(&mut *conn).await?.ok_or("not_found")?;
            }
            "client.memory_correction" => {
                sqlx::query_scalar::<_,i64>("SELECT f.number FROM memory_facts f JOIN persons p ON p.id=f.person AND p.enabled WHERE f.number=? AND f.status!='erased' AND coalesce(p.current_root,p.id)=?").bind(id).bind(&who.root).fetch_optional(&mut *conn).await?.ok_or("not_found")?;
            }
            _ => return Err("not_found".into()),
        }
    }
    let result: Value = serde_json::from_str(r.try_get("result")?)?;
    if result.as_object().is_none_or(|m| m.is_empty()) {
        return Ok(json!({"found":false}));
    }
    let conversation: String =
        sqlx::query_scalar("SELECT entry_id FROM events WHERE id=? AND raw_actor=?")
            .bind(r.get::<i64, _>("event"))
            .bind(&who.request_person)
            .fetch_optional(conn)
            .await?
            .ok_or("not_found")?;
    Ok(
        json!({"found":true,"status":"accepted","operation_kind":r.get::<&str,_>("class"),"payload_hash":r.get::<&str,_>("digest"),"event_id":r.get::<i64,_>("event"),"result":result,"conversation_id":conversation,"target":r.get::<Option<String>,_>("target")}),
    )
}
pub async fn project(conn: &mut SqliteConnection, who: &Principal, id: &str) -> Result<Value> {
    domain::owner(conn, &who.root, id).await?;
    let r=sqlx::query("SELECT w.*,r.revision,e.scope,e.actor_stage,e.space,a.platform,coalesce(n.name,'') AS display_name,b.max_requests,b.reserved_requests,(SELECT count(*) FROM work_nodes c WHERE c.parent=w.id) AS children,coalesce((SELECT window FROM main_turns t WHERE t.execution=w.owner_execution),1) AS generation FROM work_nodes w JOIN personal_work_revisions r ON r.work=w.id JOIN entries e ON e.id=w.entry LEFT JOIN assignments s ON s.id=e.actor_stage LEFT JOIN accounts a ON a.id=s.account_id LEFT JOIN persons n ON n.id=w.principal JOIN root_budgets b ON b.root=w.budget WHERE w.id=?").bind(id).fetch_one(&mut *conn).await?;
    let status = match r.get::<&str, _>("status") {
        "need_input" => "waiting_user",
        "active" => "running",
        v => v,
    };
    let updates: i64 = r.get("updated");
    let (tools, tool_limit): (i64, Option<i64>) =
        sqlx::query_as("SELECT used_calls,max_calls FROM root_tool_budgets WHERE root=?")
            .bind(r.get::<&str, _>("budget"))
            .fetch_one(&mut *conn)
            .await?;
    let source = json!({"conversation_id":r.get::<&str,_>("entry"),"kind":if r.get::<&str,_>("scope")=="group"{"space"}else{"private"},"platform":r.get::<Option<String>,_>("platform"),"binding_id":r.get::<Option<String>,_>("actor_stage"),"space_id":r.get::<Option<String>,_>("space"),"display_name":r.get::<String,_>("display_name")});
    let deliveries=sqlx::query("SELECT o.entry,o.event,o.body FROM personal_live_deliveries o JOIN work_runs run ON run.execution=o.namespace WHERE run.work=? ORDER BY o.id").bind(id).fetch_all(&mut *conn).await?;
    let mut results = Vec::new();
    let question:Option<i64>=sqlx::query_scalar("SELECT o.event FROM work_user_questions q JOIN personal_live_deliveries o ON o.id=q.outbound WHERE q.work=?").bind(id).fetch_optional(&mut *conn).await?;
    for row in deliveries {
        let event = row.get::<i64, _>("event");
        let artifacts = super::artifacts::parts(conn, event).await?;
        results.push(json!({"conversation_id":row.get::<&str,_>("entry"),"event_id":event,"text":crate::history::outbound_text(row.get("body"))?,"artifacts":artifacts}));
    }
    let waiting = question
        .and_then(|event| results.iter().find(|item| item["event_id"] == event))
        .cloned();
    Ok(
        json!({"work_id":id,"conversation_id":r.get::<&str,_>("entry"),"generation":r.get::<i64,_>("generation"),"state":status,"revision":r.get::<i64,_>("revision"),"goal":r.get::<Option<String>,_>("goal"),"source":source,"result_delivery":source,"waiting_kind":if status=="waiting_user"{Some("user")}else{None},"waiting_question":waiting,"child_count":r.get::<i64,_>("children"),"budget":{"root_id":r.get::<&str,_>("budget"),"models":r.get::<i64,_>("reserved_requests"),"model_limit":r.get::<i64,_>("max_requests"),"tools":tools,"tool_limit":tool_limit},"results":results,"updated_at":updates as f64/1000.}),
    )
}
pub async fn message(conn: &mut SqliteConnection, who: &Principal, body: &Value) -> Result<Value> {
    use crate::ingress::admit;
    let entry = who.entry.as_deref().ok_or("chat_entry_required")?;
    let key = body["request_id"].as_str().ok_or("invalid_request_id")?;
    credentials::uuid(key)?;
    let text = body["text"].as_str().ok_or("validation_error")?.trim();
    let ids: Vec<String> = body
        .get("attachments")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()?
        .unwrap_or_default();
    if (text.is_empty() && ids.is_empty()) || text.len() > 1024 * 1024 {
        return Err("validation_error".into());
    }
    if body["agent_target"].is_object() {
        if super::receipts::before_mutation(conn, who, key)
            .await?
            .is_some()
        {
            return Err("request_conflict".into());
        }
        return Err("selected_input_unavailable".into());
    }
    let fp = credentials::fingerprint(
        "client.message",
        entry,
        &json!({"text":text,"attachments":ids}),
    );
    if let Some(row) = super::receipts::before_mutation(conn, who, key).await? {
        let s: &str = row.get("digest");
        let result: &str = row.get("result");
        if s != fp {
            return Err("request_conflict".into());
        }
        let mut result: Value = serde_json::from_str(result)?;
        if result["event_id"].is_null() {
            return Err("source_erased".into());
        }
        result["replay"] = json!(true);
        return Ok(result);
    }
    let row=sqlx::query("SELECT a.account_id,e.bot_stage FROM entries e JOIN assignments a ON a.id=e.actor_stage WHERE e.id=?").bind(entry).fetch_one(&mut *conn).await?;
    let parts = super::uploads::parts(conn, who, &ids).await?;
    let mut segments = vec![admit::Segment::Text { text: text.into() }];
    let mut rendered = text.to_owned();
    for (i, part) in parts.iter().enumerate() {
        segments.push(admit::Segment::Media {
            kind: part.kind.clone(),
            reference: part.id.clone(),
        });
        rendered.push_str(&format!(" [{}附件{}：{}]", part.kind, i, part.name));
    }
    let at = credentials::now()? * 1000;
    let frame = admit::Frame {
        source: format!("personal:{entry}:{key}"),
        native_id: None,
        account: row.get("account_id"),
        bot_stage: row.get("bot_stage"),
        space: None,
        topic: who.thread.clone(),
        occurred: Some(at),
        observed: at,
        kind: admit::Kind::Message,
        segments,
        sender: None,
        reply_bot: None,
        answer_work_id: None,
        text: (!parts.is_empty()).then_some(rendered),
    };
    let admitted = admit::from_gateway(conn, frame, true).await?;
    let event = admitted["event"].as_i64().ok_or("message_unavailable")?;
    super::uploads::attach(conn, event, &parts).await?;
    let original: (String, String) =
        sqlx::query_as("SELECT entry_id,raw_actor FROM events WHERE id=?")
            .bind(event)
            .fetch_one(&mut *conn)
            .await?;
    if original.0 != entry || original.1 != who.request_person {
        return Err("precondition_failed".into());
    }
    let result = json!({"event_id":event,"conversation_id":entry,"replay":false});
    sqlx::query("INSERT INTO personal_receipts VALUES(?,?,?,'client.message',NULL,?,?,?)")
        .bind(&who.request_person)
        .bind(who.request_scope())
        .bind(key)
        .bind(fp)
        .bind(result.to_string())
        .bind(event)
        .execute(conn)
        .await?;
    Ok(result)
}
