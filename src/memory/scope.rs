use crate::{Result, history::query, runtime::effects};
use serde_json::Value;
use sqlx::{Row, SqliteConnection};

pub struct Reader {
    pub archived_evidence: bool,
    pub write_subject: Option<super::facts::Subject>,
    pub base: query::Scope,
    pub root: String,
    pub actor: String,
    pub admin: bool,
    pub delegated: bool,
}
impl Reader {
    pub async fn at(
        conn: &mut SqliteConnection,
        exec: &str,
        actor: &str,
        admin_requested: bool,
        write: bool,
    ) -> Result<Self> {
        effects::authorize(conn, exec, actor, write).await?;
        let base = query::scope(conn, exec, actor).await?;
        let (root,admin):(String,bool)=sqlx::query_as("SELECT r.id,r.admin FROM persons p JOIN persons r ON r.id=coalesce(p.current_root,p.id) WHERE p.id=? AND p.enabled AND r.enabled").bind(&base.subject).fetch_one(&mut *conn).await?;
        let delegated: bool =
            sqlx::query_scalar("SELECT origin IS NOT NULL FROM executions WHERE id=?")
                .bind(exec)
                .fetch_one(&mut *conn)
                .await?;
        if admin_requested && (!admin || delegated) {
            return Err(
                "explicit memory administration requires the original administrator".into(),
            );
        }
        Ok(Self {
            archived_evidence: false,
            write_subject: None,
            base,
            root,
            actor: actor.into(),
            admin: admin_requested,
            delegated,
        })
    }
    pub fn admits_entry(&self, entry: &str) -> bool {
        self.base.entries.iter().any(|v| v == entry)
    }
}

/// Apply the whole subject predicate before limiting or exposing any row. Common groups are
/// direct, surviving observations. Another person's readable groups never enlarge this caller.
pub async fn visible(
    conn: &mut SqliteConnection,
    r: &Reader,
    id: Option<&str>,
    cursor: &str,
    limit: i64,
) -> Result<Vec<String>> {
    let rows=sqlx::query_scalar("WITH relations AS MATERIALIZED(SELECT person,space FROM memory_group_relations),
     spaces AS MATERIALIZED(SELECT space FROM relations WHERE person=?3 AND (NOT ?6 OR space=(SELECT space FROM entries WHERE id=?4))),
     peers AS MATERIALIZED(SELECT DISTINCT person FROM relations WHERE space IN(SELECT space FROM spaces))
     SELECT f.id FROM memory_facts f LEFT JOIN persons p ON p.id=f.person LEFT JOIN persons owner ON owner.id=coalesce(p.current_root,p.id)
     WHERE (?1 IS NULL OR f.id=?1) AND f.id>?2 AND f.status!='erased' AND (?1 IS NOT NULL OR f.status='active')
     AND (f.person IS NULL OR (p.enabled AND owner.enabled)) AND (
       (f.kind='person' AND (owner.id=?3 OR owner.id IN(SELECT person FROM peers)))
       OR (f.kind='group' AND f.space IN(SELECT space FROM spaces))
       OR (f.kind='person_group' AND f.space IN(SELECT space FROM spaces) AND (owner.id,f.space) IN(SELECT person,space FROM relations))
       OR (f.kind='self' AND (f.visibility='global' OR f.visibility_entry=?4))
       OR (?5 AND f.kind!='self' AND EXISTS(SELECT 1 FROM memory_evidence original WHERE original.fact=f.id AND original.entry=?4 AND original.quote IS NOT NULL)))
     ORDER BY f.id LIMIT ?7").bind(id).bind(cursor).bind(&r.root).bind(&r.base.home).bind(r.admin).bind(r.delegated).bind(limit).fetch_all(conn).await?;
    Ok(rows)
}
pub async fn writable(conn: &mut SqliteConnection, r: &Reader, id: &str) -> Result<()> {
    let row=sqlx::query("SELECT f.kind,f.person,f.space,f.visibility,f.visibility_entry,coalesce(p.current_root,p.id) AS owner FROM memory_facts f LEFT JOIN persons p ON p.id=f.person WHERE f.id=? AND f.status='active'").bind(id).fetch_one(&mut *conn).await?;
    if let Some(granted) = &r.write_subject {
        let subject = super::facts::Subject {
            kind: row.get("kind"),
            person: row.get("person"),
            space: row.get("space"),
            visibility: Some(row.get("visibility")),
            entry: row.get("visibility_entry"),
        };
        if !same_subject(conn, granted, &subject).await? {
            return Err("fact outside installed maintenance subject".into());
        }
    } else if !r.admin
        && (row.try_get::<String, _>("kind")? != "person"
            || row.try_get::<Option<String>, _>("owner")?.as_deref() != Some(&r.root))
    {
        return Err(
            "fact modification requires its owner or explicit scoped administration".into(),
        );
    }
    if visible(conn, r, Some(id), "", 1).await?.is_empty() {
        return Err("fact unavailable".into());
    }
    Ok(())
}
pub fn hash(v: &Value) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(v.to_string().as_bytes()))
}

/// Maintenance carries an operator-installed exact subject independently of administrator roles.
pub(crate) async fn same_subject(
    conn: &mut SqliteConnection,
    allowed: &super::facts::Subject,
    actual: &super::facts::Subject,
) -> Result<bool> {
    if allowed.kind != actual.kind
        || allowed.space != actual.space
        || allowed.entry != actual.entry
        || allowed.visibility.as_deref().unwrap_or("subject")
            != actual.visibility.as_deref().unwrap_or("subject")
    {
        return Ok(false);
    }
    let left: Option<String> =
        sqlx::query_scalar("SELECT coalesce(current_root,id) FROM persons WHERE id=? AND enabled")
            .bind(&allowed.person)
            .fetch_optional(&mut *conn)
            .await?;
    let right: Option<String> =
        sqlx::query_scalar("SELECT coalesce(current_root,id) FROM persons WHERE id=? AND enabled")
            .bind(&actual.person)
            .fetch_optional(conn)
            .await?;
    Ok(allowed.person.is_some() == actual.person.is_some()
        && left == right
        && (left.is_some() || allowed.person.is_none()))
}
