use super::{
    Change,
    evidence::Source,
    facts::{self, Edit, Subject},
    scope::Reader,
};
use crate::{
    Result,
    gateway::qq,
    ingress::admit,
    runtime::effects,
    store::{Mutation, Store},
};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::time::Duration;
fn words(text: &str) -> (&str, &str) {
    text.trim()
        .split_once(char::is_whitespace)
        .map(|(a, b)| (a, b.trim()))
        .unwrap_or((text.trim(), ""))
}
async fn command(
    conn: &mut SqliteConnection,
    event: i64,
    exec: &str,
    actor: &str,
    raw: &str,
    admin: bool,
) -> Result<Value> {
    let (op, args) = words(raw);
    let reader = Reader::at(conn, exec, actor, admin, true).await?;
    if matches!(op, "" | "list" | "show" | "evidence") {
        let id = if matches!(op, "show" | "evidence") {
            if args.is_empty() {
                return Err("需要原事實 ID".into());
            }
            Some(args)
        } else {
            None
        };
        return facts::read(conn, &reader, id, "", 10, op == "evidence", true).await;
    }
    let edit = match op {
        "add" => {
            if args.is_empty() {
                return Err("需要事實內容".into());
            }
            Edit::Add {
                metadata: None,
                subject: Subject {
                    kind: "person".into(),
                    person: Some(reader.base.subject.clone()),
                    space: None,
                    visibility: None,
                    entry: None,
                },
                fact_key: "explicit".into(),
                body: args.into(),
                confidence: None,
                evidence: vec![Source::Event {
                    id: event,
                    quote: args.into(),
                }],
            }
        }
        "update" | "correct" | "delete" | "invalidate" | "restore" => {
            let (id, body) = words(args);
            let read = facts::read(conn, &reader, Some(id), "", 1, false, true).await?;
            let row = &read["items"][0];
            let revision = row["revision"].as_i64().ok_or("missing fact revision")?;
            let proof = row["proof"]
                .as_str()
                .ok_or("missing evidence binding")?
                .into();
            if matches!(op, "delete" | "invalidate") {
                Edit::Retract {
                    fact: id.into(),
                    revision,
                    proof,
                }
            } else if op == "restore" {
                Edit::Restore {
                    fact: id.into(),
                    revision,
                    proof,
                    evidence: vec![Source::Event {
                        id: event,
                        quote: raw.into(),
                    }],
                }
            } else {
                Edit::Correct {
                    metadata: None,
                    fact: id.into(),
                    revision,
                    proof,
                    body: body.into(),
                    confidence: None,
                    evidence: vec![Source::Event {
                        id: event,
                        quote: body.into(),
                    }],
                }
            }
        }
        _ => {
            return Err(
                "支援 list、show、evidence、add、correct、update、delete、invalidate、restore"
                    .into(),
            );
        }
    };
    facts::edit(
        conn,
        &reader,
        exec,
        actor,
        &format!("memory-command:{event}"),
        &edit,
        true,
    )
    .await
}
pub async fn route(conn: &mut SqliteConnection, event: i64, frame: &admit::Frame) -> Result<bool> {
    let text = frame.text.clone().unwrap_or_else(|| {
        frame
            .segments
            .iter()
            .filter_map(|s| {
                if let admit::Segment::Text { text } = s {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect()
    });
    let (ai, rest) = words(&text);
    if !ai.eq_ignore_ascii_case("/ai") {
        return Ok(false);
    }
    let (first, rest) = words(rest);
    let (name, raw, admin) = if first.eq_ignore_ascii_case("admin") {
        let (n, r) = words(rest);
        (n, r, true)
    } else {
        (first, rest, false)
    };
    if !name.eq_ignore_ascii_case("memory") {
        return Ok(false);
    }
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM memory_command_events WHERE event=?)",
    )
    .bind(event)
    .fetch_one(&mut *conn)
    .await?
    {
        return Ok(true);
    }
    let claim = admit::claim(conn, event).await?;
    let exec = claim["execution"]
        .as_str()
        .ok_or("missing memory source execution")?;
    let (actor, entry): (String, String) = sqlx::query_as(
        "SELECT e.raw_actor,d.scope FROM events e JOIN entries d ON d.id=e.entry_id WHERE e.id=?",
    )
    .bind(event)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query("SAVEPOINT memory_command")
        .execute(&mut *conn)
        .await?;
    let result = command(conn, event, exec, &actor, raw, admin).await;
    let text = match result {
        Ok(_) if entry == "group" => "記憶操作已處理，請在私聊查看本人可讀的內容。".to_owned(),
        Ok(v) => {
            // A private control result inherits the selected facts' sources before its
            // durable send is created. Erasure between this read and dispatch closes the fence.
            for fact in v["items"].as_array().into_iter().flatten() {
                if let Some(id) = fact["fact"].as_str() {
                    super::tools::bind_sources(conn, exec, id).await?;
                }
            }
            v.to_string()
        }
        Err(_) => {
            sqlx::query("ROLLBACK TO memory_command")
                .execute(&mut *conn)
                .await?;
            "此操作無法使用，請核對原 ID、參數及目前權限。".into()
        }
    };
    sqlx::query("RELEASE memory_command")
        .execute(&mut *conn)
        .await?;
    effects::apply(
        conn,
        &effects::Action::Intent {
            execution: exec.into(),
            actor: actor.clone(),
            id: format!("memory-reply:{event}"),
            action: effects::Operation::External,
            target: "send".into(),
            payload: json!({"text":text}).to_string(),
            parent: None,
            ordinal: None,
        },
    )
    .await?;
    sqlx::query("INSERT INTO memory_command_events(event,execution) VALUES(?,?)")
        .bind(event)
        .bind(exec)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE executions SET state='completed' WHERE id=?")
        .bind(exec)
        .execute(conn)
        .await?;
    Ok(true)
}
pub async fn deliver(store: Store, qq: qq::Hub) {
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    loop {
        tick.tick().await;
        let rows=sqlx::query("SELECT c.event,c.execution,e.raw_actor FROM memory_command_events c JOIN events e ON e.id=c.event WHERE c.reply_state IS NULL AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id OR i.source IN(SELECT event FROM main_dependencies WHERE execution=c.execution)) ORDER BY c.event LIMIT 32").fetch_all(store.readers()).await;
        let Ok(rows) = rows else {
            continue;
        };
        for row in rows {
            let (event, exec, actor): (i64, String, String) = (row.get(0), row.get(1), row.get(2));
            let id = format!("memory-reply:{event}");
            let delivery = async {
                if let Some(host) = &store.host
                    && let Some(r) = host.deliver(&store, &exec, &actor, &id).await?
                {
                    return Ok(r);
                }
                crate::social::deliver(&store, &qq, &exec, &id, &actor).await
            }
            .await;
            if let Ok(r) = delivery
                && let Some(state @ ("unknown" | "failed" | "confirmed")) = r["state"].as_str()
            {
                let _ = store
                    .write(Mutation::Memory(Box::new(Change::Reported(
                        event,
                        state.into(),
                    ))))
                    .await;
            }
        }
    }
}
