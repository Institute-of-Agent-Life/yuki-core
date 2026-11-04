use super::{Change, index, normalize};
use crate::{
    Result,
    runtime::effects,
    store::{Mutation, Store},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{QueryBuilder, Row, Sqlite, SqliteConnection};
use std::collections::BTreeSet;

#[derive(Default, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    #[default]
    Recent,
    Search,
    Around,
}
#[derive(Default, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    #[default]
    All,
    Any,
    Phrase,
}
#[derive(Default, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Order {
    #[default]
    Recent,
    Oldest,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Query {
    kind: Kind,
    text: String,
    mode: Mode,
    exclude: Vec<String>,
    entries: Option<Vec<String>>,
    limit: usize,
    cursor: Option<i64>,
    anchor: Option<i64>,
    source_event: Option<i64>,
    anchor_operation: Option<String>,
    order: Order,
    author: Option<String>,
    start_ms: Option<i64>,
    end_ms: Option<i64>,
    attachment: Option<String>,
}
impl Default for Query {
    fn default() -> Self {
        Self {
            kind: Kind::Recent,
            text: String::new(),
            mode: Mode::All,
            exclude: vec![],
            entries: None,
            limit: 20,
            cursor: None,
            anchor: None,
            source_event: None,
            anchor_operation: None,
            order: Order::Recent,
            author: None,
            start_ms: None,
            end_ms: None,
            attachment: None,
        }
    }
}
pub(crate) struct Scope {
    pub(crate) home: String,
    pub(crate) entries: Vec<String>,
    pub(crate) subject: String,
}
pub(crate) async fn scope(conn: &mut SqliteConnection, exec: &str, actor: &str) -> Result<Scope> {
    effects::authorize(conn, exec, actor, false).await?;
    let home=sqlx::query("WITH RECURSIVE lineage(id) AS(SELECT ? UNION SELECT o.parent FROM lineage l JOIN executions x ON x.id=l.id JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL)
      SELECT e.raw_actor,e.entry_id,x.id FROM lineage l JOIN executions x ON x.id=l.id JOIN events e ON e.id=x.source")
      .bind(exec).fetch_optional(&mut *conn).await?.ok_or("no delegated human history sources")?;
    let subject: String = home.try_get("raw_actor")?;
    let entry: String = home.try_get("entry_id")?;
    let delegated = home.try_get::<String, _>("id")? != exec;
    for_person(conn, &subject, &entry, delegated).await
}
pub(crate) async fn for_person(
    conn: &mut SqliteConnection,
    subject: &str,
    entry: &str,
    delegated: bool,
) -> Result<Scope> {
    let entries:Vec<String>=sqlx::query_scalar("SELECT d.id FROM entries d JOIN persons who ON who.id=?
      JOIN persons root ON root.id=coalesce(who.current_root,who.id) WHERE who.enabled AND root.enabled
      AND (NOT ? OR d.id=?) AND (
        (d.scope='private' AND EXISTS(SELECT 1 FROM assignments a JOIN persons p ON p.id=a.original_person WHERE a.id=d.actor_stage AND p.enabled AND coalesce(p.current_root,p.id)=root.id))
        OR (d.scope='group' AND EXISTS(SELECT 1 FROM events e JOIN persons p ON p.id=e.raw_actor WHERE e.entry_id=d.id AND p.enabled AND coalesce(p.current_root,p.id)=root.id
           AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=e.id) AND json_extract(e.envelope,'$.kind')='message'))) ORDER BY d.id")
      .bind(subject).bind(delegated).bind(entry).fetch_all(conn).await?;
    Ok(Scope {
        home: entry.to_owned(),
        entries,
        subject: subject.to_owned(),
    })
}

pub(crate) async fn directory(
    conn: &mut SqliteConnection,
    exec: &str,
    actor: &str,
) -> Result<Value> {
    let s = scope(conn, exec, actor).await?;
    let ids = serde_json::to_string(&s.entries)?;
    let entries=sqlx::query("SELECT id,scope,topic FROM entries WHERE id IN(SELECT value FROM json_each(?)) ORDER BY id")
        .bind(&ids).fetch_all(&mut *conn).await?.into_iter().map(|r|Ok(json!({"entry":r.try_get::<String,_>("id")?,"scope":r.try_get::<String,_>("scope")?,"thread":r.try_get::<Option<String>,_>("topic")?}))).collect::<Result<Vec<_>>>()?;
    let subjects=sqlx::query("SELECT p.id,p.name FROM persons p WHERE p.enabled AND (p.id=? OR EXISTS(SELECT 1 FROM history_items h
        WHERE h.actor=p.id AND h.visible AND h.entry IN(SELECT value FROM json_each(?)))) ORDER BY p.id")
        .bind(s.subject).bind(ids).fetch_all(conn).await?.into_iter().map(|r|Ok(json!({"person":r.try_get::<String,_>("id")?,"name":r.try_get::<String,_>("name")?}))).collect::<Result<Vec<_>>>()?;
    Ok(json!({"entries":entries,"subjects":subjects,"group_coverage":"observed"}))
}

struct Matcher {
    terms: Vec<normalize::Field>,
    exclude: Vec<normalize::Field>,
    mode: Mode,
}
impl Matcher {
    fn new(q: &Query) -> Result<Self> {
        let folded = normalize::fold(&q.text);
        let words: Vec<_> = match q.mode {
            Mode::Phrase if !folded.is_empty() => vec![folded.as_str()],
            _ => folded.split_whitespace().collect(),
        };
        let mut seen = BTreeSet::new();
        let terms = words
            .into_iter()
            .filter(|w| seen.insert(*w))
            .map(|w| normalize::field(String::new(), w.into()))
            .collect::<Result<_>>()?;
        let exclude = q
            .exclude
            .iter()
            .map(|w| normalize::field(String::new(), w.clone()))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|f| !f.base.is_empty())
            .collect();
        Ok(Self {
            terms,
            exclude,
            mode: q.mode,
        })
    }
    fn fts(&self, entries: &[String]) -> Option<String> {
        if self.terms.is_empty() || entries.is_empty() {
            return None;
        }
        let groups: Vec<_> = entries
            .iter()
            .map(|entry| {
                let prefix = index::prefix(entry);
                let terms: Vec<_> = self
                    .terms
                    .iter()
                    .map(|t| {
                        let alternatives: BTreeSet<_> = [&t.base, &t.simple]
                            .into_iter()
                            .filter(|s| !s.is_empty())
                            .map(|s| {
                                let scoped = normalize::candidates(s)
                                    .into_iter()
                                    .map(|g| format!("{prefix}{g}"))
                                    .collect::<Vec<_>>();
                                format!("({})", scoped.join(" AND "))
                            })
                            .collect();
                        format!(
                            "({})",
                            alternatives.into_iter().collect::<Vec<_>>().join(" OR ")
                        )
                    })
                    .collect();
                format!(
                    "({})",
                    terms.join(if matches!(self.mode, Mode::Any) {
                        " OR "
                    } else {
                        " AND "
                    })
                )
            })
            .collect();
        Some(groups.join(" OR "))
    }
    fn matched(&self, fields: &[(String, String, String, String)]) -> Option<Vec<String>> {
        let hit = |t: &normalize::Field, f: &(String, String, String, String)| {
            f.2.contains(&t.base) || f.3.contains(&t.simple)
        };
        if self
            .exclude
            .iter()
            .any(|t| fields.iter().any(|f| hit(t, f)))
        {
            return None;
        }
        let mut labels = Vec::new();
        let mut n = 0;
        for t in &self.terms {
            let mut found = false;
            for f in fields {
                if hit(t, f) {
                    found = true;
                    if !labels.contains(&f.0) {
                        labels.push(f.0.clone());
                    }
                }
            }
            n += usize::from(found);
        }
        if self.terms.is_empty() || matches!(self.mode, Mode::Any) && n > 0 || n == self.terms.len()
        {
            Some(labels)
        } else {
            None
        }
    }
}
async fn page(
    conn: &mut SqliteConnection,
    query: &Query,
    entries: &[String],
    cutoff: i64,
    matcher: &Matcher,
) -> Result<(Vec<Value>, Option<i64>)> {
    if entries.is_empty() {
        return Ok((vec![], None));
    }
    let desc = matches!(query.order, Order::Recent);
    let mut cursor = query.cursor;
    let fts = matcher.fts(entries);
    let mut result = Vec::new();
    while result.len() <= query.limit {
        let mut sql=QueryBuilder::<Sqlite>::new("SELECT h.id,h.entry,h.source_event,h.outbound,h.actor,h.kind,e.occurred_at,e.observed_at,o.observed_at AS sent_at,o.original_actor AS requested_by,o.stage AS sender_stage
          FROM history_items h LEFT JOIN events e ON e.id=h.source_event LEFT JOIN social_outbound o ON o.id=h.outbound WHERE h.visible AND h.entry IN (SELECT value FROM json_each(");
        sql.push_bind(serde_json::to_string(entries)?)
            .push(")) AND h.id<=")
            .push_bind(cutoff)
            .push(" AND NOT EXISTS(SELECT 1 FROM main_controls c WHERE c.event=h.source_event)");
        if let Some(cursor) = cursor {
            sql.push(if desc { " AND h.id<" } else { " AND h.id>" })
                .push_bind(cursor);
        }
        if let Some(expression) = &fts {
            sql.push(" AND h.id IN(SELECT rowid FROM history_fts WHERE history_fts MATCH ")
                .push_bind(expression)
                .push(")");
        }
        if let Some(author) = &query.author {
            sql.push(" AND h.actor=").push_bind(author);
        }
        if let Some(t) = query.start_ms {
            sql.push(" AND coalesce(e.occurred_at,e.observed_at,o.observed_at)>=")
                .push_bind(t);
        }
        if let Some(t) = query.end_ms {
            sql.push(" AND coalesce(e.occurred_at,e.observed_at,o.observed_at)<")
                .push_bind(t);
        }
        if let Some(kind) = &query.attachment {
            let accepted = if kind == "audio" {
                json!(["audio", "record"])
            } else {
                json!([kind])
            }
            .to_string();
            sql.push(" AND (EXISTS(SELECT 1 FROM json_each(e.envelope,'$.segments') seg WHERE json_extract(seg.value,'$.kind') IN(SELECT value FROM json_each(")
              .push_bind(accepted.clone()).push("))) OR EXISTS(SELECT 1 FROM json_each(o.body,'$.message') seg WHERE json_extract(seg.value,'$.type') IN(SELECT value FROM json_each(")
              .push_bind(accepted).push("))) OR (").push_bind(kind).push("='file' AND json_type(o.body,'$.file') IS NOT NULL))");
        }
        sql.push(if desc {
            " ORDER BY h.id DESC LIMIT 64"
        } else {
            " ORDER BY h.id ASC LIMIT 64"
        });
        let rows = sql.build().fetch_all(&mut *conn).await?;
        let n = rows.len();
        for r in rows {
            let id = r.try_get("id")?;
            cursor = Some(id);
            let fields:Vec<(String,String,String,String)>=sqlx::query_as("SELECT label,raw,base,simplified FROM history_fields WHERE item=? ORDER BY CASE WHEN label='content' THEN -1 ELSE CAST(substr(label,18) AS INTEGER) END")
                .bind(id).fetch_all(&mut *conn).await?;
            if let Some(labels) = matcher.matched(&fields) {
                result.push(json!({"id":id,"source_event":r.try_get::<Option<i64>,_>("source_event")?,"outbound":r.try_get::<Option<i64>,_>("outbound")?,
                    "entry":r.try_get::<String,_>("entry")?,"actor":r.try_get::<Option<String>,_>("actor")?,"kind":r.try_get::<String,_>("kind")?,"requested_by":r.try_get::<Option<String>,_>("requested_by")?,"sender_stage":r.try_get::<Option<String>,_>("sender_stage")?,
                    "at":r.try_get::<Option<i64>,_>("occurred_at")?.or(r.try_get("observed_at")?).or(r.try_get("sent_at")?),
                    "status":"visible","fields":fields.into_iter().map(|(label,raw,_,_)|json!({"label":label,"text":raw})).collect::<Vec<_>>(),"matched_fields":labels}));
                if result.len() > query.limit {
                    break;
                }
            }
        }
        if n < 64 {
            break;
        }
    }
    let next = if result.len() > query.limit {
        result.pop();
        result.last().and_then(|r| r["id"].as_i64())
    } else {
        None
    };
    Ok((result, next))
}

async fn search(
    conn: &mut SqliteConnection,
    exec: &str,
    s: &Scope,
    q: &Query,
    cutoff: i64,
) -> Result<(Vec<Value>, Option<i64>)> {
    let mut entries = s.entries.clone();
    if let Some(requested) = &q.entries {
        entries.retain(|e| requested.contains(e));
    } else if matches!(q.kind, Kind::Recent) {
        entries.retain(|e| e == &s.home);
    }
    let matcher = Matcher::new(q)?;
    if !matches!(q.kind, Kind::Around) {
        return page(conn, q, &entries, cutoff, &matcher).await;
    }
    let target = if let Some(key) = &q.anchor_operation {
        sqlx::query_scalar("SELECT CAST(json_extract(items,'$[0]') AS INTEGER) FROM history_anchors WHERE execution=? AND operation=?").bind(exec).bind(key).fetch_optional(&mut *conn).await?.flatten()
    } else if let Some(event) = q.source_event {
        sqlx::query_scalar("SELECT id FROM history_items WHERE source_event=?")
            .bind(event)
            .fetch_optional(&mut *conn)
            .await?
    } else {
        q.anchor
    };
    let Some(target) = target else {
        return Err("around requires an original anchor".into());
    };
    let found:Option<String>=sqlx::query_scalar("SELECT entry FROM history_items WHERE id=? AND id<=? AND visible AND entry IN(SELECT value FROM json_each(?))")
        .bind(target).bind(cutoff).bind(serde_json::to_string(&entries)?).fetch_optional(&mut *conn).await?;
    let Some(entry) = found else {
        return Ok((vec![json!({"id":target,"status":"missing"})], None));
    };
    let mut before = q.clone();
    before.order = Order::Recent;
    before.cursor = Some(target);
    before.limit = (q.limit - 1) / 2;
    let (mut rows, _) = page(
        conn,
        &before,
        std::slice::from_ref(&entry),
        cutoff,
        &matcher,
    )
    .await?;
    rows.reverse();
    let mut after = q.clone();
    after.order = Order::Oldest;
    after.cursor = Some(target - 1);
    after.limit = q.limit - rows.len();
    rows.extend(page(conn, &after, &[entry], cutoff, &matcher).await?.0);
    Ok((rows, None))
}

pub(super) async fn anchor(
    conn: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    key: &str,
    args: &str,
    rows: &[Value],
    cursor: Option<i64>,
) -> Result<Value> {
    let s = scope(conn, exec, actor).await?;
    if let Some(old) = sqlx::query_scalar::<_, String>(
        "SELECT query FROM history_anchors WHERE execution=? AND operation=?",
    )
    .bind(exec)
    .bind(key)
    .fetch_optional(&mut *conn)
    .await?
    {
        if old != args {
            return Err("original history operation has different arguments".into());
        }
        return Ok(json!({"operation":key}));
    }
    let original = sqlx::query(
        "SELECT action,target,payload,state FROM operations WHERE namespace=? AND operation_id=?",
    )
    .bind(exec)
    .bind(key)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(row) = original {
        let raw: String = row.try_get("payload")?;
        let original: Query = serde_json::from_str(&raw)?;
        if row.try_get::<String, _>("action")? != "external"
            || row.try_get::<String, _>("target")? != "history_query"
            || row.try_get::<String, _>("state")? != "intent"
            || serde_json::to_string(&original)? != args
        {
            return Err("history receipt conflicts with existing operation".into());
        }
    } else {
        effects::apply(
            conn,
            &effects::Action::Intent {
                execution: exec.into(),
                id: key.into(),
                actor: actor.into(),
                action: effects::Operation::External,
                target: "history_query".into(),
                payload: args.into(),
                parent: None,
                ordinal: None,
            },
        )
        .await?;
    }
    let ids: Vec<i64> = rows
        .iter()
        .map(|r| r["id"].as_i64().ok_or("missing history item ID"))
        .collect::<std::result::Result<_, _>>()?;
    let ids_json = serde_json::to_string(&ids)?;
    sqlx::query("INSERT INTO history_anchors VALUES(?,?,?,?,?)")
        .bind(exec)
        .bind(key)
        .bind(args)
        .bind(&ids_json)
        .bind(cursor)
        .execute(&mut *conn)
        .await?;
    let scope_json = serde_json::to_string(&s.entries)?;
    for (id, row) in ids.iter().zip(rows) {
        let visible:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM history_items WHERE id=? AND visible AND entry IN(SELECT value FROM json_each(?)))")
            .bind(id).bind(&scope_json).fetch_one(&mut *conn).await?;
        // A missing anchor has no material. A guessed numeric around-anchor may never have existed.
        if row["status"] == "visible" && visible {
            sqlx::query("INSERT INTO history_anchor_rows VALUES(?,?,?,?)")
                .bind(exec)
                .bind(key)
                .bind(id)
                .bind(row.to_string())
                .execute(&mut *conn)
                .await?;
        }
    }
    let result = json!({"operation":key,"items":ids,"cursor":cursor});
    sqlx::query("UPDATE operations SET state='confirmed',result=? WHERE namespace=? AND operation_id=? AND state='intent'")
        .bind(result.to_string()).bind(exec).bind(key).execute(conn).await?;
    Ok(result)
}

async fn replay(
    conn: &mut SqliteConnection,
    exec: &str,
    key: &str,
    args: &str,
    s: &Scope,
) -> Result<Option<Value>> {
    let Some(old) = sqlx::query(
        "SELECT query,items,cursor FROM history_anchors WHERE execution=? AND operation=?",
    )
    .bind(exec)
    .bind(key)
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(None);
    };
    if old.try_get::<String, _>("query")? != args {
        return Err("original history operation argument conflict".into());
    }
    let ids: Vec<i64> = serde_json::from_str(old.try_get("items")?)?;
    let allowed = serde_json::to_string(&s.entries)?;
    let mut rows = Vec::new();
    for id in ids {
        let material:Option<String>=sqlx::query_scalar("SELECT snap.projection FROM history_anchor_rows snap JOIN history_items h ON h.id=snap.item
          WHERE snap.execution=? AND snap.operation=? AND snap.item=? AND h.visible AND h.entry IN(SELECT value FROM json_each(?))")
          .bind(exec).bind(key).bind(id).bind(&allowed).fetch_optional(&mut *conn).await?.flatten();
        rows.push(match material {
            Some(raw) => serde_json::from_str::<Value>(&raw)?,
            None => json!({"id":id,"status":"missing"}),
        });
    }
    Ok(Some(
        json!({"items":rows,"next_cursor":old.try_get::<Option<i64>,_>("cursor")?,"operation":key}),
    ))
}

pub async fn run(
    store: &Store,
    exec: &str,
    actor: &str,
    args: &str,
    operation: Option<&str>,
) -> Result<Value> {
    let q: Query = serde_json::from_str(args)?;
    if q.limit == 0
        || q.limit > 200
        || q.text.chars().count() > 512
        || q.exclude.len() > 32
        || q.entries.as_ref().is_some_and(|v| v.len() > 128)
    {
        return Err("history query outside supported limits".into());
    }
    let canonical = serde_json::to_string(&q)?;
    let cutoff: i64 = sqlx::query_scalar("SELECT coalesce(max(id),0) FROM history_items")
        .fetch_one(store.readers())
        .await?;
    loop {
        let mut tx = store.readers().begin().await?;
        let s = scope(&mut tx, exec, actor).await?;
        if let Some(key) = operation
            && let Some(saved) = replay(&mut tx, exec, key, &canonical, &s).await?
        {
            return Ok(saved);
        }
        let unread:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM history_items WHERE visible AND indexed!=revision AND id<=?)")
            .bind(cutoff).fetch_one(&mut *tx).await?;
        if unread {
            drop(tx);
            index::prepare_batch(store, cutoff).await?;
            continue;
        }
        let (rows, cursor) = search(&mut tx, exec, &s, &q, cutoff).await?;
        let Some(key) = operation else {
            return Ok(json!({"items":rows,"next_cursor":cursor}));
        };
        drop(tx);
        store
            .write(Mutation::History(Box::new(Change::Anchor {
                exec: exec.into(),
                actor: actor.into(),
                key: key.into(),
                args: canonical.clone(),
                rows,
                cursor,
            })))
            .await?;
    }
}

pub async fn scrub(conn: &mut SqliteConnection, event: i64) -> Result<()> {
    sqlx::query("UPDATE history_anchors SET query='{}' WHERE execution IN(SELECT id FROM executions WHERE source=?1 OR id IN(SELECT execution FROM main_dependencies WHERE event=?1))")
        .bind(event).execute(&mut *conn).await?;
    sqlx::query("UPDATE history_anchor_rows SET projection=NULL WHERE execution IN(SELECT id FROM executions WHERE source=?1 OR id IN(SELECT execution FROM main_dependencies WHERE event=?1))")
        .bind(event).execute(conn).await?;
    Ok(())
}

pub fn schema() -> Value {
    json!({"type":"object","additionalProperties":false,"properties":{
        "kind":{"enum":["recent","search","around"]},"text":{"type":"string","maxLength":512},"mode":{"enum":["all","any","phrase"]},
        "entries":{"type":"array","items":{"type":"string"},"maxItems":128},"exclude":{"type":"array","items":{"type":"string"},"maxItems":32},
        "limit":{"type":"integer","minimum":1,"maximum":200},"cursor":{"type":"integer"},"anchor":{"type":"integer"},
        "source_event":{"type":"integer"},"anchor_operation":{"type":"string"},"order":{"enum":["recent","oldest"]},
        "author":{"type":"string"},"start_ms":{"type":"integer"},"end_ms":{"type":"integer"},"attachment":{"type":"string"}}})
}
/// Pair only a still-readable original result. Every source whose body enters a paid continuation
/// becomes a dependency of that execution, so later erasure also scrubs the model transcript.
pub async fn material(
    conn: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    key: &str,
) -> Result<Value> {
    let s = scope(conn, exec, actor).await?;
    let args: String =
        sqlx::query_scalar("SELECT query FROM history_anchors WHERE execution=? AND operation=?")
            .bind(exec)
            .bind(key)
            .fetch_one(&mut *conn)
            .await?;
    let result = replay(conn, exec, key, &args, &s)
        .await?
        .ok_or("missing history anchor")?;
    for row in result["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["status"] == "visible")
    {
        let item = row["id"].as_i64().ok_or("missing original item")?;
        sqlx::query("INSERT OR IGNORE INTO main_dependencies(execution,event)
          SELECT ?1,h.source_event FROM history_items h WHERE h.id=?2 AND h.source_event IS NOT NULL
          UNION SELECT ?1,x.source FROM history_items h JOIN social_outbound o ON o.id=h.outbound JOIN executions x ON x.id=o.namespace WHERE h.id=?2 AND x.source IS NOT NULL
          UNION SELECT ?1,d.event FROM history_items h JOIN social_outbound o ON o.id=h.outbound JOIN main_dependencies d ON d.execution=o.namespace WHERE h.id=?2")
          .bind(exec).bind(item).execute(&mut *conn).await?;
    }
    Ok(json!({"namespace":exec,"id":key,"state":"confirmed","result":result}))
}
