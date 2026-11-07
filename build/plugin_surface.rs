use sha2::{Digest, Sha256};
use std::{env, fs, path::Path};

pub fn compile(output: &Path) {
    println!("cargo:rerun-if-env-changed=YUKI_PLUGIN_SURFACE_PLAN");
    println!("cargo:rerun-if-env-changed=YUKI_PLUGIN_SURFACE_SHA256");
    let selected = match env::var_os("YUKI_PLUGIN_SURFACE_PLAN") {
        Some(path) => {
            let path = Path::new(&path);
            println!("cargo:rerun-if-changed={}", path.display());
            let data = fs::read(path).expect("selected plugin surface plan");
            let expected = env::var("YUKI_PLUGIN_SURFACE_SHA256")
                .expect("selected plugin surface digest is required");
            assert_eq!(format!("{:x}", Sha256::digest(&data)), expected);
            let value = String::from_utf8(data).expect("UTF-8 plugin surface plan");
            format!("Some({value:?})")
        }
        None => {
            assert!(env::var_os("YUKI_PLUGIN_SURFACE_SHA256").is_none());
            "None".into()
        }
    };
    fs::write(
        output.join("plugin_surface.rs"),
        format!("pub const SELECTED: Option<&str> = {selected};\n"),
    )
    .expect("generated plugin surface constant");
}
