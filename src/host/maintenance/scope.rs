use super::{Change, Policy, Scope};
use crate::{Result, history::query, memory::scope::Reader};
use sqlx::SqliteConnection;

pub(super) struct Granted {
    pub revision: String,
    pub policy: Scope,
    pub reader: Reader,
    pub embedding_routes:
        std::collections::BTreeMap<String, crate::memory::embedding::profile::Profile>,
}
pub(super) async fn resolve(db: &mut SqliteConnection, c: &Change) -> Result<Granted> {
    let (revision,body):(String,String)=sqlx::query_as("SELECT p.revision,p.body FROM host_maintenance p JOIN host_installations i ON i.plugin=p.plugin AND i.revision=p.revision WHERE p.plugin=? AND i.enabled AND i.online")
        .bind(&c.plugin).fetch_one(&mut *db).await?;
    let policy: Policy = serde_json::from_str(&body)?;
    let scope = policy
        .scopes
        .get(c.params["scope"].as_str().ok_or("named scope required")?)
        .ok_or("scope not installed")?
        .clone();
    if !scope.methods.contains(&c.method)
        || !scope
            .purposes
            .contains(c.params["purpose"].as_str().ok_or("purpose required")?)
    {
        return Err("method or purpose outside maintenance grant".into());
    }
    let base = query::for_person(db, &scope.actor, &scope.entry, true).await?;
    if !base.entries.contains(&scope.entry) {
        return Err("original maintenance source scope unavailable".into());
    }
    let root:String=sqlx::query_scalar("SELECT r.id FROM persons p JOIN persons r ON r.id=coalesce(p.current_root,p.id) WHERE p.id=? AND p.enabled AND r.enabled")
        .bind(&scope.actor).fetch_one(db).await?;
    let reader = Reader {
        archived_evidence: true,
        base,
        root,
        actor: scope.actor.clone(),
        admin: false,
        delegated: true,
        write_subject: scope.write_subject.clone(),
    };
    Ok(Granted {
        revision,
        policy: scope,
        reader,
        embedding_routes: policy.embedding_routes,
    })
}
