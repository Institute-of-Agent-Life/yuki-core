use super::{
    Change,
    evidence::Source,
    facts::{self, Edit, Subject},
    scope::Reader,
};
use crate::{
    Result,
    runtime::effects,
    store::{Mutation, Store},
    tools::registry::Tool,
};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};
use std::sync::LazyLock;

static DECLARATIONS: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("v4-declarations.json")).expect("v4 Memory tool declarations")
});
pub fn declaration(name: &str) -> Option<Tool> {
    let item = DECLARATIONS["tools"]
        .as_array()?
        .iter()
        .find(|v| v["name"] == name)?;
    let mut schema = item["parameters"].clone();
    let desc = if name == "memory_change" {
        let props = schema["properties"].as_object_mut()?;
        props.retain(|key, _| {
            [
                "operation",
                "fact_id",
                "selector",
                "merge_fact_id",
                "merge_selector",
                "reason",
                "target",
                "new_content",
                "memory_key",
                "confidence",
                "evidence_event_id",
                "evidence_quote",
                "evidence_refs",
                "expected_fact_state",
                "request_basis",
                "visibility",
                "category",
                "kind",
                "importance",
                "valid_from",
                "valid_until",
            ]
            .contains(&key.as_str())
        });
        props.get_mut("expected_fact_state")?["enum"] = json!(["active", "invalidated"]);
        props.get_mut("request_basis")?["enum"] = json!(["user_requested"]);
        props.get_mut("evidence_refs")?["items"] = json!({"type":"string","const":"current_event"});
        let target = &mut props.get_mut("target")?["properties"];
        target
            .as_object_mut()?
            .retain(|key, _| matches!(key.as_str(), "scope_type" | "subject_ref"));
        target["scope_type"]["enum"] = json!(["person", "person_group"]);
        target["subject_ref"]["enum"] = json!(["current_speaker", "mentioned_user"]);
        "按真实原事件写入事实。create 须给 target、new_content、memory_key 和 category。对人物或群内人物的写入忽略 SELF 专用的 visibility，不改变授权范围。没有 fact_id 时可给 selector 和本人 target；只有唯一精确匹配才定位，模糊候选须先按原 ID 读取；selector 不代替已读版本。correct、invalidate、restore 或 update_metadata 须先按原 fact_id 读取事实；更正只修改所读版本，保留未指定的 confidence。category、kind、importance 和 memory_key 随原版本保存，省略字段保留原值。valid_from／valid_until 按部署时区解读并保存 UTC；update_metadata 省略两者保留期间，给其中一个则替换整个期间；correct 省略两者清除期间。restore 仅恢复已撤回且未到期的原事实；已忘记内容不能恢复。merge 需要先分别读取来源和保留事实；用 merge_fact_id 或 merge_selector 明确同一人物的目标。contest 需要原事实的已读版本；只标争议保留原文，提供 new_content 则创建独立的争议说法。群内 create 可选 person_group 的当前说话人或唯一被提及者；reassign 需先读取本人在本群明确记录的原事实，并用本轮唯一被提及者作新 target。"
    } else {
        item["description"].as_str()?
    };
    Some(Tool {
        name: name.into(),
        description: desc.into(),
        schema,
        parallel_read: false,
        cacheable: false,
        code_direct: false,
        provider: None,
    })
}
pub async fn perform(store: &Store, exec: &str, actor: &str, op: &str) -> Result<()> {
    store
        .write(Mutation::Memory(Box::new(Change::Tool {
            exec: exec.into(),
            actor: actor.into(),
            op: op.into(),
        })))
        .await?;
    Ok(())
}
async fn fact(conn: &mut SqliteConnection, args: &Value) -> Result<String> {
    let number = args["fact_id"]
        .as_i64()
        .ok_or("original fact ID required")?;
    Ok(
        sqlx::query_scalar("SELECT id FROM memory_facts WHERE number=?")
            .bind(number)
            .fetch_one(conn)
            .await?,
    )
}
pub(super) async fn original_event(
    conn: &mut SqliteConnection,
    exec: &str,
    args: &Value,
) -> Result<Source> {
    let event: i64 = if let Some(id) = args["evidence_event_id"].as_i64() {
        sqlx::query_scalar("SELECT id FROM events WHERE id=? AND id IN(SELECT source FROM executions WHERE id=? UNION SELECT event FROM main_inputs WHERE execution=?)").bind(id).bind(exec).bind(exec).fetch_one(&mut *conn).await?
    } else {
        sqlx::query_scalar("SELECT source FROM executions WHERE id=? AND source IS NOT NULL")
            .bind(exec)
            .fetch_one(&mut *conn)
            .await?
    };
    let raw: String = sqlx::query_scalar("SELECT e.envelope FROM events e JOIN executions x ON x.id=? JOIN events origin ON origin.id=x.source JOIN persons author ON author.id=e.raw_actor JOIN persons owner ON owner.id=x.original_actor WHERE e.id=? AND coalesce(author.current_root,author.id)=coalesce(owner.current_root,owner.id) AND e.entry_id=origin.entry_id AND e.bot_stage=origin.bot_stage AND json_extract(e.envelope,'$.kind')='message' AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id)")
        .bind(exec).bind(event).fetch_optional(&mut *conn).await?
        .ok_or("untrusted_trigger_event")?;
    let raw: Value = serde_json::from_str(&raw)?;
    let quote = if let Some(quote) = args["evidence_quote"].as_str() {
        quote.to_owned()
    } else if let Some(text) = raw["text"].as_str() {
        text.to_owned()
    } else {
        raw["segments"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|s| s["type"] == "text")
            .filter_map(|s| s["text"].as_str())
            .collect::<Vec<_>>()
            .join("")
    };
    Ok(Source::Event { id: event, quote })
}
pub(super) async fn paid_active_person(
    db: &mut SqliteConnection,
    r: &Reader,
    exec: &str,
    id: &str,
) -> Result<Value> {
    super::scope::writable(db, r, id).await?;
    let (revision, proof): (i64, String) =
        sqlx::query_as("SELECT revision,proof FROM memory_paid_reads WHERE execution=? AND fact=?")
            .bind(exec)
            .bind(id)
            .fetch_optional(&mut *db)
            .await?
            .ok_or("read the original fact before changing it")?;
    let read = facts::read(db, r, Some(id), "", 1, true, false).await?;
    let row = &read["items"][0];
    if row["revision"] != revision
        || row["proof"] != proof
        || row["status"] != "active"
        || row["kind"] != "person"
    {
        return Err("fact revision or readable evidence changed".into());
    }
    Ok(row.clone())
}

async fn act(
    conn: &mut SqliteConnection,
    r: &Reader,
    exec: &str,
    op: &str,
    name: &str,
    args: Value,
) -> Result<Value> {
    let actor = &r.actor;
    if name == "read_memory" {
        let id = fact(conn, &args).await?;
        let detail = args["view"] == "evidence";
        let value = facts::read(conn, r, Some(&id), "", 1, detail, false).await?;
        let mut row = value["items"][0].clone();
        if detail {
            let own:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM memory_facts f JOIN persons p ON p.id=f.person WHERE f.id=? AND coalesce(p.current_root,p.id)=?)").bind(&id).bind(&r.root).fetch_one(&mut *conn).await?;
            if !own {
                return Err("evidence view requires the original subject".into());
            }
            let evidence = row["evidence"]
                .as_array_mut()
                .ok_or("missing evidence view")?;
            evidence.truncate(args["limit"].as_u64().unwrap_or(5) as usize);
        } else {
            sqlx::query("INSERT INTO memory_paid_reads VALUES(?,?,?,?) ON CONFLICT(execution,fact) DO UPDATE SET revision=excluded.revision,proof=excluded.proof").bind(exec).bind(&id).bind(row["revision"].as_i64()).bind(row["proof"].as_str()).execute(&mut *conn).await?;
        }
        bind_sources(conn, exec, &id).await?;
        return Ok(row);
    }
    if name != "memory_change" {
        return Err("unsupported Memory tool".into());
    }
    let requested = args["operation"]
        .as_str()
        .ok_or("missing memory operation")?;
    if requested == "create" {
        if !args["fact_id"].is_null() {
            return Err("create_does_not_accept_fact_id".into());
        }
        if !args["selector"].is_null() {
            return Err("create_does_not_accept_selector".into());
        }
        if args["target"].is_null() {
            return Err("target_required".into());
        }
        if ["new_content", "memory_key", "category"]
            .into_iter()
            .any(|key| {
                args[key]
                    .as_str()
                    .is_none_or(|s| super::metadata::normalize(s).is_empty())
            })
        {
            return Err("memory_content_key_and_category_required".into());
        }
    }
    let group_target = args["target"]["scope_type"] == "person_group";
    if group_target && !matches!(requested, "create" | "reassign")
        || args["target"]["subject_ref"] == "mentioned_user" && !group_target
    {
        return Err("memory_target_scope_mismatch".into());
    }
    if requested == "merge" {
        return super::merge::apply(conn, r, exec, op, &args).await;
    }
    if !args["merge_fact_id"].is_null() || !args["merge_selector"].is_null() {
        return Err("merge destination only valid for merge".into());
    }
    let metadata = super::metadata::Patch {
        memory_key: args["memory_key"].as_str().map(str::to_owned),
        category: args["category"].as_str().map(str::to_owned),
        kind: args["kind"].as_str().map(str::to_owned),
        importance: args["importance"].as_i64(),
        valid_from: args["valid_from"].as_str().map(str::to_owned),
        valid_until: args["valid_until"].as_str().map(str::to_owned),
    };
    let patch = (matches!(requested, "create" | "correct")
        || [
            "memory_key",
            "category",
            "kind",
            "importance",
            "valid_from",
            "valid_until",
        ]
        .iter()
        .any(|name| !args[name].is_null()))
    .then_some(metadata.clone());
    if group_target || requested == "reassign" {
        return super::attribution::apply(conn, r, exec, op, &args, metadata).await;
    }
    let edit = if requested == "create" {
        Edit::Add {
            subject: Subject {
                kind: "person".into(),
                person: Some(r.base.subject.clone()),
                space: None,
                entry: None,
                visibility: None,
            },
            fact_key: args["memory_key"]
                .as_str()
                .ok_or("memory_key required")?
                .into(),
            body: super::metadata::normalize(
                args["new_content"]
                    .as_str()
                    .ok_or("fact content required")?,
            ),
            confidence: args["confidence"].as_f64(),
            evidence: vec![original_event(conn, exec, &args).await?],
            metadata: patch,
        }
    } else {
        let id = if args["selector"].is_null() {
            fact(conn, &args).await?
        } else {
            super::selectors::locate(conn, r, &args).await?
        };
        if let Some(expected) = args["expected_fact_state"].as_str() {
            let current = facts::read(conn, r, Some(&id), "", 1, false, false).await?;
            let expected = if expected == "invalidated" {
                "retracted"
            } else {
                expected
            };
            if current["items"][0]["status"] != expected {
                return Err("expected_fact_state_mismatch".into());
            }
        }
        if requested == "contest" {
            return super::contest::apply(conn, r, exec, op, &args, &id, metadata).await;
        }
        let (revision, proof) = sqlx::query_as::<_, (i64, String)>(
            "SELECT revision,proof FROM memory_paid_reads WHERE execution=? AND fact=?",
        )
        .bind(exec)
        .bind(&id)
        .fetch_one(&mut *conn)
        .await
        .map_err(|_| "read the original fact before changing it")?;
        if requested == "correct" {
            Edit::Correct {
                fact: id,
                revision,
                proof,
                body: args["new_content"]
                    .as_str()
                    .ok_or("correction content required")?
                    .into(),
                confidence: args["confidence"].as_f64(),
                metadata: patch,
                evidence: if !args["evidence_quote"].is_null()
                    || !args["evidence_refs"].is_null()
                    || !args["evidence_event_id"].is_null()
                {
                    vec![original_event(conn, exec, &args).await?]
                } else {
                    vec![]
                },
            }
        } else if requested == "update_metadata" {
            Edit::Metadata {
                fact: id,
                revision,
                proof,
                fields: metadata,
                body: args["new_content"].as_str().map(str::to_owned),
                confidence: args["confidence"].as_f64(),
                evidence: vec![original_event(conn, exec, &args).await?],
            }
        } else if requested == "invalidate" {
            Edit::Retract {
                fact: id,
                revision,
                proof,
            }
        } else if requested == "restore" {
            Edit::Restore {
                fact: id,
                revision,
                proof,
                evidence: vec![original_event(conn, exec, &args).await?],
            }
        } else {
            return Err("unsupported Memory operation".into());
        }
    };
    let mut receipt = facts::edit(conn, r, exec, actor, op, &edit, false).await?;
    let id = receipt["fact"].as_str().ok_or("fact receipt missing")?;
    let number: i64 = sqlx::query_scalar("SELECT number FROM memory_facts WHERE id=?")
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    bind_sources(conn, exec, id).await?;
    receipt["fact_id"] = json!(number);
    Ok(receipt)
}
pub(super) async fn bind_sources(
    conn: &mut SqliteConnection,
    exec: &str,
    fact: &str,
) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO main_dependencies(execution,event) SELECT ?1,e.id FROM events e WHERE e.id IN(SELECT event FROM memory_evidence WHERE fact=?2 UNION SELECT d.event FROM main_dependencies d JOIN memory_evidence m ON m.origin_execution=d.execution WHERE m.fact=?2 UNION SELECT x.source FROM executions x JOIN memory_evidence m ON m.origin_execution=x.id WHERE m.fact=?2) AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id)").bind(exec).bind(fact).execute(conn).await?;
    Ok(())
}
pub async fn apply(
    conn: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    op: &str,
) -> Result<Value> {
    effects::authorize(conn, exec, actor, false).await?;
    let row=sqlx::query("SELECT state,target,payload FROM operations WHERE namespace=? AND operation_id=? AND action='external'").bind(exec).bind(op).fetch_one(&mut *conn).await?;
    if row.try_get::<String, _>("state")? != "intent" {
        return effects::receipt(conn, exec, op).await;
    }
    let reader = Reader::at(conn, exec, actor, false, true).await?;
    sqlx::query("SAVEPOINT memory_tool")
        .execute(&mut *conn)
        .await?;
    let result = act(
        conn,
        &reader,
        exec,
        op,
        row.try_get("target")?,
        serde_json::from_str(row.try_get("payload")?)?,
    )
    .await;
    let (state, out) = match result {
        Ok(v) => ("confirmed", v),
        Err(e) => {
            sqlx::query("ROLLBACK TO memory_tool")
                .execute(&mut *conn)
                .await?;
            let payload = if let Some(rejected) = e.downcast_ref::<super::selectors::Rejected>() {
                // Candidate content is a real read even when no mutation was
                // authorized. Retain its erasure dependencies after rollback.
                for fact in &rejected.facts {
                    bind_sources(conn, exec, fact).await?;
                }
                rejected.payload()
            } else {
                json!({"error":e.to_string()})
            };
            ("failed", payload)
        }
    };
    sqlx::query("RELEASE memory_tool")
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE operations SET state=?,result=? WHERE namespace=? AND operation_id=? AND state='intent'").bind(state).bind(out.to_string()).bind(exec).bind(op).execute(&mut *conn).await?;
    effects::receipt(conn, exec, op).await
}
