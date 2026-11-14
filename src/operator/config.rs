//! Resolve a deployed operator or the original authenticated chat device.
use crate::{Result, host::personal::credentials, media::blob::Root};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use std::{collections::BTreeSet, path::Path, sync::LazyLock};

pub(crate) const READ_MODELS: &str = "control.config.file.content.read";
pub(crate) const WRITE_MODELS: &str = "control.config.file.mutate";

static CAPABILITIES: LazyLock<BTreeSet<String>> = LazyLock::new(|| {
    let source: serde_json::Value =
        serde_json::from_str(include_str!("../../protocol/admin/capabilities.json"))
            .expect("fixed v4 capability declarations");
    serde_json::from_value(source["capabilities"].clone()).expect("fixed capability IDs")
});

static PERSON_CAPABILITIES: LazyLock<serde_json::Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../protocol/admin/person-capabilities.json"
    ))
    .expect("fixed v4 Person capabilities")
});

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Declaration {
    principal_id: String,
    token_env: String,
    person_id: Option<String>,
    #[serde(default = "enabled")]
    enabled: bool,
    #[serde(default = "operator_roles")]
    roles: BTreeSet<String>,
    #[serde(default)]
    capabilities: BTreeSet<String>,
}

fn enabled() -> bool {
    true
}

fn operator_roles() -> BTreeSet<String> {
    BTreeSet::from(["operator".into()])
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    #[serde(default)]
    operators: Vec<Declaration>,
}

fn declarations(path: Option<&Path>) -> Result<Vec<Declaration>> {
    let Some(path) = path else {
        return Ok(Vec::new());
    };
    let parent = path.parent().ok_or("operator_configuration_unavailable")?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("operator_configuration_unavailable")?;
    let snapshot = Root::open(parent)?.snapshot(name)?;
    if snapshot.data.len() > 256 * 1024 {
        return Err("operator_configuration_unavailable".into());
    }
    let mut registry: Registry = toml::from_str(std::str::from_utf8(&snapshot.data)?)
        .map_err(|_| "operator_configuration_unavailable")?;
    let mut names = BTreeSet::new();
    let mut aliases = BTreeSet::new();
    for item in &mut registry.operators {
        item.principal_id.make_ascii_lowercase();
        item.roles = item
            .roles
            .iter()
            .map(|role| role.trim().to_ascii_lowercase())
            .collect();
        item.capabilities = item
            .capabilities
            .iter()
            .map(|capability| capability.trim().to_ascii_lowercase())
            .collect();
        credentials::uuid(&item.principal_id)?;
        if !names.insert(item.principal_id.clone())
            || !aliases.insert(item.token_env.clone())
            || item.token_env.is_empty()
            || item.token_env.len() > 128
            || !item.token_env.starts_with(|c: char| c.is_ascii_uppercase())
            || !item
                .token_env
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
            || item.roles.iter().any(|role| {
                role.is_empty()
                    || role.len() > 64
                    || !role.starts_with(|c: char| c.is_ascii_lowercase())
                    || !role
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
            })
            || !item.capabilities.is_subset(&CAPABILITIES)
        {
            return Err("operator_configuration_unavailable".into());
        }
        if let Some(person) = &mut item.person_id {
            person.make_ascii_lowercase();
            credentials::uuid(person)?;
        }
    }
    Ok(registry.operators)
}

fn matches(hash: &str, secret: &str) -> bool {
    if !secret.is_ascii() || !(32..=4096).contains(&secret.len()) {
        return false;
    }
    let original = format!("{:x}", Sha256::digest(secret.as_bytes()));
    hash.len() == original.len()
        && hash
            .bytes()
            .zip(original.bytes())
            .fold(0u8, |difference, (a, b)| difference | (a ^ b))
            == 0
}

pub(crate) struct Actor {
    pub namespace: String,
    pub source_person: Option<String>,
}

#[derive(Clone)]
pub(crate) enum Authority {
    Os,
    Bearer(String),
    Invocation {
        plugin: String,
        generation: String,
        handle: String,
    },
}

/// OS authority is constructed by the protected local socket handler.
/// HTTP handlers resolve a bearer hash before entering this shared service.
pub(crate) async fn authorize(
    db: &mut SqliteConnection,
    operators: Option<&Path>,
    authority: &Authority,
    capability: &str,
) -> Result<Actor> {
    let hash = match authority {
        Authority::Invocation {
            plugin,
            generation,
            handle,
        } => {
            crate::host::domain::active(db, plugin, generation, "control.invoke").await?;
            let (execution, owner, admin): (String,String,bool) = sqlx::query_as("SELECT c.execution,x.original_actor,r.admin FROM host_calls c JOIN executions x ON x.id=c.execution JOIN persons p ON p.id=x.original_actor AND p.enabled JOIN persons r ON r.id=coalesce(p.current_root,p.id) AND r.enabled WHERE c.handle=? AND c.plugin=? AND c.generation=? AND c.state='dispatched' AND c.method IN ('command.invoke','tool.call')")
                .bind(handle).bind(plugin).bind(generation).fetch_optional(&mut *db).await?.ok_or("unauthenticated")?;
            crate::runtime::effects::authorize(db, &execution, &owner, true).await?;
            return person_access(owner, admin, capability);
        }
        Authority::Os => {
            return Ok(Actor {
                namespace: "os-operator".into(),
                source_person: None,
            });
        }
        Authority::Bearer(hash) => hash,
    };
    let matching: Vec<_> = declarations(operators)?
        .into_iter()
        .filter(|item| matches(hash, &std::env::var(&item.token_env).unwrap_or_default()))
        .collect();
    if matching.len() > 1 {
        return Err("unauthenticated".into());
    }
    if let Some(operator) = matching.into_iter().next() {
        // A recognized operator never falls through to another identity or role.
        if !operator.enabled {
            return Err("precondition_failed".into());
        }
        if !operator.capabilities.contains(capability) {
            return Err("forbidden".into());
        }
        if let Some(person) = &operator.person_id {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM persons WHERE id=?)")
                    .bind(person)
                    .fetch_one(db)
                    .await?;
            if !exists {
                return Err("unauthenticated".into());
            }
        }
        return Ok(Actor {
            namespace: format!("operator:{}", operator.principal_id),
            source_person: operator.person_id,
        });
    }
    let person = credentials::authenticate(db, hash, credentials::Purpose::Chat).await?;
    person_actor(person, capability)
}

fn person_actor(person: credentials::Principal, capability: &str) -> Result<Actor> {
    person_access(person.owner, person.admin, capability)
}
pub(super) fn person_access(owner: String, admin: bool, capability: &str) -> Result<Actor> {
    let allowed = &PERSON_CAPABILITIES[if admin { "superuser" } else { "user" }];
    if !allowed
        .as_array()
        .is_some_and(|caps| caps.iter().any(|c| c == capability))
    {
        return Err("forbidden".into());
    }
    Ok(Actor {
        namespace: format!("person:{owner}"),
        source_person: Some(owner),
    })
}
