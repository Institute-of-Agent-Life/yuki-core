mod plan;
use crate::{
    Result,
    context::{
        compact_public::{hash, input_budget, wire},
        protocol_objects as objects,
    },
    identity::new_id,
    model::attempt,
    runtime::{execution, main::Config},
};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

const INSTRUCTION: &str = concat!(
    "[Host 上下文压缩要求]\n",
    "本次只整理当前激活的私有执行进展，不执行工具、不发消息。",
    "不要复述前面的公共聊天历史或本轮运行资料。",
    "目标只认本次输入及明确接纳的当前 Work，不从公共群史、工作目录或引用中猜测原任务。",
    "保留当前目标、追加要求、有效约束、已经确认的结果、失败与未知状态、",
    "必要发现、文件/资料句柄、原来源引用和下一步。",
    "不得把未完成事项写成已完成，不要求额外汇报或所有子任务成功。",
    "仅返回可供本轮继续的文字检查点。"
);
fn marker(summary: &str, covered: i64, logical: &str) -> String {
    json!({"kind":"context_compaction","summary":summary,"covered_entries":covered,"previous_protocol_ref":logical,
           "instruction":"继续本次输入与明确接纳的当前 Work；摘要和历史中的其他任务不改变本轮归属。按原执行回执处理，不重做已发生效果。"}).to_string()
}

/// Reapply only replacements bound to the precise original paid source. New native
/// items appended after that source was frozen are retained on this activation.
pub async fn adopt(
    conn: &mut SqliteConnection,
    cfg: &Config,
    exec: &str,
    logical: &str,
    body: &mut Value,
) -> Result<bool> {
    let rows=sqlx::query("SELECT base,selected,retained,covered,summary,logical FROM exec_compacts WHERE execution=? AND source_logical=? AND state='published' ORDER BY rowid")
        .bind(exec).bind(logical).fetch_all(conn).await?;
    let mut changed = false;
    for row in rows {
        let originals: Vec<Value> = serde_json::from_str(row.get("selected"))?;
        let mut base: Vec<Value> = serde_json::from_str(row.get("base"))?;
        let retained: Vec<Value> = serde_json::from_str(row.get("retained"))?;
        let current = objects::parts(cfg.protocol, body)?;
        let Some(append) = current.strip_prefix(originals.as_slice()) else {
            continue;
        };
        base.push(objects::text(
            cfg.protocol,
            "user",
            &marker(row.get("summary"), row.get("covered"), row.get("logical")),
        ));
        base.extend(retained);
        base.extend_from_slice(append);
        changed = true;
        *body = objects::body_with(cfg.protocol, body, &base)?;
    }
    Ok(changed)
}
pub async fn recover(
    conn: &mut SqliteConnection,
    cfg: &Config,
    exec: &str,
    actor: &str,
) -> Result<Option<Value>> {
    let record=sqlx::query("SELECT j.*,p.state AS payment,p.response FROM exec_compacts j LEFT JOIN model_attempts p ON p.logical=j.logical
       WHERE j.execution=? AND j.state='pending' ORDER BY j.rowid DESC,p.ordinal DESC LIMIT 1").bind(exec).fetch_optional(&mut *conn).await?;
    let Some(r) = record else { return Ok(None) };
    let id: String = r.try_get("id")?;
    let Some(state) = r.try_get::<Option<String>, _>("payment")? else {
        return Ok(Some(
            json!({"compact":true,"private_compact":true,"logical":r.get::<String,_>("logical"),"actor":actor,"task":null}),
        ));
    };
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM main_turns t JOIN main_windows w ON w.conversation=t.conversation JOIN executions x ON x.id=t.execution JOIN exec_compacts j ON j.execution=x.id
      WHERE j.id=?1 AND j.generation=t.window AND j.generation=w.generation AND x.work_id IS j.work AND NOT x.cancelled
      AND j.previous IS (SELECT h.id FROM exec_compacts h WHERE h.execution=x.id AND h.state='published' ORDER BY h.rowid DESC LIMIT 1)
      AND j.source_logical=(SELECT r.id FROM model_requests r WHERE r.activation='main:'||x.id ORDER BY r.ordinal DESC LIMIT 1))")
      .bind(&id).fetch_one(&mut *conn).await?;
    let summary = if valid
        && r.get::<String, _>("profile") == cfg.contract()
        && matches!(state.as_str(), "response" | "paired")
        && !super::compact_public::history::stale(conn, exec).await?
    {
        wire::summary(cfg.protocol, r.try_get("response")?).ok()
    } else {
        None
    };
    sqlx::query(
        "UPDATE exec_compacts SET state=?,summary=?,diagnostic=? WHERE id=? AND state='pending'",
    )
    .bind(if summary.is_some() {
        "published"
    } else {
        "rejected"
    })
    .bind(&summary)
    .bind(
        summary
            .is_none()
            .then(|| format!("original source or private summary unavailable: {state}")),
    )
    .bind(id)
    .execute(&mut *conn)
    .await?;
    Ok(Some(json!({"compact_committed":true})))
}
pub async fn consider(
    conn: &mut SqliteConnection,
    cfg: &Config,
    exec: &str,
    actor: &str,
    body: &Value,
    structured: bool,
) -> Result<Option<Value>> {
    let limit = input_budget(
        cfg.max_input_tokens,
        cfg.context_window,
        u64::try_from(cfg.max_output)?,
    );
    let tokens = wire::estimate(cfg, body);
    let hard = limit.is_some_and(|n| tokens > n);
    if !hard && tokens < cfg.compact_soft_tokens.unwrap_or(96_000) {
        return Ok(None);
    }
    let fallback = || -> Result<Option<Value>> {
        if hard {
            Err("model_request_capacity".into())
        } else {
            Ok(None)
        }
    };
    if structured {
        return fallback();
    }
    let history = objects::history(conn, exec, cfg.protocol).await?;
    let Some(last) = history.last() else {
        return fallback();
    };
    let first: String = sqlx::query_scalar(
        "SELECT request_json FROM model_requests WHERE activation=? ORDER BY ordinal LIMIT 1",
    )
    .bind(format!("main:{exec}"))
    .fetch_one(&mut *conn)
    .await?;
    let previous=sqlx::query("SELECT id,covered,summary,logical,source_logical FROM exec_compacts WHERE execution=? AND state='published' ORDER BY rowid DESC LIMIT 1")
        .bind(exec).fetch_optional(&mut *conn).await?;
    let previous_marker = previous.as_ref().map(|r| {
        objects::text(
            cfg.protocol,
            "user",
            &marker(r.get("summary"), r.get("covered"), r.get("logical")),
        )
    });
    let focus = crate::work::context(conn, exec)
        .await?
        .get("goal")
        .cloned()
        .unwrap_or(Value::Null);
    let instruction =
        format!("{INSTRUCTION}\n本轮程序持有的目标归属（null 表示没有当前业务 Work）：{focus}");
    let public:Vec<String>=sqlx::query_scalar("SELECT message FROM exec_public_tails WHERE execution=? AND message IS NOT NULL ORDER BY first_event,last_event").bind(exec).fetch_all(&mut *conn).await?;
    let public: Vec<_> = public
        .iter()
        .map(|m| objects::text(cfg.protocol, "user", m))
        .collect();
    let Some(plan) = plan::select(
        cfg,
        body,
        &serde_json::from_str(&first)?,
        plan::Sources {
            rounds: &history,
            public: &public,
        },
        previous_marker.as_ref(),
        &instruction,
        limit,
    )?
    else {
        return fallback();
    };
    let previous = previous.as_ref().map(|r| r.get::<String, _>("id"));
    let stamp = hash(&json!([
        exec,
        last.logical,
        previous,
        cfg.contract(),
        plan.request
    ]));
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM exec_compacts WHERE execution=? AND candidate=?)",
    )
    .bind(exec)
    .bind(&stamp)
    .fetch_one(&mut *conn)
    .await?;
    if exists {
        return fallback();
    }
    let (work,generation):(Option<String>,i64)=sqlx::query_as("SELECT x.work_id,t.window FROM executions x JOIN main_turns t ON t.execution=x.id WHERE x.id=?").bind(exec).fetch_one(&mut *conn).await?;
    let id = new_id(conn).await?;
    let logical = format!("private-compact:{id}");
    let activation = format!("E:{id}");
    let raw = plan.request.to_string();
    execution::activate(conn, exec, &activation, &cfg.contract()).await?;
    attempt::apply(
        conn,
        &attempt::Action::Reserve(attempt::Reservation {
            activation,
            logical: logical.clone(),
            input: i64::try_from(raw.len())?,
            output: cfg.max_output,
            safe_rate_limit_once: false,
            native_tools: cfg.native_web_search,
            request: raw,
        }),
    )
    .await?;
    sqlx::query("INSERT INTO exec_compacts(id,execution,work,generation,profile,source_logical,previous,candidate,covered,logical,base,selected,retained,state)
      VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,'pending')").bind(id).bind(exec).bind(work).bind(generation).bind(cfg.contract()).bind(&last.logical).bind(previous)
      .bind(stamp).bind(i64::try_from(plan.covered)?).bind(&logical).bind(serde_json::to_string(&plan.base)?).bind(serde_json::to_string(&plan.original)?).bind(serde_json::to_string(&plan.retained)?).execute(conn).await?;
    Ok(Some(
        json!({"compact":true,"private_compact":true,"logical":logical,"actor":actor,"task":null}),
    ))
}

/// Restore a Work's own private checkpoint. Only original, completely paired
/// operations can supply an unobserved round on another activation.
pub async fn restore_work(
    conn: &mut SqliteConnection,
    exec: &str,
    work: &str,
    ctx: &mut Value,
) -> Result<()> {
    let row=sqlx::query("SELECT j.execution,j.summary,j.source_logical,j.logical,w.profile FROM exec_compacts j JOIN work_nodes w ON w.id=j.work WHERE j.work=?1 AND j.execution!=?2 AND j.profile=(SELECT profile_contract FROM activations WHERE id='main:'||?2) AND j.state='published' AND j.summary IS NOT NULL ORDER BY j.rowid DESC LIMIT 1")
        .bind(work).bind(exec).fetch_optional(&mut *conn).await?;
    let Some(row) = row else { return Ok(()) };
    let original: &str = row.try_get("execution")?;
    let logical: &str = row.try_get("source_logical")?;
    sqlx::query("INSERT OR IGNORE INTO main_dependencies SELECT ?,event FROM main_dependencies WHERE execution=?").bind(exec).bind(original).execute(&mut *conn).await?;
    ctx["execution_checkpoint"] = json!(row.get::<String, _>("summary"));
    let observed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM model_requests r JOIN model_attempts a ON a.logical=r.id JOIN model_requests src ON src.id=? WHERE r.activation=src.activation AND r.ordinal>src.ordinal AND a.state IN('response','paired'))")
        .bind(logical).fetch_one(&mut *conn).await?;
    if observed {
        return Ok(());
    }
    let cfg: Config = serde_json::from_str(row.try_get("profile")?)?;
    let rounds = objects::history(conn, original, cfg.protocol).await?;
    let Some(round) = rounds.into_iter().find(|r| r.logical == logical && r.calls) else {
        return Ok(());
    };
    let Some(unit) = round.portable.first() else {
        return Ok(());
    };
    let Some(raw) = objects::user_text(cfg.protocol, unit) else {
        return Ok(());
    };
    let evidence: Value = serde_json::from_str(raw)?;
    let received:Vec<String>=sqlx::query_scalar("SELECT r.request_json FROM model_requests r JOIN model_attempts p ON p.logical=r.id JOIN activations a ON a.id=r.activation JOIN work_runs w ON w.execution=a.execution WHERE w.work=?1 AND r.activation='main:'||a.execution AND r.rowid>(SELECT rowid FROM model_requests WHERE id=?2) AND p.state IN('response','paired') AND r.request_json IS NOT NULL").bind(work).bind(row.get::<String,_>("logical")).fetch_all(&mut *conn).await?;
    for bytes in received {
        for part in objects::parts(cfg.protocol, &serde_json::from_str(&bytes)?)? {
            let Some(raw) = objects::user_text(cfg.protocol, &part)
                .and_then(|s| s.strip_prefix("\n\n<runtime_context>\n"))
                .and_then(|s| s.strip_suffix("\n</runtime_context>\n\n"))
            else {
                continue;
            };
            let Ok(context) = serde_json::from_str::<Value>(raw) else {
                continue;
            };
            if context["work_id"] == work && context["unobserved_tool_round"] == evidence {
                return Ok(());
            }
            if context["materials"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|v| v["work_id"] == work && v["unobserved_tool_round"] == evidence)
            {
                return Ok(());
            }
        }
    }
    ctx["unobserved_tool_round"] = evidence;
    Ok(())
}
