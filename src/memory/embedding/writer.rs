use super::{
    profile::{Profile, bounded, digest, document},
    *,
};
use crate::{identity::new_id, model::attempt, runtime::execution};
use sqlx::{Row, SqliteConnection};

const SELECT_FACT:&str="SELECT f.id,f.revision,f.created_execution,v.execution,coalesce(d.kind,'fact') AS kind,coalesce(d.category,'') AS category,f.fact_key,v.body,
 coalesce((SELECT e.entry_id FROM executions x JOIN events e ON e.id=x.source WHERE x.id=f.created_execution),
 (SELECT conversation FROM executions x JOIN main_origins o ON o.ref=x.origin WHERE x.id=f.created_execution)) AS entry
 FROM memory_facts f JOIN memory_versions v ON v.fact=f.id AND v.revision=f.revision LEFT JOIN extract_fact_details d ON d.fact=f.id
 WHERE f.status='active' AND v.body IS NOT NULL";
const MISSING:&str=" AND NOT EXISTS(SELECT 1 FROM fact_vectors z WHERE z.fact=f.id AND z.revision=f.revision AND z.profile=?1)
 AND NOT EXISTS(SELECT 1 FROM embedding_items i JOIN embedding_requests j ON j.logical=i.logical WHERE i.fact=f.id AND i.revision=f.revision AND i.state NOT IN('released','stale') AND j.profile=?1)";
pub async fn needs(store: &Store, p: &Profile) -> Result<bool> {
    Ok(sqlx::query_scalar(&format!("SELECT EXISTS({SELECT_FACT}{MISSING}) OR EXISTS(SELECT 1 FROM embedding_requests j WHERE j.profile=?1 AND j.state='prepared' AND j.purpose!='query')"))
        .bind(p.identity()).fetch_one(store.readers()).await?)
}
async fn dependency(conn: &mut SqliteConnection, exec: &str, source_exec: &str) -> Result<()> {
    sqlx::query("INSERT OR IGNORE INTO main_dependencies SELECT ?1,e.id FROM events e WHERE e.id IN(SELECT source FROM executions WHERE id=?2 UNION SELECT event FROM main_dependencies WHERE execution=?2)")
        .bind(exec).bind(source_exec).execute(conn).await?;
    Ok(())
}
async fn root(conn: &mut SqliteConnection, entry: &str, parent: Option<&str>) -> Result<String> {
    let exec = new_id(conn).await?;
    let origin = format!("embedding:{exec}");
    sqlx::query("INSERT INTO main_origins(ref,kind,conversation,parent) VALUES(?,'embedding',?,?)")
        .bind(&origin)
        .bind(entry)
        .bind(parent)
        .execute(&mut *conn)
        .await?;
    sqlx::query("INSERT INTO executions(id,origin,state) VALUES(?,?,'running')")
        .bind(&exec)
        .bind(origin)
        .execute(&mut *conn)
        .await?;
    if let Some(parent) = parent {
        dependency(conn, &exec, parent).await?;
        // The registered internal child may only inherit its original parent's budget.
        let root:String=sqlx::query_scalar("SELECT m.root FROM budget_members m JOIN main_origins o ON o.parent=m.execution JOIN executions x ON x.origin=o.ref WHERE x.id=? AND o.kind='embedding'").bind(&exec).fetch_one(&mut *conn).await?;
        sqlx::query("INSERT INTO budget_members VALUES(?,?)")
            .bind(&exec)
            .bind(root)
            .execute(&mut *conn)
            .await?;
    } else {
        execution::root(conn, &exec, 1, 2_000_000).await?;
    }
    Ok(exec)
}
struct Pending<'a> {
    exec: &'a str,
    id: &'a str,
    purpose: &'a str,
    cache: Option<&'a str>,
    texts: &'a [String],
}
async fn reserve(conn: &mut SqliteConnection, p: &Profile, job: Pending<'_>) -> Result<()> {
    let wire = p.wire(job.texts, job.purpose == "query").to_string();
    let activation = format!("embedding:{}", job.exec);
    execution::activate(conn, job.exec, &activation, &p.identity()).await?;
    attempt::apply(
        conn,
        &attempt::Action::Reserve(attempt::Reservation {
            activation,
            logical: job.id.to_string(),
            input: i64::try_from(wire.chars().count())?,
            output: 0,
            safe_rate_limit_once: false,
            native_tools: false,
            request: wire,
        }),
    )
    .await?;
    sqlx::query("INSERT INTO embedding_requests(logical,profile,dimension,purpose,execution,cache_key,privacy_epoch,state,expected) SELECT ?,?,?,?,?,?,epoch,'prepared',? FROM embedding_privacy")
        .bind(job.id).bind(p.identity()).bind(i64::try_from(p.dimensions)?).bind(job.purpose).bind(job.exec).bind(job.cache).bind(i64::try_from(job.texts.len())?).execute(conn).await?;
    Ok(())
}
pub async fn documents(conn: &mut SqliteConnection, p: &Profile) -> Result<Value> {
    let active:Option<String>=sqlx::query_scalar("SELECT logical FROM embedding_requests WHERE profile=? AND purpose!='query' AND state='prepared' ORDER BY rowid LIMIT 1").bind(p.identity()).fetch_optional(&mut *conn).await?;
    if let Some(id) = active {
        return Ok(json!({"logical":id}));
    }
    let rows = sqlx::query(&format!(
        "{SELECT_FACT}{MISSING} ORDER BY f.number LIMIT ?2"
    ))
    .bind(p.identity())
    .bind(i64::try_from(p.batch_size)?)
    .fetch_all(&mut *conn)
    .await?;
    let Some(first) = rows.first() else {
        return Ok(Value::Null);
    };
    let entry: &str = first.try_get("entry")?;
    let purpose = |r: &sqlx::sqlite::SqliteRow| -> Result<&'static str> {
        Ok(if r.try_get::<i64, _>("revision")? > 1 {
            "reembed"
        } else {
            "document"
        })
    };
    let mut items = Vec::new();
    let mut texts = Vec::new();
    let mut kind = None;
    for row in &rows {
        if row.try_get::<&str, _>("entry")? != entry {
            continue;
        }
        let id: &str = row.try_get("id")?;
        let previous:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM embedding_items i JOIN embedding_requests j ON j.logical=i.logical WHERE fact=? AND (j.profile!=? OR i.revision!=?))").bind(id).bind(p.identity()).bind(row.try_get::<i64,_>("revision")?).fetch_one(&mut *conn).await?;
        let k = if previous { "reembed" } else { purpose(row)? };
        if *kind.get_or_insert(k) != k {
            continue;
        }
        let text = document(
            row.try_get("kind")?,
            row.try_get("category")?,
            row.try_get("fact_key")?,
            row.try_get("body")?,
            p.max_characters,
        );
        items.push((row, digest(text.as_bytes())));
        texts.push(text);
    }
    let exec = root(conn, entry, None).await?;
    let logical = format!("embed-doc:{exec}");
    for (row, _) in &items {
        dependency(conn, &exec, row.try_get("execution")?).await?;
        dependency(conn, &exec, row.try_get("created_execution")?).await?;
        sqlx::query("INSERT OR IGNORE INTO main_dependencies SELECT ?1,e.id FROM events e JOIN memory_evidence me ON me.fact=?2 WHERE me.event=e.id OR e.id IN(SELECT source FROM executions WHERE id=me.origin_execution UNION SELECT event FROM main_dependencies WHERE execution=me.origin_execution)").bind(&exec).bind(row.try_get::<&str,_>("id")?).execute(&mut *conn).await?;
    }
    reserve(
        conn,
        p,
        Pending {
            exec: &exec,
            id: &logical,
            purpose: kind.ok_or("missing embedding purpose")?,
            cache: None,
            texts: &texts,
        },
    )
    .await?;
    for (i, (row, hash)) in items.iter().enumerate() {
        sqlx::query("INSERT INTO embedding_items VALUES(?,?,?,?,?,'pending')")
            .bind(&logical)
            .bind(i64::try_from(i)?)
            .bind(row.try_get::<&str, _>("id")?)
            .bind(row.try_get::<i64, _>("revision")?)
            .bind(hash)
            .execute(&mut *conn)
            .await?;
    }
    Ok(json!({"logical":logical}))
}
pub async fn query_job(
    conn: &mut SqliteConnection,
    p: &Profile,
    exec: &str,
    actor: &str,
    op: &str,
    raw: &str,
) -> Result<Value> {
    let reader = crate::memory::scope::Reader::at(conn, exec, actor, false, true).await?;
    let text = bounded(raw, p.max_characters);
    if text.is_empty() || op.trim().is_empty() || op.len() > 256 {
        return Err("nonempty bounded query and operation required".into());
    }
    let profile = p.identity();
    let cache = digest(format!("{profile}\0{text}").as_bytes());
    let logical = format!(
        "embed-query:{}",
        digest(json!([exec, op]).to_string().as_bytes())
    );
    if let Some(row)=sqlx::query("SELECT profile,cache_key,state,privacy_epoch=(SELECT epoch FROM embedding_privacy) AS valid FROM embedding_requests WHERE logical=?").bind(&logical).fetch_optional(&mut *conn).await?{
        if row.get::<&str,_>("profile")!=profile || row.get::<Option<&str>,_>("cache_key")!=Some(cache.as_str()) || !row.get::<bool,_>("valid"){return Err("original embedding query unavailable or changed".into())}
        if row.get::<&str,_>("state")=="done" {
            let raw:String=sqlx::query_scalar("SELECT response FROM model_attempts WHERE logical=? AND state='response'").bind(&logical).fetch_one(&mut *conn).await?;
            let values=p.rows(&serde_json::from_str(&raw)?,1);
            return Ok(json!({"vector":values[0],"original":true,"profile":profile}));
        }
        return Ok(json!({"logical":logical}));
    }
    let hit=sqlx::query("SELECT cache_key,profile,epoch=(SELECT epoch FROM embedding_privacy) AS current FROM embedding_query_hits WHERE logical=?").bind(&logical).fetch_optional(&mut *conn).await?;
    if let Some(row) = &hit
        && (!row.get::<bool, _>("current")
            || row.get::<Option<&str>, _>("cache_key") != Some(&cache)
            || row.get::<&str, _>("profile") != profile)
    {
        return Err("cached operation erased or request changed".into());
    }
    if let Some(v) = sqlx::query_scalar::<_, String>(
        "SELECT vector FROM temp.embedding_cache WHERE key=? AND expires>unixepoch()",
    )
    .bind(&cache)
    .fetch_optional(&mut *conn)
    .await?
    {
        sqlx::query("INSERT OR IGNORE INTO embedding_query_hits SELECT ?,?,?,?,epoch FROM embedding_privacy")
            .bind(&logical).bind(exec).bind(&profile).bind(&cache).execute(&mut *conn).await?;
        return Ok(
            json!({"vector":serde_json::from_str::<Value>(&v)?,"cached":true,"profile":profile}),
        );
    }
    if hit.is_some() {
        return Err("original cached vector expired; use a new request".into());
    }
    let child = root(conn, &reader.base.home, Some(exec)).await?;
    reserve(
        conn,
        p,
        Pending {
            exec: &child,
            id: &logical,
            purpose: "query",
            cache: Some(&cache),
            texts: &[text],
        },
    )
    .await?;
    Ok(json!({"logical":logical}))
}
pub async fn dispatch(conn: &mut SqliteConnection, p: &Profile, id: &str) -> Result<Value> {
    let row = sqlx::query("SELECT execution,state,profile FROM embedding_requests WHERE logical=?")
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    if row.get::<&str, _>("profile") != p.identity() || row.get::<&str, _>("state") != "prepared" {
        return Err("original embedding request not dispatchable".into());
    }
    execution::eligible(conn, row.try_get("execution")?).await?;
    let res = attempt::apply(
        conn,
        &attempt::Action::Dispatch {
            logical: id.into(),
            attempt: format!("{id}:0"),
        },
    )
    .await?;
    Ok(res)
}
pub async fn settle(
    conn: &mut SqliteConnection,
    p: &Profile,
    id: &str,
    http: &Outcome,
) -> Result<Value> {
    let row=sqlx::query("SELECT purpose,expected,execution,cache_key,state,privacy_epoch=(SELECT epoch FROM embedding_privacy) AS same_epoch FROM embedding_requests WHERE logical=? AND profile=?").bind(id).bind(p.identity()).fetch_one(&mut *conn).await?;
    let exec: &str = row.try_get("execution")?;
    let attempt = format!("{id}:0");
    let query = row.try_get::<&str, _>("purpose")? == "query";
    let (raw, values) = match http {
        Outcome::Ok(raw) => {
            let value: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
            let usage=json!({"input_tokens":value["usage"]["prompt_tokens"].as_u64().or(value["usage"]["total_tokens"].as_u64())}).to_string();
            let receipt = attempt::apply(
                conn,
                &attempt::Action::Response {
                    attempt,
                    body: raw.clone(),
                    intents: "[]".into(),
                    usage,
                },
            )
            .await?;
            let valid =
                receipt["state"] == "response" && row.try_get::<&str, _>("state")? != "erased";
            let vectors = if valid {
                p.rows(&value, usize::try_from(row.try_get::<i64, _>("expected")?)?)
            } else {
                vec![]
            };
            (valid, vectors)
        }
        Outcome::Rejected(code) => {
            attempt::apply(
                conn,
                &attempt::Action::Rejected {
                    attempt,
                    status: *code,
                    unbilled_before_model: false,
                    body: "{}".into(),
                    usage: "null".into(),
                },
            )
            .await?;
            (false, vec![])
        }
        Outcome::Unknown => {
            attempt::apply(conn, &attempt::Action::Unknown { attempt }).await?;
            (false, vec![])
        }
    };
    if row.get::<&str, _>("state") == "erased" {
        return Ok(json!({"state":"erased"}));
    }
    let mut stored = 0;
    if query {
        if raw
            && row.get::<bool, _>("same_epoch")
            && let Some(Some(vector)) = values.first()
        {
            sqlx::query("DELETE FROM temp.embedding_cache WHERE expires<=unixepoch() OR key IN(SELECT key FROM temp.embedding_cache ORDER BY rowid DESC LIMIT -1 OFFSET 255)").execute(&mut *conn).await?;
            sqlx::query("INSERT OR REPLACE INTO temp.embedding_cache VALUES(?,?,unixepoch()+300)")
                .bind(row.get::<&str, _>("cache_key"))
                .bind(serde_json::to_string(vector)?)
                .execute(&mut *conn)
                .await?;
            stored = 1;
        }
    } else {
        let items=sqlx::query("SELECT ordinal,fact,revision,content_hash,state FROM embedding_items WHERE logical=? ORDER BY ordinal").bind(id).fetch_all(&mut *conn).await?;
        for item in items {
            let i: usize = item.get::<i64, _>("ordinal").try_into()?;
            if item.get::<&str, _>("state") != "pending" {
                continue;
            }
            let fact: &str = item.try_get("fact")?;
            let current = sqlx::query(&format!("{SELECT_FACT} AND f.id=?"))
                .bind(fact)
                .fetch_optional(&mut *conn)
                .await?;
            let current = current.filter(|r| {
                r.get::<i64, _>("revision") == item.get::<i64, _>("revision")
                    && digest(
                        document(
                            r.get("kind"),
                            r.get("category"),
                            r.get("fact_key"),
                            r.get("body"),
                            p.max_characters,
                        )
                        .as_bytes(),
                    ) == item.get::<&str, _>("content_hash")
            });
            let next = if current.is_none() {
                "stale"
            } else if let Some(Some(vector)) = values.get(i) {
                let bytes: Vec<u8> = vector.iter().flat_map(|x| x.to_le_bytes()).collect();
                sqlx::query("INSERT INTO fact_vectors VALUES(?,?,'document',?,?,?,?) ON CONFLICT(fact,profile,purpose,content_hash) DO NOTHING")
                        .bind(fact).bind(p.identity()).bind(i64::try_from(p.dimensions)?).bind(item.try_get::<&str,_>("content_hash")?).bind(item.get::<i64,_>("revision")).bind(bytes).execute(&mut *conn).await?;
                stored += 1;
                "stored"
            } else if matches!(http, Outcome::Unknown) {
                "unknown"
            } else {
                "missing"
            };
            sqlx::query("UPDATE embedding_items SET state=? WHERE logical=? AND ordinal=?")
                .bind(next)
                .bind(id)
                .bind(i64::try_from(i)?)
                .execute(&mut *conn)
                .await?;
        }
    }
    let state = if matches!(http, Outcome::Unknown) {
        "unknown"
    } else if stored == row.get::<i64, _>("expected") {
        "done"
    } else if raw {
        "partial"
    } else {
        "failed"
    };
    sqlx::query("UPDATE embedding_requests SET state=? WHERE logical=?")
        .bind(state)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE executions SET state=? WHERE id=?")
        .bind(if state == "done" { "completed" } else { "held" })
        .bind(exec)
        .execute(&mut *conn)
        .await?;
    Ok(
        json!({"logical":id,"state":state,"stored":stored,"vector":if query && stored==1 {values[0].as_ref()}else{None},"profile":p.identity()}),
    )
}
pub async fn init(conn: &mut SqliteConnection) -> Result<Value> {
    // This table and trigger live only on the owner connection, and disappear on shutdown.
    for sql in [
        "CREATE TEMP TABLE embedding_cache(key TEXT PRIMARY KEY,vector TEXT NOT NULL,expires INTEGER NOT NULL)",
        "CREATE TEMP TRIGGER forget_embedding_cache AFTER INSERT ON main.invalidated_sources BEGIN DELETE FROM embedding_cache; END",
        "UPDATE embedding_items SET state='unknown' WHERE state='pending' AND logical IN(SELECT logical FROM model_attempts WHERE state='unknown')",
        "UPDATE embedding_requests SET state='unknown' WHERE state='prepared' AND logical IN(SELECT logical FROM model_attempts WHERE state='unknown')",
    ] {
        sqlx::query(sql).execute(&mut *conn).await?;
    }
    Ok(Value::Null)
}
