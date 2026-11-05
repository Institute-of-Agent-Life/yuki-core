use crate::{
    Result,
    identity::{assignment, entry, new_id},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};
pub fn hash(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
pub fn fingerprint(class: &str, target: &str, payload: &Value) -> String {
    use std::fmt::Write;
    let mut value = json!([class, target, payload]);
    value.sort_all_objects();
    let mut ascii = String::new();
    for ch in value.to_string().chars() {
        if ch <= '~' {
            ascii.push(ch)
        } else {
            for word in ch.encode_utf16(&mut [0; 2]) {
                write!(ascii, "\\u{word:04x}").expect("string write");
            }
        }
    }
    hash(&ascii)
}
pub fn random<const N: usize>() -> Result<String> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| "OS credential generator unavailable")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
pub fn now() -> Result<i64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        .try_into()?)
}
pub fn stamp(t: i64) -> Result<String> {
    Ok(chrono::DateTime::from_timestamp(t, 0)
        .ok_or("invalid timestamp")?
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}
#[derive(clap::ValueEnum, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Purpose {
    Chat,
    Connector,
}
impl Purpose {
    pub fn name(self) -> &'static str {
        if self == Self::Chat {
            "chat"
        } else {
            "connector"
        }
    }
}
pub struct Principal {
    pub device: String,
    pub owner: String,
    pub root: String,
    pub admin: bool,
    pub entry: Option<String>,
}
pub async fn device(conn: &mut SqliteConnection, id: &str, purpose: Purpose) -> Result<Principal> {
    let row=sqlx::query("SELECT d.id,a.original_person,r.id AS root,r.admin,d.entry FROM personal_devices d JOIN device_origins a ON a.device_id=d.id JOIN persons p ON p.id=a.original_person AND p.enabled JOIN persons r ON r.id=coalesce(p.current_root,p.id) AND r.enabled LEFT JOIN entries e ON e.id=d.entry LEFT JOIN assignments h ON h.id=e.actor_stage LEFT JOIN assignments b ON b.id=e.bot_stage WHERE d.id=? AND d.revoked IS NULL AND d.purpose=? AND (d.entry IS NULL OR (e.closed_at IS NULL AND h.status='active' AND b.status='active' AND h.original_person=a.original_person))").bind(id).bind(purpose.name()).fetch_optional(conn).await?.ok_or("unauthenticated")?;
    Ok(Principal {
        device: row.get("id"),
        owner: row.get("original_person"),
        root: row.get("root"),
        admin: row.get("admin"),
        entry: row.get("entry"),
    })
}
pub async fn authenticate(
    conn: &mut SqliteConnection,
    hash: &str,
    purpose: Purpose,
) -> Result<Principal> {
    let id: String = sqlx::query_scalar(
        "SELECT id FROM personal_devices WHERE token_hash=? AND revoked IS NULL",
    )
    .bind(hash)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or("unauthenticated")?;
    device(conn, &id, purpose).await
}
pub fn uuid(key: &str) -> Result<()> {
    let bytes = key.as_bytes();
    if bytes.len() != 36
        || bytes[14] != b'4'
        || !matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
        || bytes.iter().enumerate().any(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                *b != b'-'
            } else {
                !b.is_ascii_hexdigit() || b.is_ascii_uppercase()
            }
        })
    {
        return Err("invalid_request_id".into());
    }
    Ok(())
}
pub async fn issue(
    conn: &mut SqliteConnection,
    issuer: &str,
    owner: &str,
    op: &str,
    purpose: Purpose,
    source: Option<&str>,
) -> Result<Value> {
    uuid(op)?;
    sqlx::query_scalar::<_,String>("SELECT p.id FROM persons p JOIN persons r ON r.id=coalesce(p.current_root,p.id) WHERE p.id=? AND p.enabled AND r.enabled").bind(owner).fetch_optional(&mut *conn).await?.ok_or("person_unavailable")?;
    if let Some(stage) = source {
        sqlx::query_scalar::<_, String>(
            "SELECT id FROM assignments WHERE id=? AND original_person=? AND status='active'",
        )
        .bind(stage)
        .bind(owner)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or("source_binding_unavailable")?;
    }
    if let Some(r) = sqlx::query("SELECT * FROM personal_codes WHERE issuer=? AND operation=?")
        .bind(issuer)
        .bind(op)
        .fetch_optional(&mut *conn)
        .await?
    {
        if r.get::<&str, _>("owner") != owner
            || r.get::<&str, _>("purpose") != purpose.name()
            || r.get::<Option<&str>, _>("source_stage") != source
        {
            return Err("request_conflict".into());
        }
        return Ok(
            json!({"code_id":r.get::<&str,_>("id"),"code":null,"expires_at":stamp(r.get("expires"))?,"purpose":purpose,"request_id":op,"replay":true}),
        );
    }
    let id = new_id(conn).await?;
    let raw = random::<24>()?;
    let expires = now()? + 600;
    sqlx::query("INSERT INTO personal_codes VALUES(?,?,?,?,?,?,?,?,NULL)")
        .bind(&id)
        .bind(issuer)
        .bind(op)
        .bind(owner)
        .bind(purpose.name())
        .bind(source)
        .bind(hash(&raw))
        .bind(expires)
        .execute(conn)
        .await?;
    Ok(
        json!({"code_id":id,"code":raw,"expires_at":stamp(expires)?,"purpose":purpose,"request_id":op,"replay":false}),
    )
}
async fn chat_entry(conn: &mut SqliteConnection, person: &str) -> Result<String> {
    async fn stage(
        conn: &mut SqliteConnection,
        native: &str,
        owner: Option<String>,
    ) -> Result<String> {
        let args = assignment::Action::Open {
            platform: "app".into(),
            namespace: "personal".into(),
            native: native.into(),
            observed: now()?,
            person: owner,
            start: None,
        };
        Ok(assignment::apply(conn, &args).await?["assignment"]
            .as_str()
            .ok_or("missing personal assignment")?
            .to_owned())
    }
    let who = stage(conn, person, Some(person.into())).await?;
    let bot = stage(conn, "yuki-app", None).await?;
    Ok(entry::apply(
        conn,
        &entry::Action::Private {
            actor: who,
            bot,
            topic: None,
        },
    )
    .await?["entry"]
        .as_str()
        .ok_or("missing personal entry")?
        .to_owned())
}
pub async fn pair(
    conn: &mut SqliteConnection,
    code: &str,
    label: &str,
    purpose: Purpose,
) -> Result<Value> {
    if label.trim().is_empty() || label.chars().count() > 128 {
        return Err("invalid_device_label".into());
    }
    let at = now()?;
    let owner:String=sqlx::query_scalar("UPDATE personal_codes SET used=?1 WHERE code_hash=?2 AND used IS NULL AND expires>?1 AND purpose=?3 AND EXISTS(SELECT 1 FROM persons p JOIN persons r ON r.id=coalesce(p.current_root,p.id) AND r.enabled WHERE p.id=personal_codes.owner AND p.enabled) AND (source_stage IS NULL OR EXISTS(SELECT 1 FROM assignments a WHERE a.id=source_stage AND a.status='active' AND a.original_person=personal_codes.owner)) RETURNING owner").bind(at).bind(code).bind(purpose.name()).fetch_optional(&mut *conn).await?.ok_or("unauthenticated")?;
    let route = if purpose == Purpose::Chat {
        Some(chat_entry(conn, &owner).await?)
    } else {
        None
    };
    let id = new_id(conn).await?;
    let token = format!("yuki_device_{}", random::<32>()?);
    sqlx::query("INSERT INTO device_origins VALUES(?,?)")
        .bind(&id)
        .bind(owner)
        .execute(&mut *conn)
        .await?;
    sqlx::query("INSERT INTO personal_devices VALUES(?,?,?,?,?,NULL,?)")
        .bind(&id)
        .bind(hash(&token))
        .bind(purpose.name())
        .bind(label)
        .bind(at)
        .bind(route)
        .execute(conn)
        .await?;
    Ok(json!({"device_id":id,"token":token}))
}
pub async fn revoke(conn: &mut SqliteConnection, id: &str, owner: Option<&str>) -> Result<Value> {
    let row=sqlx::query_scalar::<_,String>("UPDATE personal_devices SET revoked=coalesce(revoked,?1) WHERE id=?2 AND (?3 IS NULL OR EXISTS(SELECT 1 FROM device_origins d JOIN persons p ON p.id=d.original_person WHERE d.device_id=?2 AND coalesce(p.current_root,p.id)=?3)) RETURNING id").bind(now()?).bind(id).bind(owner).fetch_optional(&mut *conn).await?.ok_or("device_not_found")?;
    sqlx::query("DELETE FROM personal_proofs WHERE device=?")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM personal_sessions WHERE device=?")
        .bind(id)
        .execute(conn)
        .await?;
    Ok(json!({"device_id":row,"revoked":true}))
}
