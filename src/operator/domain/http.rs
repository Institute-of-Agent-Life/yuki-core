use super::{Request, invoke, save};
use crate::{
    model::profile_store::http::{authority, reply},
    store::Store,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::Response,
    routing::{get, post},
};
use serde_json::{Value, json};

async fn read(store: Store, headers: HeaderMap, method: &str, args: Value) -> Response {
    reply(
        async {
            invoke(
                &store,
                Request::Read(authority(&headers)?, method.into(), args),
            )
            .await
        }
        .await,
    )
}
async fn write(store: Store, headers: HeaderMap, method: &str, body: Value) -> Response {
    reply(async { save(&store, authority(&headers)?, method.into(), body).await }.await)
}
async fn character(State(store): State<Store>, headers: HeaderMap) -> Response {
    read(store, headers, "character.get", json!({})).await
}
async fn public_character(State(store): State<Store>, headers: HeaderMap) -> Response {
    read(store, headers, "character.public", json!({})).await
}
async fn update_character(
    State(store): State<Store>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    write(store, headers, "character.update", body).await
}
async fn soul(State(store): State<Store>, headers: HeaderMap) -> Response {
    read(store, headers, "character.soul.read", json!({})).await
}
async fn save_soul(
    State(store): State<Store>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    write(store, headers, "character.soul.save", body).await
}
async fn reload_soul(
    State(store): State<Store>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    write(store, headers, "character.soul.reload", body).await
}
async fn receipt(
    State(store): State<Store>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    read(store, headers, "operation.get", json!({"request_id":id})).await
}
async fn outbox_retry(
    State(store): State<Store>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    target_write(store, headers, "plugin.outbox.retry", id, body).await
}
async fn plugin_configuration(
    State(store): State<Store>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(args): Query<std::collections::BTreeMap<String, String>>,
) -> Response {
    let mut args = serde_json::to_value(args).expect("string query");
    args["plugin_id"] = json!(id);
    read(store, headers, "plugin.configuration.read", args).await
}
async fn save_plugin_configuration(
    State(store): State<Store>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut body): Json<Value>,
) -> Response {
    if let Some(object) = body.as_object_mut() {
        if object.contains_key("plugin_id") {
            return reply(Err("validation_error".into()));
        }
        object.insert("plugin_id".into(), json!(id));
    }
    write(store, headers, "plugin.configuration.save", body).await
}
async fn plugin_list(State(store): State<Store>, headers: HeaderMap) -> Response {
    read(store, headers, "plugin.list", json!({})).await
}
async fn plugin_get(
    State(store): State<Store>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    read(store, headers, "plugin.get", json!({"plugin_id":id})).await
}
async fn plugin_action(
    State(store): State<Store>,
    headers: HeaderMap,
    Path((id, action)): Path<(String, String)>,
    Json(mut body): Json<Value>,
) -> Response {
    if !matches!(
        action.as_str(),
        "enable" | "disable" | "approve" | "doctor" | "discover"
    ) {
        return reply(Err("validation_error".into()));
    }
    let Some(object) = body.as_object_mut() else {
        return reply(Err("validation_error".into()));
    };
    if object.contains_key("plugin_id") {
        return reply(Err("validation_error".into()));
    }
    object.insert("plugin_id".into(), json!(id));
    write(store, headers, &format!("plugin.{action}"), body).await
}
#[derive(serde::Deserialize)]
struct Observation {
    cursor: Option<String>,
    limit: Option<u64>,
}
async fn plugin_observe(
    State(store): State<Store>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(args): Query<Observation>,
) -> Response {
    read(
        store,
        headers,
        "plugin.observe",
        json!({"plugin_id":id,"cursor":args.cursor,"limit":args.limit.unwrap_or(10)}),
    )
    .await
}
#[derive(serde::Deserialize)]
struct TraceQuery {
    after: Option<i64>,
    limit: Option<i64>,
    content: Option<bool>,
}
async fn trace(
    State(store): State<Store>,
    headers: HeaderMap,
    Path((kind, identity)): Path<(String, String)>,
    Query(args): Query<TraceQuery>,
) -> Response {
    read(store,headers,"trace.read",json!({"kind":kind,"identity":identity,"after":args.after.unwrap_or(0),"limit":args.limit.unwrap_or(100),"content":args.content.unwrap_or(false)})).await
}
async fn model_stats(
    State(store): State<Store>,
    headers: HeaderMap,
    Query(q): Query<std::collections::BTreeMap<String, String>>,
) -> Response {
    read(
        store,
        headers,
        "model.stats",
        serde_json::to_value(q).expect("string query"),
    )
    .await
}
async fn health(State(store): State<Store>, headers: HeaderMap) -> Response {
    read(store, headers, "health.read", json!({})).await
}
#[derive(serde::Deserialize)]
struct WorkQuery {
    after: Option<String>,
    limit: Option<i64>,
    state: Option<String>,
}
async fn works(
    State(store): State<Store>,
    headers: HeaderMap,
    Query(q): Query<WorkQuery>,
) -> Response {
    read(
        store,
        headers,
        "work.list",
        json!({"after":q.after.unwrap_or_default(),"limit":q.limit.unwrap_or(100),"state":q.state}),
    )
    .await
}
async fn work(State(store): State<Store>, headers: HeaderMap, Path(id): Path<String>) -> Response {
    read(store, headers, "work.get", json!({"id":id})).await
}
async fn work_action(
    State(store): State<Store>,
    headers: HeaderMap,
    Path((id, action)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    if !matches!(action.as_str(), "cancel" | "resume") {
        return reply(Err("validation_error".into()));
    }
    target_write(store, headers, &format!("work.{action}"), id, body).await
}
#[derive(serde::Deserialize)]
struct IdentityQuery {
    after: Option<String>,
    limit: Option<i64>,
    person_id: Option<String>,
    platform: Option<String>,
    external_account_id: Option<String>,
}
async fn persons(
    State(store): State<Store>,
    headers: HeaderMap,
    Query(q): Query<IdentityQuery>,
) -> Response {
    read(
        store,
        headers,
        "person.list",
        json!({"after":q.after.unwrap_or_default(),"limit":q.limit.unwrap_or(100)}),
    )
    .await
}
async fn create_person(
    State(store): State<Store>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if body.get("id").is_some() {
        return reply(Err("validation_error".into()));
    }
    write(store, headers, "person.create", body).await
}
async fn person(
    State(store): State<Store>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    read(store, headers, "person.get", json!({"id":id})).await
}
async fn bindings(
    State(store): State<Store>,
    headers: HeaderMap,
    Query(q): Query<IdentityQuery>,
) -> Response {
    read(store,headers,"binding.list",json!({"after":q.after.unwrap_or_default(),"limit":q.limit.unwrap_or(100),"person_id":q.person_id,"platform":q.platform,"external_account_id":q.external_account_id})).await
}
async fn binding(
    State(store): State<Store>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    read(store, headers, "binding.get", json!({"id":id})).await
}
fn target(mut body: Value, id: String) -> crate::Result<Value> {
    let object = body.as_object_mut().ok_or("validation_error")?;
    if object.contains_key("id") {
        return Err("validation_error".into());
    }
    object.insert("id".into(), json!(id));
    Ok(body)
}
async fn target_write(
    store: Store,
    headers: HeaderMap,
    method: &str,
    id: String,
    body: Value,
) -> Response {
    match target(body, id) {
        Ok(body) => write(store, headers, method, body).await,
        Err(e) => reply(Err(e)),
    }
}
async fn person_action(
    State(store): State<Store>,
    headers: HeaderMap,
    Path((id, action)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let method = match action.as_str() {
        "enable" => "person.enable",
        "disable" => "person.disable",
        "rename" => "person.rename",
        "merge" => "person.merge",
        "forget" => "person.forget",
        "client-pair" => "person.client-pair",
        "bindings" => "binding.attach",
        _ => return reply(Err("not_found".into())),
    };
    target_write(store, headers, method, id, body).await
}
async fn person_superuser(
    State(store): State<Store>,
    headers: HeaderMap,
    Path((id, action)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let method = match action.as_str() {
        "grant" => "person.superuser.grant",
        "revoke" => "person.superuser.revoke",
        _ => return reply(Err("not_found".into())),
    };
    target_write(store, headers, method, id, body).await
}
async fn binding_action(
    State(store): State<Store>,
    headers: HeaderMap,
    Path((id, action)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let method = match action.as_str() {
        "enable" => "binding.enable",
        "disable" => "binding.disable",
        "transfer" => "binding.transfer",
        "unbind" => "binding.unbind",
        _ => return reply(Err("not_found".into())),
    };
    target_write(store, headers, method, id, body).await
}
async fn revoke_device(
    State(store): State<Store>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    target_write(store, headers, "device.revoke", id, body).await
}
async fn presences(
    State(store): State<Store>,
    headers: HeaderMap,
    Query(q): Query<IdentityQuery>,
) -> Response {
    read(
        store,
        headers,
        "presence.list",
        json!({"after":q.after.unwrap_or_default(),"limit":q.limit.unwrap_or(100)}),
    )
    .await
}
async fn presence(
    State(store): State<Store>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    read(store, headers, "presence.get", json!({"id":id})).await
}
async fn register_presence(
    State(store): State<Store>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if body.get("id").is_some() {
        return reply(Err("validation_error".into()));
    }
    write(store, headers, "presence.register", body).await
}
async fn presence_state(
    State(store): State<Store>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    target_write(store, headers, "presence.state", id, body).await
}
async fn presence_action(
    State(store): State<Store>,
    headers: HeaderMap,
    Path((id, action)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    if !matches!(action.as_str(), "unregister" | "transfer") {
        return reply(Err("not_found".into()));
    }
    target_write(store, headers, &format!("presence.{action}"), id, body).await
}
pub(crate) fn router(store: Store) -> Router {
    Router::new()
        .route("/admin/v1/health", get(health))
        .route("/admin/v1/works", get(works))
        .route("/admin/v1/works/{id}", get(work))
        .route("/admin/v1/works/{id}/{action}", post(work_action))
        .route(
            "/admin/v1/presences",
            get(presences).post(register_presence),
        )
        .route(
            "/admin/v1/presences/{id}",
            get(presence).post(presence_state),
        )
        .route("/admin/v1/presences/{id}/{action}", post(presence_action))
        .route("/admin/v1/models/stats", get(model_stats))
        .route("/admin/v1/persons", get(persons).post(create_person))
        .route("/admin/v1/persons/{id}", get(person))
        .route("/admin/v1/persons/{id}/{action}", post(person_action))
        .route(
            "/admin/v1/persons/{id}/superuser/{action}",
            post(person_superuser),
        )
        .route("/admin/v1/bindings", get(bindings))
        .route("/admin/v1/bindings/{id}", get(binding))
        .route("/admin/v1/bindings/{id}/{action}", post(binding_action))
        .route("/admin/v1/devices/{id}/revoke", post(revoke_device))
        .route(
            "/admin/v1/character",
            get(character).patch(update_character),
        )
        .route("/admin/v1/character/soul", get(soul).put(save_soul))
        .route(
            "/client/v1/character",
            get(public_character).patch(update_character),
        )
        .route("/client/v1/character/soul", get(soul).put(save_soul))
        .route("/client/v1/character/soul/reload", post(reload_soul))
        .route("/admin/v1/character/soul/reload", post(reload_soul))
        .route("/admin/v1/operations/{id}", get(receipt))
        .route(
            "/admin/v1/plugins/{id}/configuration",
            get(plugin_configuration).post(save_plugin_configuration),
        )
        .route("/admin/v1/traces/{kind}/{identity}", get(trace))
        .route("/admin/v1/plugins", get(plugin_list))
        .route("/admin/v1/plugins/{id}", get(plugin_get))
        .route(
            "/admin/v1/plugins/{id}/actions/{action}",
            post(plugin_action),
        )
        .route("/admin/v1/plugins/{id}/observe", get(plugin_observe))
        .route("/admin/v1/plugin-outbox/{id}/retry", post(outbox_retry))
        .with_state(store)
}
