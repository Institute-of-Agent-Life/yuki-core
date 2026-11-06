use super::{Change, content, model, scope};
use crate::{Result, identity::new_id, memory};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};

pub(super) fn text<'a>(params: &'a Value, key: &str) -> Result<&'a str> {
    params[key]
        .as_str()
        .ok_or_else(|| format!("session {key} required").into())
}
/// Match v4's Python str.strip for instructions and visible session input.
pub(super) fn material<'a>(params: &'a Value, key: &str) -> Result<&'a str> {
    let value = text(params, key)?
        .trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c));
    if value.is_empty() {
        return Err(format!("nonempty session {key} required").into());
    }
    Ok(value)
}

pub(super) fn digest(c: &Change) -> String {
    let mut params = c.params.clone();
    if let Some(object) = params.as_object_mut() {
        object.remove("authority");
    }
    memory::scope::hash(&json!([c.method, params]))
}
pub(super) async fn fresh(
    db: &mut SqliteConnection,
    c: &Change,
    s: &sqlx::sqlite::SqliteRow,
) -> Result<scope::Authority> {
    scope::resolve(
        db,
        c,
        s.get("route"),
        s.get("purpose"),
        &c.params["authority"],
        true,
    )
    .await
}
pub(super) async fn original(db: &mut SqliteConnection, c: &Change) -> Result<Option<Value>> {
    let model_key:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_session_turns WHERE plugin=? AND session=? AND original_key=?)").bind(&c.plugin).bind(c.params["session"].as_str()).bind(text(&c.params,"key")?).fetch_one(&mut *db).await?;
    if model_key && c.method != "session.run" {
        return Err("original session key belongs to a model turn".into());
    }
    let prior:Option<(String,String,String)>=sqlx::query_as("SELECT method,digest,result FROM plugin_session_operations WHERE plugin=? AND session=? AND original_key=?")
        .bind(&c.plugin).bind(c.params["session"].as_str()).bind(text(&c.params,"key")?).fetch_optional(db).await?;
    if let Some((method, hash, result)) = prior {
        if method != c.method || hash != digest(c) {
            return Err("original session operation conflicts".into());
        }
        return Ok(Some(serde_json::from_str(&result)?));
    }
    Ok(None)
}
pub(super) async fn record(db: &mut SqliteConnection, c: &Change, result: &Value) -> Result<()> {
    sqlx::query("INSERT INTO plugin_session_operations VALUES(?,?,?,?,?,?)")
        .bind(&c.plugin)
        .bind(c.params["session"].as_str())
        .bind(text(&c.params, "key")?)
        .bind(&c.method)
        .bind(digest(c))
        .bind(result.to_string())
        .execute(db)
        .await?;
    Ok(())
}
pub(super) async fn reload(
    db: &mut SqliteConnection,
    plugin: &str,
    id: &str,
) -> Result<sqlx::sqlite::SqliteRow> {
    Ok(
        sqlx::query("SELECT * FROM plugin_sessions WHERE plugin=? AND id=?")
            .bind(plugin)
            .bind(id)
            .fetch_one(db)
            .await?,
    )
}
pub(super) async fn create(db: &mut SqliteConnection, c: &Change) -> Result<Value> {
    let key = text(&c.params, "key")?;
    let route = text(&c.params, "route")?;
    let purpose = text(&c.params, "purpose")?;
    let hash = digest(c);
    let authority = scope::resolve(db, c, route, purpose, &c.params["authority"], false).await?;
    let context = text(&c.params, "context_profile")?;
    scope::context(&authority, context)?;
    if let Some((id, prior)) = sqlx::query_as::<_, (String, String)>(
        "SELECT id,created_hash FROM plugin_sessions WHERE plugin=? AND created_key=?",
    )
    .bind(&c.plugin)
    .bind(key)
    .fetch_optional(&mut *db)
    .await?
    {
        if prior != hash {
            return Err("original session creation conflicts".into());
        }
        let mut reader = c.clone();
        reader.params["session"] = json!(id);
        let (session, _) = scope::current(db, &reader, false).await?;
        content::active(&session)?;
        return Ok(content::summary(&session));
    }
    let authority = scope::resolve(db, c, route, purpose, &c.params["authority"], true).await?;
    let id = if let Some(id) = c.params["session"].as_str() {
        id.to_owned()
    } else {
        new_id(db).await?
    };
    let boot = scope::boot(db).await?;
    sqlx::query("INSERT INTO plugin_sessions(plugin,id,created_key,created_hash,revision,scope_kind,person,entry,space,name,instructions,route,purpose,persistence,boot,context_profile) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(&c.plugin).bind(&id).bind(key).bind(hash).bind(&authority.revision).bind(authority.kind()).bind(&authority.actor).bind(&authority.entry).bind(&authority.space).bind(text(&c.params,"name")?).bind(material(&c.params,"instructions")?).bind(route).bind(purpose).bind(text(&c.params,"persistence")?).bind(boot).bind(context).execute(&mut *db).await?;
    scope::bind(db, c, &id, &authority).await?;
    Ok(content::summary(&reload(db, &c.plugin, &id).await?))
}
pub(super) async fn message(
    db: &mut SqliteConnection,
    c: &Change,
    s: &sqlx::sqlite::SqliteRow,
    role: &str,
    sender: Option<&str>,
    body: &str,
    turn: Option<&str>,
) -> Result<i64> {
    let plugin = &c.plugin;
    let id: &str = s.get("id");
    let epoch: i64 = s.get("epoch");
    let sequence:i64=sqlx::query_scalar("UPDATE plugin_sessions SET next_sequence=next_sequence+1,updated_at=unixepoch() WHERE plugin=? AND id=? AND epoch=? AND state='active' RETURNING next_sequence-1")
        .bind(plugin).bind(id).bind(epoch).fetch_one(&mut *db).await?;
    let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
    sqlx::query("INSERT INTO plugin_session_messages VALUES(?,?,?,?,?,?,?,?,?)")
        .bind(plugin)
        .bind(id)
        .bind(sequence)
        .bind(epoch)
        .bind(role)
        .bind(sender)
        .bind(body)
        .bind(digest)
        .bind(turn)
        .execute(db)
        .await?;
    Ok(sequence)
}
pub(super) async fn idle(db: &mut SqliteConnection, plugin: &str, id: &str) -> Result<()> {
    let pending:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_session_turns WHERE plugin=? AND session=? AND state='prepared')").bind(plugin).bind(id).fetch_one(db).await?;
    if pending {
        return Err("original isolated turn is still pending".into());
    }
    Ok(())
}
pub(super) async fn append(
    db: &mut SqliteConnection,
    c: &Change,
    s: &sqlx::sqlite::SqliteRow,
) -> Result<Value> {
    content::active(s)?;
    if let Some(original) = original(db, c).await? {
        return Ok(original);
    }
    content::epoch(c, s)?;
    let authority = fresh(db, c, s).await?;
    let id: &str = s.get("id");
    idle(db, &c.plugin, id).await?;
    scope::bind(db, c, id, &authority).await?;
    let sequence = message(
        db,
        c,
        s,
        "user",
        authority.actor.as_deref(),
        text(&c.params, "text")?,
        None,
    )
    .await?;
    let result = json!({"session":id,"sequence":sequence,"epoch":s.get::<i64,_>("epoch")});
    record(db, c, &result).await?;
    Ok(result)
}
pub(super) async fn reset(
    db: &mut SqliteConnection,
    c: &Change,
    s: &sqlx::sqlite::SqliteRow,
) -> Result<Value> {
    if let Some(original) = original(db, c).await? {
        return Ok(original);
    }
    content::epoch(c, s)?;
    fresh(db, c, s).await?;
    let id: &str = s.get("id");
    let reset = c.method == "session.reset";
    model::invalidate(
        db,
        &c.plugin,
        id,
        if reset {
            "session_reset"
        } else {
            "session_closed"
        },
        reset,
    )
    .await?;
    if reset {
        sqlx::query("UPDATE plugin_session_messages SET body=NULL WHERE plugin=? AND session=?")
            .bind(&c.plugin)
            .bind(id)
            .execute(&mut *db)
            .await?;
    }
    sqlx::query("UPDATE plugin_sessions SET epoch=epoch+1,state=?,updated_at=unixepoch() WHERE plugin=? AND id=?")
        .bind(if reset{"active"}else{"closed"}).bind(&c.plugin).bind(id).execute(&mut *db).await?;
    let result = content::summary(&reload(db, &c.plugin, id).await?);
    record(db, c, &result).await?;
    Ok(result)
}
