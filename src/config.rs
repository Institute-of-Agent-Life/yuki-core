use crate::Result;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub format_version: u32,
    pub listen: std::net::SocketAddr,
    #[serde(default)]
    pub plugins: Vec<crate::host::Install>,
    #[serde(default)]
    pub main: Option<crate::runtime::main::Config>,
    #[serde(default)]
    pub memory: crate::memory::embedding::Config,
    #[serde(default)]
    pub extraction: Option<crate::memory::extract::Config>,
    #[serde(default)]
    pub qq: Option<crate::gateway::qq::Config>,
    #[serde(default)]
    pub media: crate::media::Config,
}

impl Config {
    pub fn read(data: &Path) -> Result<Self> {
        let config: Self = toml::from_str(&fs::read_to_string(data.join("yuki.toml"))?)?;
        if config.format_version != 1 {
            return Err("unsupported configuration format".into());
        }
        config.memory.validate()?;
        if let Some(main) = &config.main {
            main.validate()?;
        }
        if let Some(extract) = &config.extraction {
            extract.validate()?;
        }
        Ok(config)
    }
}

pub fn init(data: &Path, mode: crate::memory::embedding::Mode) -> Result<Owner> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(data)?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(data)?;
    let owner = Owner::acquire(data)?;
    for name in ["yuki.toml", "SOUL.md", "core.sqlite"] {
        if data.join(name).exists() {
            return Err(format!("init preserves existing {name}; use run/migrate").into());
        }
    }
    create_file(
        &data.join("yuki.toml"),
        format!(
            "format_version = 1\nlisten = \"127.0.0.1:0\"\n\n[memory]\nmode = \"{}\"\n",
            mode.label()
        )
        .as_bytes(),
    )?;
    create_file(
        &data.join("SOUL.md"),
        include_bytes!("../character/SOUL.md"),
    )?;
    Ok(owner)
}

fn create_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

pub struct Owner {
    _file: File,
    pub data: PathBuf,
}

impl Owner {
    pub fn acquire(data: &Path) -> Result<Self> {
        let data = data.canonicalize()?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(data.join(".owner.lock"))?;
        file.try_lock()
            .map_err(|error| format!("data directory writer unavailable: {error}"))?;
        Ok(Self { _file: file, data })
    }
}
