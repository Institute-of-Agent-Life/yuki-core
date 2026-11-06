use super::{
    Command, Request,
    credentials::{self, Purpose},
    views, work, write,
};
use crate::{Result, store::Store};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::LazyLock;
fn reply(res: Result<Value>, envelope: bool) -> Response {
    match res {
        Ok(value) => Json(if envelope {
            json!({"data":value})
        } else {
            value
        })
        .into_response(),
        Err(e) => {
            let reason = e.to_string();
            let (code, k) = match reason.as_str() {
                "unauthenticated" => (StatusCode::UNAUTHORIZED, "unauthenticated"),
                "forbidden" => (StatusCode::FORBIDDEN, "forbidden"),
                "not_found" | "device_not_found" | "original_work_unavailable" => {
                    (StatusCode::NOT_FOUND, "not_found")
                }
                "request_conflict" | "native_event_conflict" => {
                    (StatusCode::CONFLICT, "request_conflict")
                }
                "precondition_failed" | "source_erased" => {
                    (StatusCode::CONFLICT, "precondition_failed")
                }
                "plugin_grant_unavailable" | "selected_input_unavailable" => {
                    (StatusCode::SERVICE_UNAVAILABLE, "unavailable")
                }
                _ => (StatusCode::UNPROCESSABLE_ENTITY, "validation_error"),
            };
            (code, Json(json!({"problem":{"code":k,"message":k}}))).into_response()
        }
    }
}
fn bearer(h: &HeaderMap) -> Result<String> {
    let h = h
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| !v.trim().is_empty() && v.len() <= 512)
        .ok_or("unauthenticated")?;
    Ok(credentials::hash(h))
}
static VALIDATORS: LazyLock<BTreeMap<String, jsonschema::Validator>> = LazyLock::new(|| {
    let source: Value = serde_json::from_str(include_str!(
        "../../../protocol/baseline-v4/client-openapi.json"
    ))
    .unwrap();
    ["PairBody","ConnectorPairBody","LinkCodeBody","MessageBody","WorkInputBody","WorkCancelBody","MemoryCorrectionBody","AutomationActionBody"].into_iter().map(|name|{
        let spec=json!({"components":source["components"],"$ref":format!("#/components/schemas/{name}")});
        (name.to_owned(),jsonschema::validator_for(&spec).expect("pinned Client9 schema"))
    }).collect()
});
fn validate(name: &str, v: &Value) -> Result<()> {
    if !VALIDATORS.get(name).ok_or("unknown schema")?.is_valid(v) {
        return Err("validation_error".into());
    }
    Ok(())
}
async fn pair(store: Store, body: Value, purpose: Purpose) -> Result<Value> {
    validate(
        if purpose == Purpose::Chat {
            "PairBody"
        } else {
            "ConnectorPairBody"
        },
        &body,
    )?;
    write(
        &store,
        Request::Pair {
            purpose,
            code_hash: credentials::hash(body["code"].as_str().ok_or("unauthenticated")?.trim()),
            label: body["device_label"]
                .as_str()
                .ok_or("invalid_device_label")?
                .into(),
        },
    )
    .await
}
async fn pair_chat(State(s): State<Store>, Json(v): Json<Value>) -> Response {
    reply(pair(s, v, Purpose::Chat).await, true)
}
async fn pair_connector(State(s): State<Store>, Json(v): Json<Value>) -> Response {
    reply(pair(s, v, Purpose::Connector).await, false)
}
pub(super) async fn action(
    s: &Store,
    h: &HeaderMap,
    purpose: Purpose,
    c: Command,
) -> Result<Value> {
    write(
        s,
        Request::Entry {
            token_hash: bearer(h)?,
            selection: super::selection::Selection::headers(h)?,
            purpose,
            command: c,
        },
    )
    .await
}
async fn issue(State(s): State<Store>, h: HeaderMap, Json(v): Json<Value>) -> Response {
    reply(
        async {
            validate("LinkCodeBody", &v)?;
            let purpose =
                serde_json::from_value(v.get("purpose").cloned().unwrap_or(json!("chat")))?;
            let op = v["request_id"].as_str().ok_or("invalid_request_id")?;
            action(&s, &h, Purpose::Chat, Command::Issue(op.into(), purpose)).await
        }
        .await,
        true,
    )
}
async fn revoke(State(s): State<Store>, h: HeaderMap, Path(id): Path<String>) -> Response {
    reply(
        action(&s, &h, Purpose::Chat, Command::Revoke(id)).await,
        true,
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Proof {
    plugin: String,
    work: Option<String>,
    purpose: Purpose,
}
async fn exchange(State(s): State<Store>, h: HeaderMap, Json(p): Json<Proof>) -> Response {
    let command = match p.work {
        Some(work) => Command::Proof(p.plugin, work, p.purpose == Purpose::Connector),
        None if p.purpose == Purpose::Connector => Command::ConnectionProof(p.plugin),
        None => return reply(Err("validation_error".into()), true),
    };
    reply(action(&s, &h, p.purpose, command).await, true)
}
enum Read {
    Facts(String, Option<String>, i64),
    Fact(i64),
    Automations,
    Me,
    Home,
    Work(String),
    Works(Option<String>, i64),
    Events(String, i64, Option<i64>, i64),
    Receipt(String),
}
async fn current(db: &mut sqlx::SqliteConnection, h: &HeaderMap) -> Result<credentials::Principal> {
    super::selection::authenticate(
        db,
        &bearer(h)?,
        Purpose::Chat,
        &super::selection::Selection::headers(h)?,
        false,
    )
    .await
}
async fn read(s: &Store, h: &HeaderMap, method: Read) -> Result<Value> {
    let mut tx = s.readers().begin().await?;
    let who = current(&mut tx, h).await?;
    match method {
        Read::Facts(about, cursor, n) => {
            super::domains::list(&mut tx, &who, &about, cursor.as_deref(), n).await
        }
        Read::Fact(n) => super::domains::fact(&mut tx, &who, n).await,
        Read::Automations => super::domains::schedules(&mut tx, &who).await,
        Read::Me => views::me(&mut tx, &who).await,
        Read::Home => views::home(&mut tx, &who).await,
        Read::Work(id) => work::project(&mut tx, &who, &id).await,
        Read::Works(before, limit) => views::works(&mut tx, &who, before.as_deref(), limit).await,
        Read::Events(id, after, before, limit) => {
            views::events(&mut tx, &who, &id, after, before, limit).await
        }
        Read::Receipt(op) => work::receipt(&mut tx, &who, &op).await,
    }
}
async fn me(State(s): State<Store>, h: HeaderMap) -> Response {
    reply(read(&s, &h, Read::Me).await, true)
}
async fn home(State(s): State<Store>, h: HeaderMap) -> Response {
    reply(read(&s, &h, Read::Home).await, true)
}
async fn one_work(State(s): State<Store>, h: HeaderMap, Path(id): Path<String>) -> Response {
    reply(read(&s, &h, Read::Work(id)).await, true)
}
async fn receipt(State(s): State<Store>, h: HeaderMap, Path(id): Path<String>) -> Response {
    reply(read(&s, &h, Read::Receipt(id)).await, true)
}
#[derive(Deserialize, Default)]
struct Page {
    limit: Option<i64>,
    before: Option<String>,
    after: Option<i64>,
}
async fn works(State(s): State<Store>, h: HeaderMap, Query(p): Query<Page>) -> Response {
    reply(
        read(&s, &h, Read::Works(p.before, p.limit.unwrap_or(30))).await,
        true,
    )
}
async fn events(
    State(s): State<Store>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(p): Query<Page>,
) -> Response {
    reply(
        async {
            let before = p.before.as_deref().map(str::parse).transpose()?;
            read(
                &s,
                &h,
                Read::Events(id, p.after.unwrap_or(0), before, p.limit.unwrap_or(30)),
            )
            .await
        }
        .await,
        true,
    )
}
async fn change(s: Store, h: HeaderMap, id: String, v: Value, cancel: bool) -> Response {
    reply(
        async {
            validate(
                if cancel {
                    "WorkCancelBody"
                } else {
                    "WorkInputBody"
                },
                &v,
            )?;
            action(&s, &h, Purpose::Chat, Command::Work(id, v, cancel)).await
        }
        .await,
        true,
    )
}
async fn input(
    State(s): State<Store>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(v): Json<Value>,
) -> Response {
    change(s, h, id, v, false).await
}
async fn cancel(
    State(s): State<Store>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(v): Json<Value>,
) -> Response {
    change(s, h, id, v, true).await
}
async fn message(State(s): State<Store>, h: HeaderMap, Json(v): Json<Value>) -> Response {
    reply(
        async {
            validate("MessageBody", &v)?;
            action(&s, &h, Purpose::Chat, Command::Message(v)).await
        }
        .await,
        true,
    )
}
#[derive(Deserialize)]
struct FactPage {
    about: Option<String>,
    before: Option<String>,
    limit: Option<i64>,
}
async fn facts(State(s): State<Store>, h: HeaderMap, Query(p): Query<FactPage>) -> Response {
    reply(
        read(
            &s,
            &h,
            Read::Facts(
                p.about.unwrap_or_else(|| "me".into()),
                p.before,
                p.limit.unwrap_or(50),
            ),
        )
        .await,
        true,
    )
}
async fn fact(State(s): State<Store>, h: HeaderMap, Path(n): Path<i64>) -> Response {
    reply(read(&s, &h, Read::Fact(n)).await, true)
}
async fn correct(
    State(s): State<Store>,
    h: HeaderMap,
    Path(n): Path<i64>,
    Json(v): Json<Value>,
) -> Response {
    reply(
        async {
            validate("MemoryCorrectionBody", &v)?;
            action(&s, &h, Purpose::Chat, Command::Correct(n, v)).await
        }
        .await,
        true,
    )
}
async fn automations(State(s): State<Store>, h: HeaderMap) -> Response {
    reply(read(&s, &h, Read::Automations).await, true)
}
async fn schedule(
    State(s): State<Store>,
    h: HeaderMap,
    Path((id, a)): Path<(i64, String)>,
    Json(v): Json<Value>,
) -> Response {
    reply(
        async {
            validate("AutomationActionBody", &v)?;
            action(&s, &h, Purpose::Chat, Command::Schedule(id, a, v)).await
        }
        .await,
        true,
    )
}
async fn meta(State(store): State<Store>) -> Response {
    let name = store.main.as_ref().map_or("Yuki", |p| p.name.as_str());
    reply(
        Ok(
            json!({"api_revision":9,"server_version":env!("CARGO_PKG_VERSION"),"character":{"id":"yuki","display_name":name,"accent":{"light":"#3A6BD6","dark":"#7FA4FF"},"avatar_url":null},"min_client":{"desktop":null,"android":null},"capabilities":["works","memory","automations"]}),
        ),
        true,
    )
}
#[derive(Deserialize)]
struct ArtifactList {
    cursor: Option<String>,
    limit: Option<usize>,
    kind: Option<String>,
}
async fn artifacts(
    State(s): State<Store>,
    h: HeaderMap,
    Query(q): Query<ArtifactList>,
) -> Response {
    reply(
        async {
            let mut tx = s.readers().begin().await?;
            let who = current(&mut tx, &h).await?;
            super::artifacts::list(
                &mut tx,
                &who,
                q.cursor.as_deref(),
                q.limit.unwrap_or(50),
                q.kind.as_deref().unwrap_or("all"),
            )
            .await
        }
        .await,
        true,
    )
}
#[derive(Deserialize)]
struct Version {
    version: String,
}
async fn download(
    State(s): State<Store>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<Version>,
) -> Response {
    let result = async {
        let mut tx = s.readers().begin().await?;
        let who = current(&mut tx, &h).await?;
        super::artifacts::download(&mut tx, &who, &id, &q.version).await
    }
    .await;
    match result {
        Err(e) => reply(Err(e), true),
        Ok((name, data)) => {
            let mut encoded = String::new();
            for b in name.bytes() {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    encoded.push(b as char);
                } else {
                    use std::fmt::Write;
                    write!(encoded, "%{b:02X}").expect("write to String");
                }
            }
            (
                [
                    ("content-type", "application/octet-stream".to_string()),
                    ("etag", format!("\"{}\"", q.version)),
                    ("x-yuki-sha256", q.version),
                    (
                        "content-disposition",
                        format!("attachment; filename*=UTF-8''{encoded}"),
                    ),
                ],
                data,
            )
                .into_response()
        }
    }
}
pub fn router(store: Store) -> Router {
    Router::new()
        .route("/client/v1/meta", get(meta))
        .route("/client/v1/pair", post(pair_chat))
        .route("/agents/v1/pair", post(pair_connector))
        .route("/client/v1/link-codes", post(issue))
        .route("/client/v1/devices/{id}", delete(revoke))
        .route("/core/v1/entry/exchange", post(exchange))
        .route("/client/v1/memory/facts", get(facts))
        .route("/client/v1/memory/facts/{id}", get(fact))
        .route("/client/v1/memory/facts/{id}/corrections", post(correct))
        .route("/client/v1/automations", get(automations))
        .route(
            "/client/v1/automations/{id}/actions/{action}",
            post(schedule),
        )
        .route("/client/v1/artifacts", get(artifacts))
        .route("/client/v1/artifacts/{id}", get(download))
        .route("/client/v1/me", get(me))
        .route("/client/v1/home", get(home))
        .route("/client/v1/conversations/{id}/events", get(events))
        .route("/client/v1/messages", post(message))
        .route("/client/v1/works", get(works))
        .route("/client/v1/works/{id}", get(one_work))
        .route("/client/v1/works/{id}/inputs", post(input))
        .route("/client/v1/works/{id}/cancel", post(cancel))
        .route("/client/v1/requests/{id}", get(receipt))
        .with_state(store)
}
