use super::catalog::{self, Catalog};
use crate::{
    Result,
    media::blob::{Root, Snapshot},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const SECRET_FILE: &str = "model_profiles.secrets.json";
const LIMIT: usize = 256 * 1024;

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Secrets {
    version: u32,
    keys: BTreeMap<String, String>,
}

pub(super) struct Bundle {
    pub configuration: Option<Snapshot>,
    pub secrets: Option<Snapshot>,
    pub keys: BTreeMap<String, String>,
    pub revision: u64,
}

pub(super) fn revision(configuration: Option<&[u8]>, secrets: Option<&[u8]>) -> u64 {
    if configuration.is_none() && secrets.is_none() {
        return 0;
    }
    let mut combined = Sha256::new();
    combined.update(Sha256::digest(configuration.unwrap_or_default()));
    combined.update(Sha256::digest(secrets.unwrap_or_default()));
    let digest = combined.finalize();
    // The original v3 transport uses the first 13 hex digits plus one, within JS's safe range.
    (digest[..7]
        .iter()
        .fold(0u64, |value, byte| (value << 8) | u64::from(*byte))
        >> 4)
        + 1
}

fn alias(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_uppercase())
        && name
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
}

fn validate_keys(keys: &BTreeMap<String, String>) -> Result<()> {
    if keys.iter().any(|(name, value)| {
        !alias(name)
            || value.trim().is_empty()
            || value.chars().count() > 8192
            || value.contains(['\n', '\r'])
    }) {
        return Err("validation_error".into());
    }
    Ok(())
}

pub(super) fn keys(bytes: Option<&[u8]>) -> Result<BTreeMap<String, String>> {
    let Some(bytes) = bytes else {
        return Ok(BTreeMap::new());
    };
    let body: Secrets = serde_json::from_slice(bytes).map_err(|_| "validation_error")?;
    if body.version != 1 {
        return Err("validation_error".into());
    }
    validate_keys(&body.keys)?;
    Ok(body.keys)
}

impl Bundle {
    pub(super) fn read(root: &Root, name: &str) -> Result<Self> {
        let configuration = root.snapshot_optional(name)?;
        let secrets = root.private_snapshot_optional(SECRET_FILE)?;
        if configuration.as_ref().is_some_and(|s| s.data.len() > LIMIT)
            || secrets.as_ref().is_some_and(|s| s.data.len() > LIMIT)
        {
            return Err("validation_error".into());
        }
        let keys = keys(secrets.as_ref().map(|snapshot| snapshot.data.as_slice()))?;
        let revision = revision(
            configuration.as_ref().map(|s| s.data.as_slice()),
            secrets.as_ref().map(|s| s.data.as_slice()),
        );
        Ok(Self {
            configuration,
            secrets,
            keys,
            revision,
        })
    }

    pub(super) fn prepare(&self, request: &Value) -> Result<Prepared> {
        catalog::validate("ModelsBody", request)?;
        if request["expected_revision"].as_u64() != Some(self.revision) {
            return Err("version_conflict".into());
        }
        let document = &request["document"];
        let configuration = match &self.configuration {
            Some(original) => Catalog::parse(
                std::str::from_utf8(&original.data).map_err(|_| "validation_error")?,
            )?
            .patch(document)?
            .into_bytes(),
            None => {
                let mut source = document.clone();
                remove_nulls(&mut source);
                let text = toml::to_string(&source).map_err(|_| "validation_error")?;
                Catalog::parse(&text)?;
                text.into_bytes()
            }
        };
        let mut keys = self.keys.clone();
        let references = referenced_keys(document);
        if let Some(updates) = request["api_keys"].as_object() {
            for (name, value) in updates {
                if !references.contains(name) {
                    return Err("validation_error".into());
                }
                keys.insert(
                    name.clone(),
                    value.as_str().ok_or("validation_error")?.into(),
                );
            }
        }
        validate_keys(&keys)?;
        let secrets = if keys == self.keys {
            self.secrets.as_ref().map(|s| s.data.clone())
        } else {
            Some(serde_json::to_vec(&Secrets { version: 1, keys })?)
        };
        if configuration.len() > LIMIT || secrets.as_ref().is_some_and(|s| s.len() > LIMIT) {
            return Err("validation_error".into());
        }
        Ok(Prepared {
            before_configuration: self.configuration.as_ref().map(|s| s.digest.clone()),
            before_secrets: self.secrets.as_ref().map(|s| s.digest.clone()),
            revision: revision(Some(&configuration), secrets.as_deref()),
            configuration,
            secrets,
        })
    }
}

// This internal write material contains server-only fields. Public receipts contain hashes only.
#[derive(Serialize, Deserialize)]
pub(super) struct Prepared {
    pub before_configuration: Option<String>,
    pub before_secrets: Option<String>,
    pub configuration: Vec<u8>,
    pub secrets: Option<Vec<u8>>,
    pub revision: u64,
}

pub(super) fn referenced_keys(document: &Value) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for profile in document["profiles"]
        .as_object()
        .into_iter()
        .flat_map(|profiles| profiles.values())
    {
        if let Some(name) = profile["api_key_env"].as_str() {
            keys.insert(name.into());
        }
    }
    for section in ["embedding", "vision", "asr", "search"] {
        for key in ["api_key_env", "tavily_api_key_env"] {
            if let Some(name) = document[section][key].as_str() {
                keys.insert(name.into());
            }
        }
    }
    for connection in document["semantic_observation"]["connections"]
        .as_object()
        .into_iter()
        .flat_map(|connections| connections.values())
    {
        if let Some(name) = connection["api_key_env"].as_str() {
            keys.insert(name.into());
        }
    }
    keys
}

fn remove_nulls(value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        object.retain(|_, value| !value.is_null());
        for child in object.values_mut() {
            remove_nulls(child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn original_bundle_revision_and_private_key_preparation_survive_atomic_replace() {
        let directory = std::env::temp_dir().join(format!(
            "yuki-profile-files-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let root = Root::open(&directory).unwrap();
        let empty = Bundle::read(&root, "model_profiles.toml").unwrap();
        assert_eq!(empty.revision, 0);
        root.stage("model_profiles.toml", catalog::ORIGINAL.as_bytes())
            .unwrap();
        let original = Bundle::read(&root, "model_profiles.toml").unwrap();
        let public = Catalog::parse(catalog::ORIGINAL).unwrap().public;
        let mut request = json!({"request_id":"00000000-0000-4000-8000-000000000001",
            "expected_revision":original.revision,"document":public});
        let no_op = original.prepare(&request).unwrap();
        assert_eq!(no_op.configuration, catalog::ORIGINAL.as_bytes());
        assert_eq!(no_op.revision, original.revision);
        request["document"]["profiles"]["primary"]["model"] = json!("after");
        request["api_keys"] = json!({"MODEL_TEST_KEY":"fixture-private-model-key"});
        let pending = original.prepare(&request).unwrap();
        assert_ne!(pending.revision, original.revision);
        root.stage("config-staged", &pending.configuration).unwrap();
        root.stage("secrets-staged", pending.secrets.as_ref().unwrap())
            .unwrap();
        root.replace("secrets-staged", SECRET_FILE).unwrap();
        // The retained before/after digests identify this partial physical write.
        let partial = Bundle::read(&root, "model_profiles.toml").unwrap();
        assert_eq!(
            partial.configuration.as_ref().unwrap().digest,
            pending.before_configuration.as_ref().unwrap().as_str()
        );
        assert_ne!(partial.revision, original.revision);
        root.replace("config-staged", "model_profiles.toml")
            .unwrap();
        let applied = Bundle::read(&root, "model_profiles.toml").unwrap();
        assert_eq!(applied.revision, pending.revision);
        assert_eq!(applied.keys["MODEL_TEST_KEY"], "fixture-private-model-key");
        let catalog = Catalog::parse(
            std::str::from_utf8(&applied.configuration.as_ref().unwrap().data).unwrap(),
        )
        .unwrap();
        assert!(
            !catalog
                .public
                .to_string()
                .contains("fixture-private-model-key")
        );
        assert!(!catalog.public.to_string().contains("header-canary"));
        assert!(applied.prepare(&request).is_err());
        request["expected_revision"] = json!(applied.revision);
        request["api_keys"] = json!({"UNREFERENCED_KEY":"rejected"});
        assert!(applied.prepare(&request).is_err());
        request["api_keys"] = json!({"MODEL_TEST_KEY":""});
        assert!(applied.prepare(&request).is_err());
        assert!(root.replace("model_profiles.toml", "../escape").is_err());
        assert_eq!(
            Bundle::read(&root, "model_profiles.toml").unwrap().revision,
            applied.revision
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                directory.join(SECRET_FILE),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap();
            assert!(Bundle::read(&root, "model_profiles.toml").is_err());
        }
        drop(root);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
