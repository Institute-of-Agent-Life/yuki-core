mod config;
mod identity;
mod operator;
mod store;

use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use clap::{Parser, Subcommand};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Notify;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Parser)]
#[command(version, about = "Yuki Rust core")]
struct Args {
    #[arg(long, global = true)]
    data: Option<PathBuf>,
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    Init,
    Migrate,
    Doctor,
    Run,
    Operator {
        #[command(subcommand)]
        command: operator::Action,
    },
}

async fn ready(State(store): State<store::Store>) -> (StatusCode, Json<serde_json::Value>) {
    let health = store.health().await;
    let ready = health.reader_ok && health.writer_accepting;
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(json!({"ready":ready})),
    )
}
async fn health(State(store): State<store::Store>) -> Json<store::Health> {
    Json(store.health().await)
}

async fn close_writer(store: &store::Store) {
    if let Err(error) = store.shutdown().await {
        eprintln!(
            "{}",
            json!({"error":"writer_close_unknown","detail":error.to_string()})
        );
        std::process::exit(1); // Keep owner held until the OS terminates all process handles.
    }
}

async fn execute(args: Args) -> Result<()> {
    let data = args.data.ok_or("--data is required")?;
    match args.command {
        Action::Init => {
            let owner = config::init(&data)?;
            let version = store::bootstrap(&owner.data).await?;
            println!(
                "{}",
                json!({"status":"initialized","schema_version":version})
            );
        }
        Action::Migrate => {
            config::Config::read(&data)?;
            let owner = config::Owner::acquire(&data)?;
            let version = store::bootstrap(&owner.data).await?;
            println!("{}", json!({"status":"migrated","schema_version":version}));
        }
        Action::Doctor => {
            let config = config::Config::read(&data)?;
            println!(
                "{}",
                json!({"configured":true,"format_version":config.format_version,
                "database_present":data.join("core.sqlite").is_file(),
                "external_probes":false,"mutation_attempted":false})
            );
        }
        Action::Operator { command } => operator::invoke(&data, command).await?,
        Action::Run => {
            let config = config::Config::read(&data)?;
            let owner = config::Owner::acquire(&data)?;
            #[cfg(unix)]
            let mut terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
            let listener = tokio::net::TcpListener::bind(config.listen).await?;
            let store = store::Store::open(&owner.data).await?;
            let stop = Arc::new(Notify::new());
            let control = match operator::listen(&owner.data, store.clone(), stop.clone()).await {
                Ok(control) => control,
                Err(error) => {
                    close_writer(&store).await;
                    return Err(error);
                }
            };
            println!(
                "{}",
                json!({"status":"ready","listen":listener.local_addr()?.to_string(),
                "operator":operator::endpoint(&owner.data)})
            );
            let router = Router::new()
                .route("/livez", get(|| async { Json(json!({"live":true})) }))
                .route("/readyz", get(ready))
                .route("/healthz", get(health))
                .with_state(store.clone());
            let signal = stop.clone();
            let service = axum::serve(listener, router).with_graceful_shutdown(async move {
                #[cfg(unix)]
                tokio::select! { _ = signal.notified() => {}, _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
                #[cfg(windows)]
                tokio::select! { _ = signal.notified() => {}, _ = tokio::signal::ctrl_c() => {} }
            }).await;
            control.abort();
            let _ = control.await;
            close_writer(&store).await;
            service?;
            drop(owner);
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(error) = execute(Args::parse()).await {
        eprintln!("{}", json!({"error":error.to_string()}));
        std::process::exit(1);
    }
}
