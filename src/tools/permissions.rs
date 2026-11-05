//! Internal permission metadata for the current original principal, never an execution grant.
use crate::{Result, runtime::effects, tools::registry::Tool};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqliteConnection;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};

static ORIGINAL: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../fixtures/control/v4-permissions.json"))
        .expect("fixed v4 permission directory")
});

pub(crate) fn declaration() -> Tool {
    let d = &ORIGINAL["declaration"];
    Tool {
        name: "get_my_capabilities".into(),
        description: d["description"].as_str().unwrap().into(),
        schema: d["parameters"].clone(),
        parallel_read: true,
        cacheable: false,
        code_direct: true,
        provider: None,
    }
}
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct Request {
    mode: Option<String>,
    category: Option<String>,
    query: Option<String>,
}
fn folded(s: &str) -> String {
    crate::history::normalize::casefold(s.chars())
}
fn projection(admin: bool, args: &Value) -> Result<Value> {
    let q: Request = serde_json::from_value(args.clone())?;
    let mode = q.mode.as_deref().unwrap_or("summary");
    if !matches!(mode, "summary" | "focused" | "full")
        || q.query.as_ref().is_some_and(|s| s.chars().count() > 64)
        || (mode == "focused"
            && q.category.as_ref().is_none_or(|s| s.is_empty())
            && q.query.as_ref().is_none_or(|s| s.is_empty()))
    {
        return Err("invalid_arguments".into());
    }
    let category = q
        .category
        .filter(|s| !s.is_empty())
        .map(|s| folded(s.trim()));
    let query = q.query.filter(|s| !s.is_empty()).map(|s| folded(s.trim()));
    let selected: Vec<&Value> = ORIGINAL["descriptors"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| {
            if d["minimum_rank"].as_u64().unwrap() > if admin { 100 } else { 0 } {
                return false;
            }
            if category
                .as_ref()
                .is_some_and(|c| folded(d["category"].as_str().unwrap()) != *c)
            {
                return false;
            }
            query.as_ref().is_none_or(|q| {
                ["id", "category", "display_name", "description"]
                    .iter()
                    .map(|k| d[*k].as_str().unwrap())
                    .chain(
                        d["search_terms"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|s| s.as_str().unwrap()),
                    )
                    .any(|s| folded(s).contains(q))
            })
        })
        .collect();
    let count = |kind: &str, mutating: Option<bool>| {
        selected
            .iter()
            .filter(|d| d["kind"] == kind && mutating.is_none_or(|m| d["mutating"] == m))
            .count()
    };
    let mut out = json!({"transient_internal_reference":true,"do_not_copy_verbatim_to_user":true,"permission_level":if admin{"superuser"}else{"user"},"permission_source":if admin{"person"}else{"default_user"},"counts":{"total":selected.len(),"mutable_configurations":count("configuration",Some(true)),"protected_configurations":count("configuration",Some(false)),"self_service_operations":count("command",None),"self_service_mutations":count("command",Some(true))}});
    let mut categories: BTreeMap<String, [usize; 4]> = BTreeMap::new();
    for d in &selected {
        let c = categories
            .entry(d["category"].as_str().unwrap().into())
            .or_default();
        c[0] += 1;
        c[if d["kind"] == "command" { 1 } else { 2 }] += 1;
        if d["mutating"] == true {
            c[3] += 1;
        }
    }
    out["categories"] = json!(
        categories
            .into_iter()
            .map(|(k, c)| (
                k,
                json!({"total":c[0],"commands":c[1],"configurations":c[2],"mutating":c[3]})
            ))
            .collect::<BTreeMap<_, _>>()
    );
    if mode == "focused" {
        out["capabilities"]=json!(selected.iter().map(|d|json!({"id":d["id"],"kind":d["kind"],"category":d["category"],"name":d["display_name"],"description":d["description"],"mutating":d["mutating"],"scopes":d["target_scopes"],"apply_mode":d["apply_mode"],"value_type":d["value_type"],"minimum":d["minimum"],"maximum":d["maximum"],"choices":d["choices"],"aliases":d["search_terms"]})).collect::<Vec<_>>());
    } else if mode == "full" {
        out.as_object_mut().unwrap().remove("categories");
        let mut groups: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
        let mut scopes = BTreeSet::new();
        let mut modes = BTreeSet::new();
        for d in selected {
            groups
                .entry(d["category"].as_str().unwrap().into())
                .or_default()
                .entry(d["kind"].as_str().unwrap().into())
                .or_default()
                .push(d["id"].as_str().unwrap().into());
            for s in d["target_scopes"].as_array().unwrap() {
                scopes.insert(s.as_str().unwrap());
            }
            if let Some(s) = d["apply_mode"].as_str() {
                modes.insert(s);
            }
        }
        out["capability_ids"] = json!(groups);
        out["available_scopes"] = json!(scopes);
        out["available_apply_modes"] = json!(modes);
    }
    Ok(out)
}

pub(crate) async fn read(
    db: &mut SqliteConnection,
    exec: &str,
    actor: &str,
    args: &Value,
) -> Result<Value> {
    effects::authorize(db, exec, actor, false).await?;
    // A registered SELF source stays SELF through its children and schedules. No model-supplied identity participates.
    let is_self:bool=sqlx::query_scalar("WITH RECURSIVE family(id) AS(SELECT ?1 UNION SELECT o.parent FROM family f JOIN executions x ON x.id=f.id JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL) SELECT EXISTS(SELECT 1 FROM family f JOIN executions x ON x.id=f.id JOIN main_origins o ON o.ref=x.origin WHERE o.kind='self')").bind(exec).fetch_one(&mut *db).await?;
    let admin = if is_self {
        false
    } else {
        // Resolve the original source principal without enumerating any history directory.
        sqlx::query_scalar("WITH RECURSIVE family(id) AS(SELECT ?1 UNION SELECT o.parent FROM family f JOIN executions x ON x.id=f.id JOIN main_origins o ON o.ref=x.origin WHERE o.parent IS NOT NULL) SELECT r.admin FROM family f JOIN executions x ON x.id=f.id JOIN persons p ON p.id=x.original_actor JOIN persons r ON r.id=coalesce(p.current_root,p.id) WHERE x.source IS NOT NULL AND p.enabled AND r.enabled").bind(exec).fetch_optional(&mut *db).await?.ok_or("permission_context_mismatch")?
    };
    projection(admin, args)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_permission_views_and_filters() {
        for case in ORIGINAL["cases"].as_array().unwrap() {
            assert_eq!(
                projection(case["admin"].as_bool().unwrap(), &case["args"]).unwrap(),
                case["result"],
                "{:?}",
                case["args"]
            );
        }
        for args in [
            json!({"person_id":"someone"}),
            json!({"mode":"focused"}),
            json!({"mode":"unknown"}),
            json!({"query":"x".repeat(65)}),
        ] {
            assert!(projection(true, &args).is_err());
        }
    }
}
