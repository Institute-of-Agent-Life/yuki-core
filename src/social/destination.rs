//! Resolve a canonical target before a QQ part acquires dispatch rights.
use super::{
    content::{Canonical, Target},
    send::{self, Route},
};
use crate::{Result, history::query};
use sqlx::{Row, SqliteConnection};

#[derive(Debug)]
pub(crate) struct Refused(pub &'static str);
impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Refused {}

impl Canonical {
    pub(crate) fn validate(&self) -> Result<()> {
        if !matches!(self.kind.as_str(), "person" | "space")
            || !std::iter::once(&self.target_id)
                .chain(self.conversation_id.iter())
                .chain(self.binding_id.iter())
                .all(|v| query::contacts::uuid(v))
        {
            return Err(Refused("invalid canonical send target").into());
        }
        Ok(())
    }

    /// Resolve both the requested subject and the entry through the current identity graph.
    /// A conversation selector takes precedence over the optional binding, as in V4.
    pub(crate) async fn matches(&self, db: &mut SqliteConnection, entry: &str) -> Result<bool> {
        self.validate()?;
        if self
            .conversation_id
            .as_ref()
            .is_some_and(|v| !v.eq_ignore_ascii_case(entry))
        {
            return Ok(false);
        }
        let binding = self
            .binding_id
            .as_ref()
            .filter(|_| self.conversation_id.is_none())
            .map(|b| b.to_ascii_lowercase());
        let sql = if self.kind == "person" {
            "SELECT EXISTS(SELECT 1 FROM entries e JOIN assignments a ON a.id=e.actor_stage AND a.kind='person' AND a.status='active'
              JOIN persons p ON p.id=a.original_person AND p.enabled JOIN persons root ON root.id=coalesce(p.current_root,p.id) AND root.enabled
              JOIN persons target ON target.id=lower(?2) AND target.enabled AND coalesce(target.current_root,target.id)=root.id
              WHERE e.id=?1 AND e.scope='private' AND e.closed_at IS NULL AND (?3 IS NULL OR a.id=?3))"
        } else {
            "SELECT EXISTS(SELECT 1 FROM entries e JOIN contact_spaces s ON s.space_key=e.space OR s.id=lower(e.space)
             JOIN contact_space_bindings b ON b.subject=s.id JOIN observed_space_bindings o USING(subject,platform,namespace,native)
             WHERE e.id=?1 AND e.scope='group' AND e.closed_at IS NULL AND s.id=lower(?2)
             AND json_valid(e.space) AND json_extract(e.space,'$[0]')=b.platform AND json_extract(e.space,'$[1]')=b.namespace AND json_extract(e.space,'$[2]')=b.native
             AND (?3 IS NULL OR b.id=?3))"
        };
        Ok(sqlx::query_scalar(sql)
            .bind(entry)
            .bind(&self.target_id)
            .bind(binding)
            .fetch_one(db)
            .await?)
    }
}

pub(super) async fn select(
    db: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    op: &str,
) -> Result<(Route, Option<i64>)> {
    let mut home = send::route(db, exec, actor).await?;
    let raw:Option<String>=sqlx::query_scalar("SELECT json_extract(payload,'$.target') FROM operations WHERE namespace=? AND operation_id=? AND action='external' AND target='send'").bind(exec).bind(op).fetch_one(&mut *db).await?;
    let Some(raw) = raw else {
        return Ok((home, None));
    };
    let target: Target = serde_json::from_str(&raw)?;
    let target = match target {
        Target::Legacy(v) => {
            if (home.group && v.kind == "space" && v.id == home.entry)
                || (!home.group && v.kind == "person" && v.id == home.person)
            {
                return Ok((home, None));
            }
            return Err("target differs from original conversation".into());
        }
        Target::Canonical(v) => v,
    };
    if target.matches(db, &home.entry).await? {
        return Ok((home, None));
    }
    // The directory is bounded to this original human; delegated Work cannot expand its home.
    let scope = query::members::scope(db, exec, actor).await?;
    if target.kind == "space" {
        return group(db, home, &scope, &target).await;
    }
    let rows=sqlx::query("SELECT d.id,d.actor_stage,a.native,coalesce(w.generation,1) AS generation,max(e.id) AS observation
        FROM entries d JOIN assignments p ON p.id=d.actor_stage AND p.status='active' JOIN accounts a ON a.id=p.account_id AND a.platform='qq'
        JOIN persons person ON person.id=p.original_person AND person.enabled JOIN persons root ON root.id=coalesce(person.current_root,person.id) AND root.enabled
        JOIN persons target ON target.id=lower(?5) AND target.enabled AND coalesce(target.current_root,target.id)=root.id
        JOIN events e ON e.entry_id=d.id AND e.actor_stage=p.id AND e.bot_stage=d.bot_stage AND json_extract(e.envelope,'$.kind')='message'
        LEFT JOIN main_windows w ON w.conversation=d.id
        WHERE d.scope='private' AND d.closed_at IS NULL AND d.bot_stage=?1 AND d.id IN(SELECT value FROM json_each(?2))
          AND (?3 IS NULL OR d.id=lower(?3)) AND (?4 IS NULL OR p.id=lower(?4))
          AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id)
        GROUP BY d.id ORDER BY d.id LIMIT 2")
        .bind(&home.stage).bind(serde_json::to_string(&scope.entries)?).bind(&target.conversation_id)
        .bind(target.binding_id.as_ref().filter(|_|target.conversation_id.is_none())).bind(&target.target_id).fetch_all(&mut *db).await?;
    let row = match rows.as_slice() {
        [row] => row,
        [] => return Err(Refused("target_entry_mismatch").into()),
        _ => return Err(Refused("binding_required").into()),
    };
    home.entry = row.get("id");
    home.private_stage = Some(row.get("actor_stage"));
    home.target = row.get::<&str, _>("native").parse()?;
    home.window = row.get("generation");
    home.group = false;
    home.revision = None;
    Ok((home, Some(row.get("observation"))))
}

/// Use the gateway's persisted configuration/recovery decision. Never route via
/// a guessed group number, an old inbound bot, or an offline substitute.
async fn group(
    db: &mut SqliteConnection,
    mut home: Route,
    scope: &query::Scope,
    target: &Canonical,
) -> Result<(Route, Option<i64>)> {
    let rows=sqlx::query("SELECT d.id,d.space,coalesce(w.generation,1) AS generation,max(e.id) AS observation
         FROM contact_spaces s JOIN entries d ON s.space_key=d.space OR s.id=lower(d.space)
         JOIN events e ON e.entry_id=d.id AND json_extract(e.envelope,'$.kind')='message'
         JOIN persons witness ON witness.id=e.raw_actor AND witness.enabled JOIN persons origin ON origin.id=?4
            AND coalesce(witness.current_root,witness.id)=coalesce(origin.current_root,origin.id)
         LEFT JOIN main_windows w ON w.conversation=d.id
         WHERE s.id=lower(?1) AND d.id IN(SELECT value FROM json_each(?2)) AND (?3 IS NULL OR d.id=lower(?3))
           AND d.closed_at IS NULL AND NOT EXISTS(SELECT 1 FROM invalidated_sources i WHERE i.source=e.id)
         GROUP BY d.id ORDER BY d.id")
        .bind(&target.target_id).bind(serde_json::to_string(&scope.entries)?).bind(&target.conversation_id).bind(&scope.subject).fetch_all(&mut *db).await?;
    let mut chosen = None;
    for row in rows {
        if !target.matches(db, row.get("id")).await? {
            continue;
        }
        let Ok((platform, ns, native)) =
            serde_json::from_str::<(String, String, String)>(row.get("space"))
        else {
            continue;
        };
        if platform != "qq" {
            continue;
        }
        let Some((stage, revision, true)) =
            crate::host::participation::qq_route(db, &ns, &native).await?
        else {
            continue;
        };
        if stage != home.stage {
            continue;
        }
        if chosen.is_some() {
            return Err(Refused("conversation_required").into());
        }
        chosen = Some((row, native.parse::<i64>()?, revision));
    }
    let (row, native, rev) = chosen.ok_or(Refused("target_entry_mismatch"))?;
    home.entry = row.get("id");
    home.group = true;
    home.target = native;
    home.window = row.get("generation");
    home.private_stage = None;
    home.revision = Some(rev);
    Ok((home, Some(row.get("observation"))))
}
