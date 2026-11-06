use super::credentials::{self, Principal, Purpose};
use crate::Result;
use axum::http::HeaderMap;
use sqlx::{Row, SqliteConnection};

#[derive(Default)]
pub(crate) struct Selection {
    binding: Option<String>,
    conversation: Option<String>,
}
impl Selection {
    pub(super) fn headers(h: &HeaderMap) -> Result<Self> {
        fn one(h: &HeaderMap, name: &'static str) -> Result<Option<String>> {
            let mut values = h.get_all(name).iter();
            let value = values.next().map(|v| v.to_str()).transpose()?;
            if values.next().is_some() || value.is_some_and(|v| v.len() > 256) {
                return Err("validation_error".into());
            }
            Ok(value.map(str::to_owned))
        }
        Ok(Self {
            binding: one(h, "x-yuki-entry")?,
            conversation: one(h, "x-yuki-conversation")?,
        })
    }
    fn explicit(&self) -> bool {
        self.binding.is_some() || self.conversation.is_some()
    }
}

pub(super) async fn authenticate(
    db: &mut SqliteConnection,
    hash: &str,
    purpose: Purpose,
    selection: &Selection,
    write: bool,
) -> Result<Principal> {
    let id = credentials::token_device(db, hash).await?;
    let who = credentials::subject(db, &id, purpose, false).await?;
    resolve(db, who, selection, write).await
}
pub(super) async fn delegated(
    db: &mut SqliteConnection,
    device: &str,
    purpose: Purpose,
    entry: Option<&str>,
    write: bool,
) -> Result<Principal> {
    let who = credentials::subject(db, device, purpose, false).await?;
    bound(db, who, entry, write).await
}
/// Source device, effective root and request binding are three separate identities.
/// Resolve on the same connection as the subsequent read or mutation.
pub(super) async fn resolve(
    db: &mut SqliteConnection,
    mut who: Principal,
    selection: &Selection,
    write: bool,
) -> Result<Principal> {
    if who.entry.is_none() {
        return if selection.explicit() {
            Err("unauthenticated".into())
        } else {
            Ok(who)
        };
    }
    let default = if selection.explicit() {
        None
    } else {
        who.entry.as_deref()
    };
    let acp = selection.conversation.is_some();
    let sql = "SELECT e.id,s.original_person,s.status AS actor_status,b.status AS bot_status,e.closed_at,e.topic,a.platform
      FROM entries e JOIN assignments s ON s.id=e.actor_stage JOIN assignments b ON b.id=e.bot_stage
      JOIN accounts a ON a.id=s.account_id JOIN accounts bot ON bot.id=b.account_id
      JOIN persons p ON p.id=s.original_person AND p.enabled
      WHERE e.scope='private' AND b.kind='self' AND a.platform=bot.platform AND a.namespace=bot.namespace
      AND (?4 IS NULL OR e.id=?4) AND coalesce(p.current_root,p.id)=?1 AND (?2 IS NULL OR s.id=?2) AND
      ((?3 IS NOT NULL AND e.id=?3 AND a.platform='acp' AND bot.native='yuki-acp' AND e.topic IS NOT NULL)
      OR (?3 IS NULL AND a.platform='app' AND a.namespace='personal' AND bot.native='yuki-app' AND e.topic IS NULL)) LIMIT 2";
    let rows = sqlx::query(sql)
        .bind(&who.root)
        .bind(&selection.binding)
        .bind(&selection.conversation)
        .bind(default)
        .fetch_all(db)
        .await?;
    let [row] = rows.as_slice() else {
        return Err(if acp { "not_found" } else { "unauthenticated" }.into());
    };
    if write
        && (row.get::<&str, _>("actor_status") != "active"
            || row.get::<&str, _>("bot_status") != "active"
            || row.get::<Option<i64>, _>("closed_at").is_some())
    {
        return Err("precondition_failed".into());
    }
    who.request_person = row.get("original_person");
    who.entry = Some(row.get("id"));
    who.thread = row.get("topic");
    Ok(who)
}

/// Reconstruct a stored delegation from its immutable entry. NULL in readers <=46
/// denotes the original device entry, the only supported issuer in those readers.
pub(super) async fn bound(
    db: &mut SqliteConnection,
    who: Principal,
    entry: Option<&str>,
    write: bool,
) -> Result<Principal> {
    let Some(id) = entry else {
        return resolve(db, who, &Selection::default(), write).await;
    };
    let (platform,binding):(String,String)=sqlx::query_as("SELECT a.platform,e.actor_stage FROM entries e JOIN assignments s ON s.id=e.actor_stage JOIN accounts a ON a.id=s.account_id WHERE e.id=? AND e.scope='private'").bind(id).fetch_optional(&mut *db).await?.ok_or("not_found")?;
    let selection = Selection {
        binding: Some(binding),
        conversation: (platform == "acp").then(|| id.to_owned()),
    };
    let who = resolve(db, who, &selection, write).await?;
    if who.entry.as_deref() != Some(id) {
        return Err("not_found".into());
    }
    Ok(who)
}
