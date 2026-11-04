use crate::{
    Result,
    store::{Mutation, Store},
};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::Notify;
use tokio::task::{JoinHandle, JoinSet};

#[derive(Subcommand)]
pub enum Action {
    Host {
        #[command(subcommand)]
        command: crate::host::Action,
    },
    Tools {
        #[command(subcommand)]
        command: crate::tools::Action,
    },
    Memory {
        #[command(subcommand)]
        command: crate::memory::Action,
    },
    Work {
        #[command(subcommand)]
        command: crate::work::signal::Action,
    },
    History {
        #[command(subcommand)]
        command: crate::history::Action,
    },
    Social {
        #[command(subcommand)]
        command: crate::social::Action,
    },
    Main {
        #[command(subcommand)]
        command: crate::runtime::main::Action,
    },
    Privacy {
        #[command(subcommand)]
        command: crate::privacy::Action,
    },
    Model {
        #[command(subcommand)]
        command: crate::model::attempt::Action,
    },
    Effect {
        #[command(subcommand)]
        command: crate::runtime::effects::Action,
    },
    Qq {
        #[command(subcommand)]
        command: crate::gateway::qq::Action,
    },
    Ingress {
        #[command(subcommand)]
        command: crate::ingress::admit::Action,
    },
    Entry {
        #[command(subcommand)]
        command: crate::identity::entry::Action,
    },
    Account {
        #[command(subcommand)]
        command: crate::identity::assignment::Action,
    },
    Person {
        #[command(subcommand)]
        command: crate::identity::person::Action,
    },
    GetLabel,
    SetLabel {
        value: String,
    },
    DatabaseInfo,
    Shutdown,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Host {
        command: crate::host::Action,
    },
    Tools {
        command: crate::tools::Action,
    },
    Memory {
        command: crate::memory::Action,
    },
    Work {
        command: crate::work::signal::Action,
    },
    History {
        command: crate::history::Action,
    },
    Social {
        command: crate::social::Action,
    },
    Main {
        command: crate::runtime::main::Action,
    },
    Privacy {
        command: crate::privacy::Action,
    },
    Model {
        command: crate::model::attempt::Action,
    },
    Effect {
        command: crate::runtime::effects::Action,
    },
    Qq {
        command: crate::gateway::qq::Action,
    },
    Ingress {
        command: crate::ingress::admit::Action,
    },
    Entry {
        command: crate::identity::entry::Action,
    },
    Account {
        command: crate::identity::assignment::Action,
    },
    Person {
        command: crate::identity::person::Action,
    },
    GetLabel,
    SetLabel {
        value: String,
    },
    DatabaseInfo,
    Shutdown,
}

pub fn endpoint(data: &Path) -> String {
    #[cfg(unix)]
    {
        data.join("operator.sock").to_string_lossy().into_owned()
    }
    #[cfg(windows)]
    {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(data.to_string_lossy().to_lowercase().as_bytes());
        format!(r"\\.\pipe\yuki-core-{hash:x}")
    }
}

async fn handle(
    stream: impl AsyncRead + AsyncWrite + Unpin,
    store: Store,
    stop: Arc<Notify>,
    qq: crate::gateway::qq::Hub,
) -> Result<()> {
    let mut stream = BufReader::new(stream);
    let mut line = String::new();
    if stream.read_line(&mut line).await? == 0 {
        return Ok(());
    }
    let request: Request = serde_json::from_str(&line)?;
    let shutdown = matches!(request, Request::Shutdown);
    let result = match request {
        Request::Person { command } => crate::identity::person::invoke(&store, command).await,
        Request::Account { command } => crate::identity::assignment::invoke(&store, command).await,
        Request::Entry { command } => crate::identity::entry::invoke(&store, command).await,
        Request::Ingress { command } => crate::ingress::admit::invoke(&store, command).await,
        Request::Qq { command } => qq.operator(command).await,
        Request::Effect { command } => crate::runtime::effects::invoke(&store, command).await,
        Request::Host { command } => match &store.host {
            Some(h) => h.operator(&store, command).await,
            None => Err("plugin host not configured".into()),
        },
        Request::Tools { command } => crate::tools::invoke(&store, command).await,
        Request::Memory { command } => crate::memory::invoke(&store, command).await,
        Request::Work { command } => crate::work::signal::invoke(&store, command).await,
        Request::History { command } => crate::history::invoke(&store, command).await,
        Request::Social { command } => crate::social::invoke(&store, &qq, command).await,
        Request::Main { command } => crate::runtime::main::invoke(&store, command).await,
        Request::Privacy { command } => crate::privacy::invoke(&store, command).await,
        Request::Model { command } => crate::model::attempt::invoke(&store, command).await,
        Request::GetLabel => store.label().await,
        Request::SetLabel { value } => store.write(Mutation::SetLabel(value)).await,
        Request::DatabaseInfo => store.database_info().await,
        Request::Shutdown => Ok(json!({"status":"stopping"})),
    };
    let value = match result {
        Ok(value) => json!({"result":value}),
        Err(error) => json!({"error":error.to_string()}),
    };
    let mut bytes = serde_json::to_vec(&value)?;
    bytes.push(b'\n');
    stream.get_mut().write_all(&bytes).await?;
    stream.get_mut().flush().await?;
    if shutdown {
        stop.notify_one();
    }
    Ok(())
}

async fn send(stream: impl AsyncRead + AsyncWrite + Unpin, request: Request) -> Result<()> {
    let mut stream = BufReader::new(stream);
    let mut bytes = serde_json::to_vec(&request)?;
    bytes.push(b'\n');
    stream.get_mut().write_all(&bytes).await?;
    stream.get_mut().flush().await?;
    let mut line = String::new();
    stream.read_line(&mut line).await?;
    let response: Value = serde_json::from_str(&line)?;
    if let Some(error) = response.get("error") {
        return Err(format!("operator: {error}").into());
    }
    println!("{}", response["result"]);
    Ok(())
}

pub async fn invoke(data: &Path, action: Action) -> Result<()> {
    let request = match action {
        Action::Person { command } => Request::Person { command },
        Action::Account { command } => Request::Account { command },
        Action::Entry { command } => Request::Entry { command },
        Action::Ingress { command } => Request::Ingress { command },
        Action::Qq { command } => Request::Qq { command },
        Action::Effect { command } => Request::Effect { command },
        Action::Host { command } => Request::Host { command },
        Action::Tools { command } => Request::Tools { command },
        Action::Memory { command } => Request::Memory { command },
        Action::Work { command } => Request::Work { command },
        Action::History { command } => Request::History { command },
        Action::Social { command } => Request::Social { command },
        Action::Main { command } => Request::Main { command },
        Action::Privacy { command } => Request::Privacy { command },
        Action::Model { command } => Request::Model { command },
        Action::GetLabel => Request::GetLabel,
        Action::SetLabel { value } => Request::SetLabel { value },
        Action::DatabaseInfo => Request::DatabaseInfo,
        Action::Shutdown => Request::Shutdown,
    };
    let endpoint = endpoint(&data.canonicalize()?);
    #[cfg(unix)]
    let stream = tokio::net::UnixStream::connect(endpoint).await?;
    #[cfg(windows)]
    let stream = {
        use tokio::net::windows::named_pipe::ClientOptions;
        let connect = async {
            loop {
                match ClientOptions::new().open(&endpoint) {
                    Ok(stream) => break Ok::<_, std::io::Error>(stream),
                    Err(error) if error.raw_os_error() == Some(231) => {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await
                    }
                    Err(error) => break Err(error),
                }
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), connect).await??
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), send(stream, request)).await?
}

#[cfg(unix)]
pub async fn listen(
    data: &Path,
    store: Store,
    stop: Arc<Notify>,
    qq: crate::gateway::qq::Hub,
) -> Result<JoinHandle<()>> {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    let path = data.join("operator.sock");
    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        if !metadata.file_type().is_socket() {
            return Err("operator path occupied by a non-socket file".into());
        }
        std::fs::remove_file(&path)?;
    }
    let listener = tokio::net::UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    struct SocketPath(std::path::PathBuf);
    impl Drop for SocketPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    Ok(tokio::spawn(async move {
        let _path = SocketPath(path);
        let mut peers = JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => match accepted {
                    Ok((stream, _)) => { peers.spawn(handle(stream, store.clone(), stop.clone(),qq.clone())); },
                    Err(_) => break,
                },
                _ = peers.join_next(), if !peers.is_empty() => {},
            }
        }
    }))
}

#[cfg(windows)]
mod windows_acl;

#[cfg(windows)]
pub async fn listen(
    data: &Path,
    store: Store,
    stop: Arc<Notify>,
    qq: crate::gateway::qq::Hub,
) -> Result<JoinHandle<()>> {
    let endpoint = endpoint(data);
    let mut server = windows_acl::pipe(&endpoint, true)?;
    Ok(tokio::spawn(async move {
        let mut peers = JoinSet::new();
        loop {
            tokio::select! {
                connected = server.connect() => {
                    if connected.is_err() { break; }
                    let next = match windows_acl::pipe(&endpoint, false) { Ok(next) => next, Err(_) => break };
                    let stream = std::mem::replace(&mut server, next);
                    peers.spawn(handle(stream, store.clone(), stop.clone(),qq.clone()));
                },
                _ = peers.join_next(), if !peers.is_empty() => {},
            }
        }
    }))
}
