//! Approved definitions survive stopped and failed instances. No process is started here.
use crate::{
    Result,
    operator::domain::{get, put},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
use std::collections::BTreeSet;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub schema: Value,
    #[serde(default)]
    pub secret_fields: BTreeSet<String>,
}
impl Definition {
    pub(super) fn validate(&self) -> Result<()> {
        jsonschema::validator_for(&self.schema)?;
        let fields = self.schema["properties"]
            .as_object()
            .ok_or("invalid_plugin_configuration_schema")?;
        if self.schema["type"] != "object"
            || self.schema["additionalProperties"] != false
            || self.secret_fields.iter().any(|key| {
                !fields.get(key).is_some_and(|field| {
                    field["type"] == "string"
                        && field.get("default").is_none()
                        && field["writeOnly"] == true
                })
            })
        {
            return Err("invalid_plugin_configuration_schema".into());
        }
        Ok(())
    }
}
fn definition_key(plugin: &str) -> String {
    format!("plugin-config-definition:{plugin}")
}
pub(super) async fn configure(db: &mut SqliteConnection, install: &super::Install) -> Result<()> {
    if let Some(definition) = &install.configuration {
        put(
            db,
            &definition_key(&install.id),
            &json!({"manifest":install.revision()?,"definition":definition}),
        )
        .await?;
    } else {
        sqlx::query("DELETE FROM core_meta WHERE key=?")
            .bind(definition_key(&install.id))
            .execute(db)
            .await?;
    }
    Ok(())
}
async fn definition(db: &mut SqliteConnection, plugin: &str) -> Result<(Definition, String)> {
    let manifest: String =
        sqlx::query_scalar("SELECT revision FROM host_installations WHERE plugin=?")
            .bind(plugin)
            .fetch_optional(&mut *db)
            .await?
            .ok_or("not_found")?;
    let approved = get(db, &format!("plugin-approval:{plugin}"))
        .await?
        .ok_or("configuration_unavailable")?;
    if !approved["approved"].is_array() {
        return Err("configuration_unavailable".into());
    }
    let value = get(db, &definition_key(plugin))
        .await?
        .ok_or("configuration_unavailable")?;
    if value["manifest"] != manifest {
        return Err("configuration_unavailable".into());
    }
    Ok((
        serde_json::from_value(value["definition"].clone())?,
        manifest,
    ))
}
async fn scope(db: &mut SqliteConnection, args: &Value) -> Result<(String, Option<String>)> {
    let kind = args["scope_type"].as_str().unwrap_or("global");
    let owner = args["owner_id"].as_str();
    match (kind, owner) {
        ("global", None) => Ok((kind.into(), None)),
        ("user", Some(id)) => {
            crate::host::personal::credentials::uuid(id)?;
            let id: String =
                sqlx::query_scalar("SELECT coalesce(current_root,id) FROM persons WHERE id=? AND NOT EXISTS(SELECT 1 FROM forgotten_people WHERE person=persons.id)")
                    .bind(id.to_ascii_lowercase())
                    .fetch_optional(db)
                    .await?
                    .ok_or("not_found")?;
            Ok((kind.into(), Some(id)))
        }
        // Group scopes are canonical Space IDs; a conversation or native ID cannot stand in.
        ("group", Some(id)) => {
            crate::host::personal::credentials::uuid(id)?;
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM contact_spaces WHERE id=?)")
                    .bind(id.to_ascii_lowercase())
                    .fetch_one(db)
                    .await?;
            if !exists {
                return Err("not_found".into());
            }
            Ok((kind.into(), Some(id.to_ascii_lowercase())))
        }
        _ => Err("validation_error".into()),
    }
}
fn key(plugin: &str, kind: &str, owner: &Option<String>) -> String {
    format!(
        "plugin-config-values:{}",
        crate::memory::scope::hash(&json!([plugin, kind, owner]))
    )
}
pub(super) fn revision(manifest: &str, state: &Value) -> Result<u64> {
    Ok(u64::from_str_radix(
        &crate::memory::scope::hash(&json!([manifest, state]))[..13],
        16,
    )? + 1)
}
fn defaults(definition: &Definition, state: &Value) -> Value {
    let mut values = state["values"].as_object().cloned().unwrap_or_default();
    for (key, field) in definition.schema["properties"]
        .as_object()
        .into_iter()
        .flatten()
    {
        if !values.contains_key(key)
            && let Some(default) = field.get("default")
        {
            values.insert(key.clone(), default.clone());
        }
    }
    Value::Object(values)
}
fn public(
    definition: &Definition,
    manifest: &str,
    state: &Value,
    plugin: &str,
    kind: &str,
    owner: &Option<String>,
) -> Result<Value> {
    let mut values = defaults(definition, state);
    let valid = jsonschema::validator_for(&definition.schema)?.is_valid(&values);
    let mut secrets = serde_json::Map::new();
    for field in &definition.secret_fields {
        let configured = values[field].as_str().is_some_and(|s| !s.is_empty());
        values.as_object_mut().unwrap().remove(field);
        secrets.insert(field.clone(), json!({"configured":configured}));
    }
    Ok(
        json!({"plugin_id":plugin,"scope_type":kind,"owner_id":owner,"revision":revision(manifest,state)?,
        "schema":definition.schema,"values":values,"secrets":secrets,"valid":valid,"apply_mode":"plugin_defined"}),
    )
}
pub(crate) async fn read(db: &mut SqliteConnection, args: &Value) -> Result<Value> {
    let plugin = args["plugin_id"].as_str().ok_or("validation_error")?;
    let (definition, manifest) = definition(db, plugin).await?;
    let (kind, owner) = scope(db, args).await?;
    let state = get(db, &key(plugin, &kind, &owner))
        .await?
        .unwrap_or(json!({"values":{},"version":0}));
    public(&definition, &manifest, &state, plugin, &kind, &owner)
}
pub(crate) async fn save(db: &mut SqliteConnection, args: &Value) -> Result<Value> {
    let plugin = args["plugin_id"].as_str().ok_or("validation_error")?;
    let (definition, manifest) = definition(db, plugin).await?;
    let (kind, owner) = scope(db, args).await?;
    let key = key(plugin, &kind, &owner);
    let before = get(db, &key)
        .await?
        .unwrap_or(json!({"values":{},"version":0}));
    if args["expected_revision"] != revision(&manifest, &before)? {
        return Err("version_conflict".into());
    }
    let mut values = args["values"]
        .as_object()
        .cloned()
        .ok_or("validation_error")?;
    for field in &definition.secret_fields {
        // Omission, empty strings, and masked display values are never inferred as intent.
        let action = values.remove(field).ok_or("secret_action_required")?;
        let object = action.as_object().ok_or("secret_action_required")?;
        match action["action"].as_str() {
            Some("keep") if object.len() == 1 => {
                if let Some(value) = before["values"].get(field) {
                    values.insert(field.clone(), value.clone());
                }
            }
            Some("clear") if object.len() == 1 => (),
            Some("replace") if object.len() == 2 => {
                let value = action["value"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 16384)
                    .ok_or("validation_error")?;
                values.insert(field.clone(), json!(value));
            }
            _ => return Err("secret_action_required".into()),
        }
    }
    let next = json!({"scope_type":kind,"owner_id":owner,"values":values,"version":before["version"].as_u64().ok_or("invalid_configuration_revision")?.checked_add(1).ok_or("revision_exhausted")?});
    if !jsonschema::validator_for(&definition.schema)?.is_valid(&defaults(&definition, &next)) {
        return Err("validation_error".into());
    }
    put(db, &key, &next).await?;
    public(&definition, &manifest, &next, plugin, &kind, &owner)
}

pub(crate) async fn callback(
    db: &mut SqliteConnection,
    plugin: &str,
    method: &str,
    args: &Value,
) -> Result<Value> {
    let (definition, _) = definition(db, plugin).await?;
    let field = args["key"].as_str().ok_or("validation_error")?;
    if definition.schema["properties"].get(field).is_none()
        || definition.secret_fields.contains(field) != (method == "plugin.secrets.get")
    {
        return Err("forbidden".into());
    }
    let state = get(db, &key(plugin, "global", &None))
        .await?
        .unwrap_or(json!({"values":{}}));
    Ok(json!({"value":state["values"][field]}))
}

pub(crate) async fn status(db: &mut SqliteConnection, args: &Value) -> Result<Value> {
    use sqlx::Row;
    let id = args["plugin_id"].as_str();
    let rows = sqlx::query("SELECT plugin,namespace,revision,grants,enabled,online,generation,(SELECT json_extract(value,'$.requested') FROM core_meta WHERE key='plugin-approval:'||plugin) AS requested,(SELECT value FROM core_meta WHERE key='plugin-approval:'||plugin) AS approval,EXISTS(SELECT 1 FROM core_meta WHERE key='plugin-install:'||plugin) AS installed FROM host_installations WHERE (?1 IS NULL OR plugin=?1) ORDER BY plugin")
        .bind(id).fetch_all(db).await?;
    let items = rows.iter().map(|row| -> Result<Value> {
        let online: bool = row.try_get("online")?;
        let enabled: bool = row.try_get("enabled")?;
        let generation: Option<String> = row.try_get("generation")?;
        let manifest: String = row.try_get("revision")?;
        let grants: Value = serde_json::from_str(row.try_get("grants")?)?;
        let requested: Value = row.try_get::<Option<&str>,_>("requested")?.map(serde_json::from_str).transpose()?.unwrap_or_else(||grants.clone());
        let approval:Value=row.try_get::<Option<&str>,_>("approval")?.map(serde_json::from_str).transpose()?.unwrap_or(Value::Null);
        let installed:bool=row.try_get("installed")?;
        let approved=approval["approved"].is_array();
        let status = if !installed {"unavailable"}else if !approved {"discovered"}else if online {"running"} else if !enabled {"disabled"} else if generation.is_some() {"failed"} else {"approved"};
        Ok(json!({"plugin_id":row.try_get::<String,_>("plugin")?,"name":row.try_get::<String,_>("namespace")?,
            "version":null,"status":status,"enabled":enabled,"revision":revision(&manifest,&json!([enabled,generation,online,grants,installed,approved,approval["deployment"]]))?,
            "manifest_hash":manifest,"requested_permissions":requested,"approved_permissions":grants,
            "error_category":if status=="failed" {json!("instance_unavailable")} else {Value::Null}}))
    }).collect::<Result<Vec<_>>>()?;
    if id.is_some() {
        items.into_iter().next().ok_or_else(|| "not_found".into())
    } else {
        Ok(json!(items))
    }
}
pub(crate) async fn observation(db: &mut SqliteConnection, args: &Value) -> Result<Value> {
    let id = args["plugin_id"].as_str().ok_or("validation_error")?;
    let generation: String = sqlx::query_scalar("SELECT generation FROM host_installations WHERE plugin=? AND online AND enabled AND EXISTS(SELECT 1 FROM json_each(grants) WHERE value='plugin.observe')")
        .bind(id).fetch_optional(&mut *db).await?.ok_or("observation_unavailable")?;
    super::domain::active(db, id, &generation, "plugin.observe").await?;
    Ok(json!({"generation":generation}))
}
