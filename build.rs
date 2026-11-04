use std::{collections::BTreeMap, env, fs, path::PathBuf};

fn main() {
    let directory = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("migrations");
    println!("cargo:rerun-if-changed=migrations");
    let mut migrations = BTreeMap::new();
    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let name = name.to_str().expect("UTF-8 migration filename");
        // AppleDouble contains filesystem metadata, not another SQL migration.
        if name.starts_with("._")
            || !name.ends_with(".sql")
            || !entry.file_type().unwrap().is_file()
        {
            continue;
        }
        let (version, description) = name
            .trim_end_matches(".sql")
            .split_once('_')
            .expect("VERSION_description.sql");
        let version: i64 = version.parse().expect("integer migration version");
        assert!(version > 0, "migration version must be positive");
        assert!(
            migrations
                .insert(version, (description.replace('_', " "), entry.path()))
                .is_none(),
            "duplicate migration version"
        );
    }
    assert!(!migrations.is_empty(), "no migrations found");
    let mut generated = String::from("vec![\n");
    for (version, (description, path)) in migrations {
        generated.push_str(&format!(
            "sqlx::migrate::Migration::new({version}, {description:?}.into(), sqlx::migrate::MigrationType::Simple, include_str!({path:?}).into(), false),\n"
        ));
    }
    generated.push_str("]\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("migrations.rs"),
        generated,
    )
    .unwrap();
}
