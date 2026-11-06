//! Read a command in its original namespace, including pre-selection App receipts.
use super::credentials::Principal;
use crate::Result;
use sqlx::{SqliteConnection, sqlite::SqliteRow};

pub(super) async fn find(
    db: &mut SqliteConnection,
    who: &Principal,
    operation: &str,
) -> Result<Option<SqliteRow>> {
    let mut rows=sqlx::query("SELECT r.* FROM personal_receipts r WHERE r.owner=?1 AND r.operation=?2 AND (r.scope=?3 OR (?4 AND EXISTS(SELECT 1 FROM entries e JOIN assignments s ON s.id=e.actor_stage JOIN accounts a ON a.id=s.account_id WHERE e.id=r.scope AND s.original_person=?1 AND a.platform='app' AND e.scope='private'))) LIMIT 2").bind(&who.request_person).bind(operation).bind(who.request_scope()).bind(who.thread.is_none()).fetch_all(db).await?;
    if rows.len() > 1 {
        return Err("request_conflict".into());
    }
    Ok(rows.pop())
}

pub(super) async fn before_mutation(
    db: &mut SqliteConnection,
    who: &Principal,
    op: &str,
) -> Result<Option<SqliteRow>> {
    if super::credentials::code_receipt(db, who, op)
        .await?
        .is_some()
    {
        return Err("request_conflict".into());
    }
    find(db, who, op).await
}
