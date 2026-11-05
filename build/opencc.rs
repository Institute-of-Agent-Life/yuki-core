use std::{
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
};

fn collect(directory: &Path, extension: &str, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("native source directory") {
        let entry = entry.unwrap();
        if entry.file_name().to_string_lossy().starts_with("._") {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect(&path, extension, files);
        } else if path.extension().is_some_and(|ext| ext == extension) {
            files.push(path);
        }
    }
}
pub fn compile(out: &Path) {
    let archive = "native/opencc/opencc-1.4.2-t2s.tar.gz";
    let bridge = "native/opencc/bridge.cpp";
    println!("cargo:rerun-if-changed={archive}");
    println!("cargo:rerun-if-changed={bridge}");
    use sha2::{Digest, Sha256};
    let hash = fs::read_to_string("native/opencc/archive.sha256").unwrap();
    println!("cargo:rerun-if-changed=native/opencc/archive.sha256");
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(archive).unwrap())),
        hash.trim(),
        "OpenCC source digest mismatch"
    );
    let source = out.join(hash.trim());
    println!(
        "cargo:rustc-env=YUKI_OPENCC_RESOURCES={}",
        source.join("resources").display()
    );
    let input = flate2::read::GzDecoder::new(BufReader::new(File::open(archive).unwrap()));
    tar::Archive::new(input)
        .unpack(&source)
        .expect("verified native source archive");
    let core = source.join("src");
    let marisa = source.join("deps/marisa-0.3.1");
    let mut files = Vec::new();
    collect(&core, "cpp", &mut files);
    collect(&marisa.join("lib"), "cc", &mut files);
    files.sort();
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .warnings(false)
        .include(&core)
        .include(marisa.join("include"))
        .include(marisa.join("lib"))
        .include(source.join("deps/darts-clone-0.32h/include"))
        .include(source.join("deps/rapidjson-1.1.0"))
        .define("Opencc_BUILT_AS_STATIC", None)
        .define("OPENCC_VERSION", Some("\"1.4.2\""))
        .define("PKGDATADIR", Some("\"\""))
        .define("LOCALEDIR", Some("\"\""))
        .define("PACKAGE_NAME", Some("\"opencc\""))
        .define("OPENCC_SEGMENTATION_PLUGIN_DIR", Some("\"\""))
        .files(files)
        .file(bridge);
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        build.flag("/utf-8").flag("/EHsc");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu")
    {
        build.cpp_link_stdlib_static(true);
    }
    build.compile("yuki_opencc_1_4_2");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-lib=dl");
    }
}
