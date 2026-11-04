pub mod assignment;
pub mod entry;
pub mod person;

use crate::Result;
use sqlx::SqliteConnection;

async fn new_id(conn: &mut SqliteConnection) -> Result<String> {
    Ok(sqlx::query_scalar(
        "SELECT lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' ||
       substr(hex(randomblob(2)),2) || '-' || substr('89ab',1+(random() & 3),1) ||
       substr(hex(randomblob(2)),2) || '-' || hex(randomblob(6)))",
    )
    .fetch_one(conn)
    .await?)
}
