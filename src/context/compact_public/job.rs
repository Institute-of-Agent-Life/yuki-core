use super::{INSTRUCTION, checkpoint, hash, history, input_budget, scan, wire};
use crate::{
    Result,
    identity::new_id,
    model::{attempt, chat},
    runtime::{execution, main::Config},
};
use serde_json::{Value, json};
use sqlx::{Row, SqliteConnection};

async fn permitted(conn: &mut SqliteConnection, event: i64) -> Result<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM events e JOIN entries d ON d.id=e.entry_id JOIN persons p ON p.id=e.raw_actor JOIN persons root ON root.id=coalesce(p.current_root,p.id) WHERE e.id=? AND p.enabled AND root.enabled AND (d.scope='private' OR root.admin))")
        .bind(event).fetch_one(conn).await?)
}
pub async fn manual(
    conn: &mut SqliteConnection,
    cfg: &Config,
    event: i64,
    focus: &str,
) -> Result<()> {
    let proof = crate::ingress::admit::claim(conn, event).await?;
    let exec = proof["execution"]
        .as_str()
        .ok_or("original manual execution missing")?;
    if sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM compact_manual WHERE event=?)")
        .bind(event)
        .fetch_one(&mut *conn)
        .await?
    {
        return Ok(());
    }
    let entry: String = sqlx::query_scalar("SELECT entry_id FROM events WHERE id=?")
        .bind(event)
        .fetch_one(&mut *conn)
        .await?;
    let generation = crate::runtime::main::window(conn, &entry).await?;
    sqlx::query("INSERT INTO compact_manual VALUES(?,?,?)")
        .bind(event)
        .bind(exec)
        .bind(focus)
        .execute(&mut *conn)
        .await?;
    let permitted = permitted(conn, event).await?;
    sqlx::query("INSERT INTO main_turns(execution,conversation,window,ready_after,diagnostic) VALUES(?,?,?,0,?)").bind(exec).bind(&entry).bind(generation).bind((!permitted).then_some("compact_admin_required")).execute(&mut *conn).await?;
    execution::root(conn, exec, cfg.root_slots, cfg.max_input_units).await?;
    sqlx::query("INSERT OR IGNORE INTO main_dependencies VALUES(?,?)")
        .bind(exec)
        .bind(event)
        .execute(&mut *conn)
        .await?;
    if !permitted {
        sqlx::query("UPDATE executions SET state='held' WHERE id=?")
            .bind(exec)
            .execute(conn)
            .await?;
    }
    Ok(())
}
async fn finish_manual(conn: &mut SqliteConnection, exec: &str, diagnostic: &str) -> Result<()> {
    sqlx::query("UPDATE main_turns SET diagnostic=?,result=? WHERE execution=?")
        .bind(diagnostic)
        .bind(json!({"kind":"public_compact","diagnostic":diagnostic}).to_string())
        .bind(exec)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE executions SET state='completed' WHERE id=?")
        .bind(exec)
        .execute(conn)
        .await?;
    Ok(())
}
pub(super) async fn current_published(
    conn: &mut SqliteConnection,
    entry: &str,
    generation: i64,
) -> Result<Option<(String, i64, i64, String)>> {
    let mut head = history::head(conn, entry, generation).await?;
    while let Some((ref id, _, _, _)) = head {
        if valid(conn, id).await? {
            break;
        }
        sqlx::query("WITH RECURSIVE chain(id) AS(SELECT ? UNION SELECT j.id FROM compact_jobs j JOIN chain c ON j.prior_checkpoint=c.id) UPDATE compact_jobs SET summary=NULL,state='invalid',diagnostic='source_version_changed' WHERE id IN chain").bind(id).execute(&mut *conn).await?;
        head = history::head(conn, entry, generation).await?;
    }
    Ok(head)
}
async fn valid(conn: &mut SqliteConnection, id: &str) -> Result<bool> {
    let rows=sqlx::query("WITH RECURSIVE chain(id) AS(SELECT ? UNION SELECT j.prior_checkpoint FROM compact_jobs j JOIN chain c ON j.id=c.id WHERE j.prior_checkpoint IS NOT NULL) SELECT f.item,f.revision,f.source_hash,f.source_key FROM compact_fragments f JOIN chain ON f.job=chain.id ORDER BY f.ordinal").bind(id).fetch_all(&mut *conn).await?;
    if rows.is_empty() {
        return Ok(false);
    }
    for row in rows {
        let Some(f) = history::current(conn, row.try_get("item")?, "UTC").await? else {
            return Ok(false);
        };
        if f.revision != row.try_get::<i64, _>("revision")?
            || f.digest != row.try_get::<String, _>("source_hash")?
            || f.key != row.try_get::<String, _>("source_key")?
        {
            return Ok(false);
        }
    }
    Ok(true)
}
/// One writer transaction publishes the stored paid response. It does not reserve the
/// next normal turn, so exhaustion of the root cannot roll back a successful publication.
pub async fn recover(
    conn: &mut SqliteConnection,
    cfg: &Config,
    exec: &str,
    actor: &str,
) -> Result<Option<Value>> {
    let row=sqlx::query("SELECT j.id,j.logical,j.entry,j.generation,j.prior_checkpoint,p.state,p.response,p.diagnostic FROM compact_jobs j LEFT JOIN model_attempts p ON p.logical=j.logical WHERE j.execution=? AND j.state='pending' ORDER BY j.rowid DESC,p.ordinal DESC LIMIT 1").bind(exec).fetch_optional(&mut *conn).await?;
    let Some(row) = row else { return Ok(None) };
    let id: String = row.try_get("id")?;
    let logical: String = row.try_get("logical")?;
    let state: Option<String> = row.try_get("state")?;
    let Some(state) = state else {
        return Ok(Some(
            json!({"compact":true,"logical":logical,"actor":actor,"task":null}),
        ));
    };
    let (entry, generation): (String, i64) = (row.try_get("entry")?, row.try_get("generation")?);
    let prior = row.try_get::<Option<String>, _>("prior_checkpoint")?;
    let summary = if matches!(state.as_str(), "response" | "paired") {
        wire::summary(cfg.protocol, row.try_get("response")?).ok()
    } else {
        None
    };
    let generation_ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM main_windows WHERE conversation=? AND generation=?)",
    )
    .bind(&entry)
    .bind(generation)
    .fetch_one(&mut *conn)
    .await?;
    let previous = current_published(conn, &entry, generation)
        .await?
        .map(|h| h.0);
    let manual_event: Option<i64> =
        sqlx::query_scalar("SELECT event FROM compact_manual WHERE execution=?")
            .bind(exec)
            .fetch_optional(&mut *conn)
            .await?;
    let authorized = match manual_event {
        Some(event) => permitted(conn, event).await?,
        None => true,
    };
    let published = authorized
        && summary.is_some()
        && generation_ok
        && previous == prior
        && valid(conn, &id).await?;
    sqlx::query(
        "UPDATE compact_jobs SET state=?,summary=?,diagnostic=? WHERE id=? AND state='pending'",
    )
    .bind(if published { "published" } else { "rejected" })
    .bind(if published { summary } else { None })
    .bind(
        (!published)
            .then(|| format!("paid:{state}; original source, summary or checkpoint unavailable")),
    )
    .bind(&id)
    .execute(&mut *conn)
    .await?;
    if manual_event.is_some() {
        finish_manual(
            conn,
            exec,
            if published {
                "published"
            } else {
                "public_candidate_rejected"
            },
        )
        .await?;
        return Ok(Some(Value::Null));
    }
    Ok(Some(json!({"compact_committed":true})))
}
pub async fn consider(
    conn: &mut SqliteConnection,
    cfg: &Config,
    exec: &str,
    actor: &str,
    body: &mut Value,
    structured: bool,
) -> Result<Option<Value>> {
    let manual: Option<(i64, Option<String>)> =
        sqlx::query_as("SELECT event,focus FROM compact_manual WHERE execution=?")
            .bind(exec)
            .fetch_optional(&mut *conn)
            .await?;
    if let Some((event, _)) = &manual
        && !permitted(conn, *event).await?
    {
        crate::runtime::main::hold_original(conn, exec, "compact_admin_required").await?;
        return Ok(Some(Value::Null));
    }
    let manual = manual.map(|(_, focus)| focus.unwrap_or_default());
    let limit = input_budget(
        cfg.max_input_tokens,
        cfg.context_window,
        u64::try_from(cfg.max_output)?,
    );
    let view = sqlx::query(
        "SELECT entry,generation,checkpoint,static_program FROM compact_views WHERE execution=?",
    )
    .bind(exec)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(view) = view else {
        if limit.is_some_and(|l| wire::estimate(cfg, body) > l) {
            return Err("model_request_capacity".into());
        }
        return Ok(None);
    };
    let entry: String = view.try_get("entry")?;
    let generation: i64 = view.try_get("generation")?;
    let same_window: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM main_windows WHERE conversation=? AND generation=?)",
    )
    .bind(&entry)
    .bind(generation)
    .fetch_one(&mut *conn)
    .await?;
    if !same_window {
        if manual.is_some() {
            finish_manual(conn, exec, "window_reset").await?;
        } else {
            crate::runtime::main::hold_original(conn, exec, "window_reset").await?;
        }
        return Ok(Some(Value::Null));
    }
    let previous = current_published(conn, &entry, generation).await?;
    let prior = view.try_get::<Option<String>, _>("checkpoint")?;
    if manual.is_none() && previous.as_ref().map(|h| h.0.as_str()) == prior.as_deref() {
        let tokens = wire::estimate(cfg, body);
        if tokens < cfg.compact_soft_tokens.unwrap_or(96_000) && limit.is_none_or(|l| tokens <= l) {
            return Ok(None);
        }
    }
    let original = head_and_items(conn, exec, &view).await?;
    let fragments=sqlx::query("SELECT ordinal,item,revision,source_key,source_hash,message FROM compact_view_items WHERE execution=? AND message IS NOT NULL ORDER BY ordinal").bind(exec).fetch_all(&mut *conn).await?;
    let last = previous.as_ref().map_or(0, |h| h.1);
    let latest = previous.as_ref().map(|h| h.0.as_str());
    let upper = fragments
        .last()
        .map(|r| r.try_get::<i64, _>("item"))
        .transpose()?
        .unwrap_or(0);
    let usable = latest == prior.as_deref() || last <= upper;
    let checkpoint_messages = if usable {
        history::active_checkpoint(conn, latest).await?
    } else {
        Vec::new()
    };
    let rows: Vec<_> = fragments
        .iter()
        .filter(|r| r.get::<i64, _>("item") > last)
        .collect();
    let items: Vec<Value> = rows
        .iter()
        .map(|r| serde_json::from_str(r.get("message")))
        .collect::<std::result::Result<_, _>>()?;
    let opaque = wire::opaque(cfg.protocol, body);
    if usable && latest != prior.as_deref() && !opaque {
        let mut after = checkpoint_messages.clone();
        after.extend(items.clone());
        if wire::replace(body, cfg.protocol, &original, &after).is_err() {
            wire::replace(body, cfg.protocol, &after, &after)?;
        }
    }
    let tokens = wire::estimate(cfg, body);
    let hard = limit.is_some_and(|l| tokens > l);
    if manual.is_none() && !hard && tokens < cfg.compact_soft_tokens.unwrap_or(96_000) {
        return Ok(None);
    }
    let focus = manual.as_deref().unwrap_or("");
    let static_program: String = view.try_get("static_program")?;
    let mut leading = vec![json!({"role":"system","content":static_program})];
    leading.extend(checkpoint_messages);
    let cut = if usable && !opaque && !structured {
        scan(&wire::base(cfg, body), &leading, &items, focus, limit)?
    } else {
        None
    };
    let Some(cut) = cut else {
        if manual.is_some() {
            finish_manual(conn, exec, "no_legal_public_prefix").await?;
            return Ok(Some(Value::Null));
        }
        if hard {
            return Err("model_request_capacity".into());
        }
        return Ok(None);
    };
    let selected = &rows[..cut];
    let stamp = json!([
        entry,
        generation,
        latest,
        cfg.contract(),
        focus,
        hash(&json!(static_program)),
        selected
            .iter()
            .map(|r| json!([
                r.get::<i64, _>("item"),
                r.get::<i64, _>("revision"),
                r.get::<String, _>("source_hash")
            ]))
            .collect::<Vec<_>>()
    ]);
    let source_hash = hash(&stamp);
    let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM compact_jobs WHERE entry=? AND generation=? AND source_hash=?)").bind(&entry).bind(generation).bind(&source_hash).fetch_one(&mut *conn).await?;
    if exists {
        if manual.is_some() {
            finish_manual(conn, exec, "original_public_candidate_already_recorded").await?;
            return Ok(Some(Value::Null));
        }
        if hard {
            return Err("model_request_capacity".into());
        }
        return Ok(None);
    }
    let id = new_id(conn).await?;
    let logical = format!("compact:{id}");
    let activation = format!("P:{id}");
    let (manifest, code) = crate::tools::registry::pinned(conn, exec, cfg).await?;
    let mut compact_messages = leading;
    compact_messages.extend_from_slice(&items[..cut]);
    compact_messages.push(json!({"role":"user","content":if focus.is_empty(){INSTRUCTION.into()}else{format!("{INSTRUCTION}\n整理重点：{focus}")}}));
    let mut layout = json!({"model":cfg.model,"messages":compact_messages,"max_tokens":cfg.max_output,"stream":false});
    chat::configure(&mut layout, None, manifest.declarations(code));
    let mut wire = cfg.protocol.request(layout, cfg, None)?;
    if cfg.protocol.is_messages() {
        crate::model::messages::cache_last(&mut wire)?;
    }
    let raw = wire.to_string();
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
    let coverage = selected
        .iter()
        .filter_map(|r| {
            r.get::<String, _>("source_key")
                .strip_prefix("event:")
                .and_then(|v| v.parse::<i64>().ok())
        })
        .max()
        .or(previous.as_ref().map(|h| h.2))
        .unwrap_or(0);
    sqlx::query("INSERT INTO compact_jobs(id,execution,entry,generation,prior_checkpoint,source_hash,profile,coverage,last_item,logical,focus,state) VALUES(?,?,?,?,?,?,?,?,?,?,?,'pending')").bind(&id).bind(exec).bind(entry).bind(generation).bind(latest).bind(source_hash).bind(cfg.contract()).bind(coverage).bind(selected.last().unwrap().get::<i64,_>("item")).bind(&logical).bind(focus).execute(&mut *conn).await?;
    for (ord, r) in selected.iter().enumerate() {
        sqlx::query("INSERT INTO compact_fragments VALUES(?,?,?,?,?,?,?)")
            .bind(&id)
            .bind(i64::try_from(ord)?)
            .bind(r.get::<i64, _>("item"))
            .bind(r.get::<i64, _>("revision"))
            .bind(r.get::<String, _>("source_key"))
            .bind(r.get::<String, _>("source_hash"))
            .bind(r.get::<String, _>("message"))
            .execute(&mut *conn)
            .await?;
    }
    Ok(Some(
        json!({"compact":true,"logical":logical,"actor":actor,"task":null}),
    ))
}
async fn head_and_items(
    conn: &mut SqliteConnection,
    exec: &str,
    view: &sqlx::sqlite::SqliteRow,
) -> Result<Vec<Value>> {
    let prior: Option<String> = view.try_get("checkpoint")?;
    let mut original = Vec::new();
    if let Some(id) = prior {
        let row = sqlx::query(
            "SELECT coverage,summary FROM compact_jobs WHERE id=? AND summary IS NOT NULL",
        )
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or("adopted checkpoint unavailable")?;
        original.push(checkpoint(
            row.try_get("summary")?,
            row.try_get("coverage")?,
        ));
    }
    let recorded:Vec<String>=sqlx::query_scalar("SELECT message FROM compact_view_items WHERE execution=? AND message IS NOT NULL ORDER BY ordinal").bind(exec).fetch_all(conn).await?;
    for raw in recorded {
        original.push(serde_json::from_str(&raw)?);
    }
    Ok(original)
}
