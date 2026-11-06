//! Stopped-copy import. The source bundle is read-only; only captured legacy SQL
//! rows can be retired after the destination bundle is read back by its digest.
mod transform;
use super::{catalog::Catalog, files};
use crate::{Result, media::blob::Root};
use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Args)]
pub(crate) struct Options {
    #[arg(long)]
    profile: PathBuf,
    #[arg(long)]
    database: PathBuf,
    /// Explicit process environment capture; this command never reads ambient provider keys.
    #[arg(long)]
    environment: PathBuf,
    #[arg(long)]
    dotenv: Option<PathBuf>,
    #[arg(long)]
    revision: Option<u64>,
    #[arg(long)]
    inspect: bool,
    #[arg(long)]
    stopped_copy: bool,
    #[arg(long)]
    resume: bool,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
struct Fact {
    id: i64,
    version: i64,
    key: String,
    scope: String,
    mode: String,
    person: Option<String>,
    space: Option<String>,
    value_sha256: String,
}
struct RowValue {
    fact: Fact,
    value: Value,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    version: u32,
    profile: PathBuf,
    database: PathBuf,
    environment: PathBuf,
    dotenv: Option<PathBuf>,
    source_revision: u64,
    revision: u64,
    files: BTreeMap<String, String>,
    source_files: BTreeMap<String, String>,
    database_snapshot: Value,
    sqlite_build: Value,
    inputs: Vec<Fact>,
    environment_sources: BTreeMap<String, String>,
    rows: Vec<Fact>,
    consumed: Vec<String>,
    migrated: Vec<String>,
    comparison: Value,
}
fn digest(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn snapshot(path: &Path) -> Result<Vec<u8>> {
    let root = Root::open(path.parent().ok_or("invalid_source_path")?)?;
    Ok(root
        .snapshot(
            path.file_name()
                .and_then(|v| v.to_str())
                .ok_or("invalid_source_path")?,
        )?
        .data)
}
fn database_snapshot(path: &Path) -> Result<Value> {
    use std::io::Read;
    let mut members = vec![];
    for source in [
        path.to_owned(),
        PathBuf::from(format!("{}-wal", path.display())),
    ] {
        if !source.exists() {
            continue;
        }
        if fs::symlink_metadata(&source)?.file_type().is_symlink() {
            return Err("invalid_database_snapshot".into());
        }
        let mut file = fs::File::open(&source)?;
        let before = file.metadata()?;
        let mut hash = Sha256::new();
        let mut buf = [0u8; 65536];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
        }
        let after = file.metadata()?;
        if before.len() != after.len() || before.modified()? != after.modified()? {
            return Err("source_database_changed".into());
        }
        members.push(
            json!({"path":source,"bytes":before.len(),"sha256":format!("{:x}",hash.finalize())}),
        );
    }
    Ok(json!({"schema":"0156","files":members,"state":"stopped-copy-before-import"}))
}
async fn facts(db: &mut SqliteConnection) -> Result<Vec<RowValue>> {
    let versions: Vec<String> = sqlx::query_scalar("SELECT version_num FROM alembic_version")
        .fetch_all(&mut *db)
        .await?;
    if versions != ["0156"] {
        return Err("unsupported_legacy_schema".into());
    }
    let rows=sqlx::query("SELECT id,version,config_key,scope_type,value_json,apply_mode,canonical_person_id,canonical_space_id FROM runtime_config_overrides ORDER BY id").fetch_all(db).await?;
    let mut result = vec![];
    for r in rows {
        let key: String = r.get("config_key");
        if !transform::relevant(&key) {
            continue;
        }
        let raw: String = r.get("value_json");
        result.push(RowValue {
            fact: Fact {
                id: r.get("id"),
                version: r.get("version"),
                key,
                scope: r.get("scope_type"),
                mode: r.get("apply_mode"),
                person: r.get("canonical_person_id"),
                space: r.get("canonical_space_id"),
                value_sha256: digest(raw.as_bytes()),
            },
            value: serde_json::from_str(&raw).map_err(|_| "invalid_legacy_override")?,
        });
    }
    Ok(result)
}
fn public(plan: &Journal, status: &str, error: Option<&str>, out: &Path) -> Value {
    json!({"ok":status=="reconciled","partial":status!="reconciled","status":status,"error_category":error,
      "source_revision":plan.source_revision,"revision":plan.revision,"apply_mode":"restart",
      "source_profile":plan.profile,"source_database":plan.database,"output":out,
      "source_files":plan.source_files,"files":plan.files,"original_rows":plan.rows,
      "database_snapshot":plan.database_snapshot,"sqlite_build":plan.sqlite_build,
      "input_rows":plan.inputs,"environment_sources":plan.environment_sources,
      "consumed_env_fields":plan.consumed,"migrated":plan.migrated,"effective_parameters":plan.comparison,
      "paid_requests":0,"gateway_started":false,"plugins_started":false,"dotenv_modified":false})
}
fn ensure_file(root: &Root, name: &str, expected: &str, data: &[u8]) -> Result<()> {
    if digest(data) != expected {
        return Err("import_intent_changed".into());
    }
    if let Some(old) = root.snapshot_optional(name)? {
        if old.digest != expected {
            return Err("destination_changed".into());
        }
    } else {
        root.stage(name, data)?;
    }
    if root.snapshot(name)?.digest != expected {
        return Err("destination_unconfirmed".into());
    }
    Ok(())
}
async fn barrier(phase: &str) -> Result<()> {
    if let Ok(path) = std::env::var("YUKI_TEST_MODEL_IMPORT_BARRIER") {
        let path = PathBuf::from(path);
        fs::write(path.join(phase), b"reached")?;
        while !path.join(format!("{phase}.release")).exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    Ok(())
}
pub(crate) async fn run(out: &Path, options: Options) -> Result<Value> {
    if !options.stopped_copy {
        return Err("stopped_copy_required".into());
    }
    let profile = options.profile.canonicalize()?;
    let database = options.database.canonicalize()?;
    let environment = options.environment.canonicalize()?;
    let dotenv = options
        .dotenv
        .as_ref()
        .map(|p| p.canonicalize())
        .transpose()?;
    let lock_path = PathBuf::from(format!("{}.application.lock", database.display()));
    if fs::symlink_metadata(&lock_path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("invalid_source_lock".into());
    }
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    lock.try_lock().map_err(|_| "application_active")?;
    let source = Root::open(profile.parent().ok_or("invalid_source_path")?)?;
    let name = profile
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or("invalid_source_path")?
        .to_owned();
    let bundle = files::Bundle::read(&source, &name)?;
    if !options.inspect && Some(bundle.revision) != options.revision {
        return Err("source_revision_changed".into());
    }
    let raw = bundle
        .configuration
        .as_ref()
        .ok_or("source_profile_missing")?;
    let env_bytes = snapshot(&environment)?;
    let dotenv_bytes = dotenv.as_ref().map(|p| snapshot(p)).transpose()?;
    let mut source_files = BTreeMap::from([
        ("profile".into(), raw.digest.clone()),
        ("environment".into(), digest(&env_bytes)),
    ]);
    if let Some(ref v) = bundle.secrets {
        source_files.insert("sidecar".into(), v.digest.clone());
    }
    if let Some(ref v) = dotenv_bytes {
        source_files.insert("dotenv".into(), digest(v));
    }
    let database_snapshot = database_snapshot(&database)?;
    let mut db = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(&database)
            .create_if_missing(false)
            .busy_timeout(std::time::Duration::from_secs(2)),
    )
    .await?;
    // Never bootstrap or migrate the legacy database, and never start an owner runtime.
    let captured = facts(&mut db).await?;
    let sqlite_version: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(&mut db)
        .await?;
    let compile_options: Vec<String> = sqlx::query_scalar("PRAGMA compile_options")
        .fetch_all(&mut db)
        .await?;
    let sqlite_build = json!({"sqlite_version":sqlite_version,"compile_options":compile_options,"schema_version":null,"legacy_schema_version":"0156","schema_kind":"legacy-copy"});
    if options.inspect {
        return Ok(
            json!({"ok":true,"status":"inspected","revision":bundle.revision,"source_profile":profile,"source_database":database,"source_files":source_files,"database_snapshot":database_snapshot,"sqlite_build":sqlite_build,"input_rows":captured.iter().map(|r|&r.fact).collect::<Vec<_>>(),"destination_written":false,"sql_retired":false}),
        );
    }
    let plan;
    let destination;
    if options.resume {
        destination = Root::open(out)?;
        plan = serde_json::from_slice::<Journal>(&destination.snapshot("import-intent.json")?.data)
            .map_err(|_| "invalid_import_intent")?;
        if plan.version != 1
            || plan.profile != profile
            || plan.database != database
            || plan.environment != environment
            || plan.dotenv != dotenv
            || Some(plan.source_revision) != options.revision
            || plan.source_files != source_files
        {
            return Err("import_source_changed".into());
        }
    } else {
        if out.exists() {
            return Err("destination_exists_use_original_resume".into());
        }
        let original =
            Catalog::parse(std::str::from_utf8(&raw.data).map_err(|_| "invalid_profile")?)?;
        let mut changed = transform::convert(
            &original,
            &bundle.keys,
            &captured,
            &env_bytes,
            dotenv_bytes.as_deref(),
        )?;
        let raw_profiles = changed.document["profiles"].clone();
        for profile in changed.document["profiles"]
            .as_object_mut()
            .ok_or("invalid_profiles")?
            .values_mut()
        {
            *profile = super::catalog::public_fields(
                profile,
                super::catalog::schema("ModelProfileDocument"),
            );
        }
        let mut prepared=bundle.prepare(&json!({"request_id":"00000000-0000-4000-8000-000000000057","expected_revision":bundle.revision,"document":changed.document,"api_keys":changed.keys}))?;
        let mut text: toml_edit::DocumentMut =
            std::str::from_utf8(&prepared.configuration)?.parse()?;
        for (name, profile) in raw_profiles.as_object().ok_or("invalid_profiles")? {
            if original.raw["profiles"].get(name).is_none() {
                let fragment: toml_edit::DocumentMut =
                    toml::to_string(&json!({"profile":profile}))?.parse()?;
                text["profiles"][name] = fragment["profile"].clone();
            }
        }
        prepared.configuration = text.to_string().into_bytes();
        let saved = Catalog::parse(std::str::from_utf8(&prepared.configuration)?)?;
        transform::verify_saved(&saved.raw, &changed)?;
        prepared.revision =
            files::revision(Some(&prepared.configuration), prepared.secrets.as_deref());
        let mut files = BTreeMap::from([(
            "model_profiles.toml".into(),
            digest(&prepared.configuration),
        )]);
        if let Some(ref bytes) = prepared.secrets {
            files.insert(files::SECRET_FILE.into(), digest(bytes));
        }
        plan = Journal {
            version: 1,
            profile,
            database,
            environment,
            dotenv,
            source_revision: bundle.revision,
            revision: prepared.revision,
            source_files,
            database_snapshot,
            sqlite_build,
            inputs: captured.iter().map(|r| r.fact.clone()).collect(),
            environment_sources: changed.environment_sources,
            files,
            rows: captured
                .iter()
                .filter(|r| transform::retired(&r.fact.key))
                .map(|r| r.fact.clone())
                .collect(),
            consumed: changed.consumed,
            migrated: changed.migrated,
            comparison: changed.comparison,
        };
        // Validation and all source comparisons precede any destination write.
        fs::create_dir(out)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(out, fs::Permissions::from_mode(0o700))?;
        }
        destination = Root::open(out)?;
        destination.stage("pending-profile", &prepared.configuration)?;
        if let Some(ref bytes) = prepared.secrets {
            destination.stage("pending-secrets", bytes)?;
        }
        destination.stage("import-intent.json", &serde_json::to_vec(&plan)?)?;
    }
    let publish = (|| -> Result<()> {
        for (file, hash) in &plan.files {
            let pending = if file == files::SECRET_FILE {
                "pending-secrets"
            } else {
                "pending-profile"
            };
            ensure_file(
                &destination,
                file,
                hash,
                &destination.snapshot(pending)?.data,
            )?;
        }
        if files::Bundle::read(&destination, "model_profiles.toml")?.revision != plan.revision {
            return Err("destination_unconfirmed".into());
        }
        Ok(())
    })();
    if publish.is_err() {
        return Ok(public(
            &plan,
            "partial",
            Some("destination_changed_or_unconfirmed"),
            out,
        ));
    }
    barrier("files-published").await?;
    // A user's edit after publication must be detected before touching any SQL row.
    let after = files::Bundle::read(&destination, "model_profiles.toml")?;
    let current = files::Bundle::read(&source, &name)?;
    if after.revision != plan.revision || current.revision != plan.source_revision {
        return Ok(public(&plan, "partial", Some("bundle_changed"), out));
    }
    if let Err(error) = retire(&mut db, &plan).await {
        let category = if error.to_string() == "legacy_changed" {
            "legacy_changed"
        } else {
            "legacy_retirement_unconfirmed"
        };
        return Ok(public(&plan, "partial", Some(category), out));
    }
    barrier("sql-retired").await?;
    let result = public(&plan, "reconciled", None, out);
    let bytes = serde_json::to_vec(&result)?;
    ensure_file(&destination, "reconciliation.json", &digest(&bytes), &bytes)?;
    Ok(result)
}

async fn retire(db: &mut SqliteConnection, plan: &Journal) -> Result<()> {
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    let fresh = facts(&mut tx).await?;
    if fresh.iter().any(|r| !plan.inputs.contains(&r.fact))
        || plan
            .inputs
            .iter()
            .filter(|r| !transform::retired(&r.key))
            .any(|r| !fresh.iter().any(|v| v.fact == *r))
    {
        return Err("legacy_changed".into());
    }
    let mut present = 0;
    for expected in &plan.rows {
        match fresh.iter().find(|r| r.fact.id == expected.id) {
            Some(row) if row.fact == *expected => present += 1,
            Some(_) => return Err("legacy_changed".into()),
            None => {}
        }
    }
    // Retirement is one transaction. A mixed row set has no confirmed origin.
    if present != 0 && present != plan.rows.len() {
        return Err("legacy_retirement_unconfirmed".into());
    }
    if present != 0 {
        for expected in &plan.rows {
            let n=sqlx::query("DELETE FROM runtime_config_overrides WHERE id=? AND version=? AND config_key=? AND scope_type=?")
                .bind(expected.id).bind(expected.version).bind(&expected.key).bind(&expected.scope).execute(&mut *tx).await?.rows_affected();
            if n != 1 {
                return Err("legacy_changed".into());
            }
        }
    }
    if tx.commit().await.is_err() {
        return Err("legacy_retirement_unconfirmed".into());
    }

    Ok(())
}
