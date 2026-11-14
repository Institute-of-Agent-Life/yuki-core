mod legacy;
mod ordered;
use crate::{Result, identity::new_id, runtime::effects};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};

pub const DEFAULT_PAGE: usize = 8000;
pub const DEFAULT_QUOTA: i64 = 64 * 1024 * 1024;
fn size(v: &Value) -> usize {
    v.to_string().chars().count()
}
fn version(raw: &str) -> String {
    format!("{:x}", Sha256::digest(raw.as_bytes()))
}

pub(crate) async fn retain(
    conn: &mut SqliteConnection,
    exec: &str,
    op: &str,
    raw: &str,
    quota: i64,
) -> Result<Option<(String, String)>> {
    let hash = version(raw);
    let original: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT handle,version,body FROM tool_artifacts WHERE execution=? AND operation=?",
    )
    .bind(exec)
    .bind(op)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some((handle, previous, body)) = original {
        if previous != hash || body.as_deref() != Some(raw) {
            return Err("original tool artifact changed or was erased".into());
        }
        return Ok(Some((handle, hash)));
    }
    let bytes = i64::try_from(raw.len())?;
    let used: i64 = sqlx::query_scalar(
        "SELECT coalesce(sum(length(CAST(body AS BLOB))),0) FROM tool_artifacts",
    )
    .fetch_one(&mut *conn)
    .await?;
    if quota.saturating_sub(used) < bytes {
        return Ok(None);
    }
    let handle = new_id(conn).await?;
    sqlx::query("INSERT INTO tool_artifacts VALUES(?,?,?,?,?)")
        .bind(&handle)
        .bind(&hash)
        .bind(exec)
        .bind(op)
        .bind(raw)
        .execute(&mut *conn)
        .await?;
    Ok(Some((handle, hash)))
}

// The limit includes the complete JSON model receipt. A stored array length describes only
// that artifact; an original query's total and next_cursor have independent meanings.
pub async fn present(
    conn: &mut SqliteConnection,
    exec: &str,
    op: &str,
    original: Value,
    limit: usize,
    quota: i64,
) -> Result<Value> {
    if size(&original) <= limit {
        return Ok(original);
    }
    let raw = original.to_string();
    let artifact = retain(conn, exec, op, &raw, quota).await?;
    let cursor = original["result"].get("next_cursor").cloned();
    let mut out = json!({"id":op,"state":original["state"],"result":{"truncated":true,"reason":if artifact.is_some(){"size"}else{"storage_full"},"total":original["result"].get("total")}});
    if let Some(cursor) = cursor {
        out["result"]["next_cursor"] = cursor;
    }
    if let Some((h, hash)) = artifact {
        out["result"]["artifact"] = json!({"handle":h,"version":hash,"path":"/result","offset":0});
        out["result"]["read_more"] = json!({"handle":h,"operation":"get","path":[],"offset":0});
    }
    let text = original.get("result").unwrap_or(&original).to_string();
    out["result"]["preview"] = json!("");
    let chars: Vec<_> = text.chars().take(limit).collect();
    let mut low = 0;
    let mut high = chars.len();
    while low < high {
        let mid = (low + high).div_ceil(2);
        out["result"]["preview"] = json!(chars[..mid].iter().collect::<String>());
        if size(&out) <= limit {
            low = mid
        } else {
            high = mid - 1;
        }
    }
    out["result"]["preview"] = json!(chars[..low].iter().collect::<String>());
    if size(&out) > limit {
        return Err("tool result budget cannot hold continuation metadata".into());
    }
    Ok(out)
}
pub async fn authorize(
    conn: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    handle: &str,
    hash: &str,
) -> Result<String> {
    effects::authorize(conn, exec, actor, false).await?;
    let owner: String = sqlx::query_scalar(
        "SELECT execution FROM tool_artifacts WHERE handle=? AND version=? AND body IS NOT NULL",
    )
    .bind(handle)
    .bind(hash)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or("artifact unavailable")?;
    effects::authorize(conn, &owner, actor, false).await?;
    crate::operator::domain::trace::authorize_artifact(conn, handle, actor).await?;
    if owner != exec {
        let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM executions x JOIN executions y ON y.id=? LEFT JOIN events e ON e.id=x.source LEFT JOIN events f ON f.id=y.source
            WHERE x.id=? AND x.work_id IS y.work_id AND ((x.origin IS NOT NULL AND x.origin=y.origin) OR (e.entry_id=f.entry_id AND x.original_actor=y.original_actor)))")
            .bind(exec).bind(&owner).fetch_one(conn).await?;
        if !allowed {
            return Err("artifact unavailable in current execution scope".into());
        }
    }
    Ok(owner)
}
pub struct Snapshot {
    pub raw: String,
    pub hash: String,
    pub tool: String,
}
impl Snapshot {
    pub fn page(&self, args: &Value, budget: usize) -> Result<Value> {
        let mut args = args.clone();
        args["_tool_name"] = json!(self.tool);
        page(&self.raw, &args, budget)
    }
}
pub async fn load(
    conn: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    args: &Value,
) -> Result<Snapshot> {
    let handle = args["handle"].as_str().ok_or("missing handle")?;
    // Resolve once in the same read transaction; the final writer checks this
    // exact version again after decoding, even when the original API omits it.
    let row = sqlx::query("SELECT a.body,a.version,o.target FROM tool_artifacts a JOIN operations o ON o.namespace=a.execution AND o.operation_id=a.operation WHERE a.handle=? AND a.body IS NOT NULL")
        .bind(handle).fetch_optional(&mut *conn).await?.ok_or("artifact unavailable")?;
    let hash: String = row.try_get("version")?;
    if args
        .get("version")
        .is_some_and(|requested| requested.as_str() != Some(&hash))
    {
        return Err("artifact version unavailable".into());
    }
    authorize(conn, exec, actor, handle, &hash).await?;
    let raw: String = row.try_get("body")?;
    if version(&raw) != hash {
        return Err("artifact integrity check failed".into());
    }
    Ok(Snapshot {
        raw,
        hash,
        tool: row.try_get("target")?,
    })
}
pub fn page(raw: &str, args: &Value, budget: usize) -> Result<Value> {
    if !args["version"].is_string() {
        return legacy::page(raw, args, budget);
    }
    let root: Value = serde_json::from_str(raw)?;
    let path = args["path"].as_str().unwrap_or("/result");
    let value = root.pointer(path).ok_or("artifact path does not exist")?;
    let offset = usize::try_from(args["offset"].as_u64().unwrap_or(0))?;
    let limit = usize::try_from(args["limit"].as_u64().unwrap_or(DEFAULT_PAGE as u64))?
        .clamp(1, DEFAULT_PAGE);
    let (total, unit) = match value {
        Value::String(s) => (s.chars().count(), "characters"),
        Value::Array(a) => (a.len(), "items"),
        _ => (1, "value"),
    };
    if offset > total {
        return Err("offset beyond artifact length".into());
    }
    let make = |length: usize| -> Value {
        let next = offset + length;
        let part = match value {
            Value::String(s) => json!(s.chars().skip(offset).take(length).collect::<String>()),
            Value::Array(a) => json!(a[offset..next]),
            _ if length > 0 => value.clone(),
            _ => Value::Null,
        };
        json!({"handle":args["handle"],"version":args["version"],"path":path,"offset":offset,"offset_unit":unit,"length":length,"total":total,"next_offset":(next<total).then_some(next),"truncated":next<total,"reason":if next==total{None}else if length==0{Some("item_exceeds_budget")}else{Some("page_limit")},"value":part})
    };
    let mut low = 0;
    let mut high = limit.min(total - offset);
    while low < high {
        let n = (low + high).div_ceil(2);
        if size(&make(n)) <= budget {
            low = n;
        } else {
            high = n - 1;
        }
    }
    let out = make(low);
    if size(&out) > budget {
        return Err("page metadata exceeds tool result budget".into());
    }
    Ok(out)
}

/// Operator compatibility also accepts cursors issued before the original v4 API was restored.
pub fn operator_schema() -> Value {
    json!({"oneOf":[super::registry::builtin("read_tool_artifact").unwrap().schema,
        {"type":"object","properties":{"handle":{"type":"string"},"version":{"type":"string"},"path":{"type":"string"},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":8000}},"required":["handle","version"],"additionalProperties":false}]})
}
