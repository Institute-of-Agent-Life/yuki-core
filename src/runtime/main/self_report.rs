use crate::Result;
use serde_json::Value;
use sqlx::SqliteConnection;

fn extract_tail(text: &str) -> (&str, Option<&'static str>) {
    let Some((body, tail)) = text.rsplit_once("<yuki-state>") else {
        return (text, None);
    };
    let body = body.trim_end();
    let engage = tail
        .trim()
        .strip_suffix("</yuki-state>")
        .map(str::trim)
        .filter(|raw| raw.chars().take(257).count() <= 256)
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|state| {
            let state = state.as_object()?;
            if !state
                .keys()
                .all(|key| matches!(key.as_str(), "engage" | "mood"))
            {
                return None;
            }
            match state.get("engage")?.as_str()? {
                "join" => Some("join"),
                "stay" => Some("stay"),
                "quiet" => Some("quiet"),
                _ => None,
            }
        });
    (body, engage)
}

pub(super) async fn retain(
    conn: &mut SqliteConnection,
    execution: &str,
    logical: &str,
    attempt: &str,
    result: &mut Value,
) -> Result<()> {
    let field = if result["content"].is_string() {
        "content"
    } else {
        "text"
    };
    let Some(text) = result[field].as_str() else {
        return Ok(());
    };
    let (visible, engage) = extract_tail(text);
    if visible == text {
        return Ok(());
    }
    let source: Option<i64> = sqlx::query_scalar(
        "SELECT mi.event FROM main_inputs mi
         JOIN main_turns mt ON mt.execution=mi.execution AND mt.origin IS NULL
         JOIN entries e ON e.id=mt.conversation AND e.scope='group'
         JOIN executions x ON x.id=mi.execution AND x.source=mi.event AND x.work_id IS NULL
         JOIN events ev ON ev.id=mi.event AND ev.raw_actor IS NOT NULL AND ev.observed_generation=mt.window
         LEFT JOIN main_windows w ON w.conversation=mt.conversation
         JOIN model_requests r ON r.id=?2 AND r.original_work_id IS NULL
         WHERE mi.execution=?1 AND mi.consumed AND mt.window=coalesce(w.generation,1)
         AND (SELECT count(*) FROM main_inputs WHERE execution=?1)=1",
    )
    .bind(execution)
    .bind(logical)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(source) = source else { return Ok(()) };
    result[field] = Value::String(visible.to_owned());
    if let Some(engage) = engage {
        sqlx::query(
            "INSERT INTO participation_main_reports VALUES(?,?,?,?,?) ON CONFLICT(logical) DO NOTHING",
        )
        .bind(logical)
        .bind(attempt)
        .bind(execution)
        .bind(source)
        .bind(engage)
        .execute(conn)
        .await?;
    }
    Ok(())
}
