use crate::Result;
use serde_json::Value;
use sqlx::SqliteConnection;
/// Bind every private note reference to an actual source in the current Work.
pub(super) async fn validate(
    conn: &mut SqliteConnection,
    work: &str,
    exec: &str,
    note: &Value,
) -> Result<()> {
    if note["version"].as_i64().is_some_and(|v| v != 1) {
        return Err("unsupported Work note version".into());
    }
    for section in ["facts", "unresolved", "next_steps"] {
        for fact in note[section].as_array().into_iter().flatten() {
            if fact["text"].as_str().is_none_or(|s| s.trim().is_empty()) {
                return Err("empty Work note fact".into());
            }
            for reference in fact["refs"].as_array().ok_or("missing Work note refs")? {
                let reference = reference.as_str().ok_or("invalid Work note ref")?;
                if reference == "goal" {
                    continue;
                }
                let (kind, key) = reference
                    .split_once(':')
                    .ok_or("unsupported Work note reference")?;
                let sql = match kind {
                    "input" => {
                        "SELECT EXISTS(SELECT 1 FROM work_mail WHERE id=?1 AND work=?2 AND body IS NOT NULL AND ?3 IS NOT NULL)"
                    }
                    "event" => {
                        "SELECT EXISTS(SELECT 1 FROM events WHERE id=?1 AND id IN(SELECT event FROM main_dependencies WHERE execution=?3 UNION SELECT event FROM work_sources WHERE work=?2) AND NOT EXISTS(SELECT 1 FROM invalidated_sources WHERE source=events.id))"
                    }
                    "effect" => {
                        "SELECT EXISTS(SELECT 1 FROM operations o JOIN work_runs r ON r.execution=o.namespace WHERE o.operation_id=?1 AND o.state NOT IN('intent','dispatched') AND r.work=?2 AND ?3 IS NOT NULL)"
                    }
                    "child" => {
                        "SELECT EXISTS(SELECT 1 FROM work_nodes WHERE id=?1 AND parent=?2 AND goal IS NOT NULL AND ?3 IS NOT NULL)"
                    }
                    "artifact" => {
                        let version: String=sqlx::query_scalar("SELECT version FROM tool_artifacts WHERE handle=? AND body IS NOT NULL").bind(key).fetch_one(&mut *conn).await?;
                        let actor: String = sqlx::query_scalar(
                            "SELECT coalesce(original_actor,origin) FROM executions WHERE id=?",
                        )
                        .bind(exec)
                        .fetch_one(&mut *conn)
                        .await?;
                        let source =
                            crate::tools::results::authorize(conn, exec, &actor, key, &version)
                                .await?;
                        super::tree::provenance(conn, work, &source).await?;
                        continue;
                    }
                    _ => return Err("unsupported Work note reference".into()),
                };
                let authorized: bool = sqlx::query_scalar(sql)
                    .bind(key)
                    .bind(work)
                    .bind(exec)
                    .fetch_one(&mut *conn)
                    .await?;
                if !authorized {
                    return Err("unavailable Work note reference".into());
                }
            }
        }
    }
    Ok(())
}
