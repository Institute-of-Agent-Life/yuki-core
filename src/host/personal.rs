mod artifacts;
mod connector;
pub(crate) mod credentials;
mod domains;
mod http;
mod native;
mod receipts;
mod selection;
mod stream;
mod upload_body;
mod uploads;
mod views;
mod work;
use crate::{
    Result,
    store::{Mutation, Store},
};
use credentials::Purpose;
pub use http::router;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqliteConnection;
#[derive(clap::Subcommand, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "method", rename_all = "snake_case")]
pub enum Operator {
    Issue {
        owner: String,
        request_id: String,
        purpose: Purpose,
        #[arg(long)]
        source_stage: Option<String>,
    },
    Revoke {
        device: String,
    },
}
pub enum Request {
    Operator(Operator),
    Pair {
        purpose: Purpose,
        code_hash: String,
        label: String,
    },
    Entry {
        token_hash: String,
        selection: selection::Selection,
        purpose: Purpose,
        command: Command,
    },
}
pub enum Command {
    ConnectionProof(String),
    Issue(String, Purpose),
    Revoke(String),
    Work(String, Value, bool),
    Proof(String, String, bool),
    Message(Value),
    Upload(uploads::Spec, String, std::fs::File),
    Correct(i64, Value),
    Schedule(i64, String, Value),
}
pub async fn write(store: &Store, r: Request) -> Result<Value> {
    store.write(Mutation::Personal(Box::new(r))).await
}
pub async fn apply(conn: &mut SqliteConnection, r: &Request) -> Result<Value> {
    match r {
        Request::Operator(Operator::Issue {
            owner,
            request_id,
            purpose,
            source_stage,
        }) => {
            credentials::issue(
                conn,
                "operator",
                owner,
                request_id,
                *purpose,
                source_stage.as_deref(),
            )
            .await
        }
        Request::Operator(Operator::Revoke { device }) => {
            credentials::revoke(conn, device, None).await
        }
        Request::Pair {
            purpose,
            code_hash,
            label,
        } => credentials::pair(conn, code_hash, label, *purpose).await,
        Request::Entry {
            token_hash,
            selection,
            purpose,
            command,
        } => {
            let who = selection::authenticate(conn, token_hash, *purpose, selection, true).await?;
            match command {
                Command::ConnectionProof(plugin) if *purpose == Purpose::Connector => {
                    connector::proof(conn, &who, plugin).await
                }
                Command::Proof(plugin, w, native)
                    if *native == (*purpose == Purpose::Connector) =>
                {
                    native::proof(conn, &who, plugin, w, *native).await
                }
                _ if *purpose != Purpose::Chat => Err("unauthenticated".into()),
                Command::Issue(op, p) => {
                    if receipts::find(conn, &who, op).await?.is_some() {
                        return Err("request_conflict".into());
                    }
                    let scope = who.request_scope();
                    let binding: String =
                        sqlx::query_scalar("SELECT actor_stage FROM entries WHERE id=?")
                            .bind(&who.entry)
                            .fetch_one(&mut *conn)
                            .await?;
                    credentials::issue_with_scope(
                        conn,
                        &who.device,
                        &who.request_person,
                        op,
                        *p,
                        Some(&binding),
                        Some(scope),
                    )
                    .await
                }
                Command::Revoke(id) => credentials::revoke(conn, id, Some(&who.root)).await,
                Command::Work(id, v, cancel) => work::apply(conn, &who, id, v, *cancel).await,
                Command::Correct(n, v) => domains::correct(conn, &who, *n, v).await,
                Command::Schedule(n, action, v) => {
                    domains::change_schedule(conn, &who, *n, action, v).await
                }
                Command::Upload(spec, entry, file) => {
                    uploads::register(conn, &who, spec, entry, file).await
                }
                Command::Message(v) => work::message(conn, &who, v).await,
                _ => Err("scope_unavailable".into()),
            }
        }
    }
}

pub(crate) async fn callback(
    conn: &mut SqliteConnection,
    plugin: &str,
    generation: &str,
    method: &str,
    args: &Value,
) -> Result<Value> {
    if matches!(method, "device.connect" | "device.check" | "device.close") {
        connector::callback(conn, plugin, generation, method, args).await
    } else {
        native::callback(conn, plugin, generation, method, args).await
    }
}
