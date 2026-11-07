mod config;
mod context;
mod gateway;
mod history;
mod host;
mod identity;
mod ingress;
mod media;
mod memory;
mod model;
mod operator;
mod privacy;
mod runtime;
mod social;
mod store;
mod tools;
mod work;

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
    /// Use the witnessed plugin wire 1.0 for a new installation or its existing volume.
    #[arg(long, global = true)]
    plugin_base_1: bool,
    #[command(subcommand)]
    command: Action,
}
#[derive(Subcommand)]
enum Action {
    /// Describe the compiled stable plugin surface without opening a data directory.
    PluginSurface,
    Init {
        #[arg(long, value_enum, default_value = "hybrid")]
        memory_mode: memory::embedding::Mode,
    },
    Migrate,
    ImportModels(model::profile_store::legacy::Options),
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
async fn health(State(store): State<store::Store>) -> Json<serde_json::Value> {
    let mut health = json!(store.health().await);
    health["memory"] = match memory::embedding::status(&store).await {
        Ok(v) => json!({"mode":v["mode"],"degraded":v["degraded"].as_bool().unwrap_or(false)}),
        Err(_) => json!({"degraded":true}),
    };
    if store
        .profile_files
        .as_ref()
        .is_some_and(|service| service.connections_suspended())
    {
        health["memory"]["degraded"] = json!(true);
        health["memory"]["reason"] = json!("original_configuration_pending_restart");
    }
    Json(health)
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
    if matches!(args.command, Action::PluginSurface) {
        println!("{}", host::grants::profile_description());
        return Ok(());
    }
    let data = args.data.ok_or("--data is required")?;
    let marked = data.join("plugin-base-1.0").try_exists()?;
    let stable = host::grants::selected_build()
        || args.plugin_base_1
        || std::env::current_exe()?.file_stem() == Some(std::ffi::OsStr::new("yuki-base"));
    if stable && !marked && !matches!(args.command, Action::Init { .. }) {
        return Err("stable plugin base requires its own initialized data directory".into());
    }
    host::grants::select_base(stable || marked)?;
    match args.command {
        Action::PluginSurface => unreachable!("handled before acquiring data"),
        Action::Init { memory_mode } => {
            let owner = config::init(&data, memory_mode)?;
            let version = store::bootstrap(&owner.data).await?;
            println!(
                "{}",
                json!({"status":"initialized","schema_version":version})
            );
        }
        Action::ImportModels(options) => {
            let result = model::profile_store::legacy::run(&data, options).await?;
            println!("{}", result);
            if result["ok"] != true {
                std::process::exit(1);
            }
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
            let mut config = config::Config::read(&data)?;
            let owner = config::Owner::acquire(&data)?;
            #[cfg(unix)]
            let mut terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
            let listener = tokio::net::TcpListener::bind(config.listen).await?;
            let mut store = store::Store::open(&owner.data).await?;
            store.main = config.main.clone().map(Arc::new);
            store.control = Some(operator::domain::Service::open(
                &owner.data,
                config.operator_registry_file.as_deref(),
                config
                    .main
                    .as_ref()
                    .map(|c| c.name.as_str())
                    .unwrap_or("Yuki"),
                &config.bot_aliases,
            )?);
            operator::domain::invoke(&store, operator::domain::Request::Start).await?;
            store.work_segments = config.work_segments;
            if let Some(path) = &config.model_profiles_file {
                let baseline = store
                    .main
                    .as_deref()
                    .cloned()
                    .ok_or("model profiles require Main runtime policy")?;
                store.profile_files = Some(model::profile_store::Service::open(
                    &owner.data,
                    path,
                    config.operator_registry_file.as_deref(),
                    baseline,
                )?);
                let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM core_meta WHERE key='model-profile-active-operation')")
                    .fetch_one(store.readers()).await?;
                Arc::get_mut(
                    store
                        .profile_files
                        .as_mut()
                        .ok_or("profile_service_unavailable")?,
                )
                .ok_or("profile_service_already_shared")?
                .configure_connections(&mut config, pending)?;
                model::profile_store::invoke(&store, model::profile_store::Request::Start).await?;
                store.main = model::profile_store::runtime::current(&store).await?;
            }
            store.media = Some(media::Runtime::open(&owner.data, config.media)?);
            media::recover(&store).await?;
            let vectors = memory::embedding::start(&mut store, config.memory).await?;
            let extraction = memory::extract::start(store.clone(), config.extraction).await?;
            store.search = model::search::start(config.search)?;
            let qq = gateway::qq::Hub::new(store.clone(), config.qq, &owner.data).await?;
            host::Hub::start(&mut store, config.plugins, &owner.data, qq.clone()).await?;
            let stop = Arc::new(Notify::new());
            let control = match operator::listen(
                &owner.data,
                store.clone(),
                stop.clone(),
                qq.clone(),
            )
            .await
            {
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
                .with_state(store.clone())
                .merge(qq.router())
                .merge(host::personal::router(store.clone()))
                .merge(model::profile_store::http::router(store.clone()));
            let router = router.merge(operator::domain::http::router(store.clone()));
            let media = media::start(store.clone());
            let memory = memory::start(store.clone(), qq.clone());
            let runner = runtime::main::start(store.clone(), owner.data.clone(), qq.clone())?;
            let history = history::start(store.clone());
            if let Some(domain) = &store.control {
                for name in ["work", "automation", "subagents"] {
                    domain.worker(name, store.main.is_some(), runner.as_ref());
                }
                domain.worker("history", true, Some(&history));
                domain.worker("media", true, Some(&media));
                domain.worker("memory_delivery", true, Some(&memory));
                domain.worker(
                    "memory_extraction",
                    extraction.is_some(),
                    extraction.as_ref(),
                );
                domain.worker("memory_vectors", vectors.is_some(), vectors.as_ref());
            }
            let signal = stop.clone();
            let draining = store.draining_http.clone();
            let service = axum::serve(listener, router).with_graceful_shutdown(async move {
                #[cfg(unix)]
                tokio::select! { _ = signal.notified() => {}, _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
                #[cfg(windows)]
                tokio::select! { _ = signal.notified() => {}, _ = tokio::signal::ctrl_c() => {} }
                draining.send_replace(true);
                qq.shutdown();
            }).await;
            if let Some(host) = &store.host {
                host.stop().await;
            }
            if let Some(worker) = vectors {
                worker.abort();
                let _ = worker.await;
            }
            media.abort();
            let _ = media.await;
            if let Some(worker) = extraction {
                worker.abort();
                let _ = worker.await;
            }
            memory.abort();
            let _ = memory.await;
            history.abort();
            let _ = history.await;
            if let Some(runner) = runner {
                runner.abort();
                let _ = runner.await;
            }
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
