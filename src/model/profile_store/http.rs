use super::{Request, invoke, save};
use crate::{Result, operator::config::Authority, store::Store};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};

fn authority(headers: &HeaderMap) -> Result<Authority> {
    let token = headers
        .get("authorization")
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer "))
        .filter(|token| !token.trim().is_empty() && token.len() <= 4096)
        .ok_or("unauthenticated")?;
    Ok(Authority::Bearer(crate::host::personal::credentials::hash(
        token,
    )))
}

fn reply(result: Result<Value>) -> Response {
    match result {
        Ok(value) => Json(json!({"data":value})).into_response(),
        Err(error) => {
            let reason = error.to_string();
            let (status, code) = match reason.as_str() {
                "unauthenticated" => (StatusCode::UNAUTHORIZED, "unauthenticated"),
                "forbidden" => (StatusCode::FORBIDDEN, "forbidden"),
                "not_found" => (StatusCode::NOT_FOUND, "not_found"),
                "version_conflict" => (StatusCode::CONFLICT, "version_conflict"),
                "request_conflict" => (StatusCode::CONFLICT, "request_conflict"),
                "precondition_failed" => (StatusCode::CONFLICT, "precondition_failed"),
                "configuration_unavailable"
                | "operator_configuration_unavailable"
                | "original_configuration_operation_unavailable" => {
                    (StatusCode::SERVICE_UNAVAILABLE, "unavailable")
                }
                _ => (StatusCode::UNPROCESSABLE_ENTITY, "validation_error"),
            };
            // Provider material and native file errors are never exposed over Admin HTTP.
            (
                status,
                Json(json!({"problem":{"code":code,"message":code}})),
            )
                .into_response()
        }
    }
}

async fn read(State(store): State<Store>, headers: HeaderMap) -> Response {
    reply(async { invoke(&store, Request::Read(authority(&headers)?)).await }.await)
}

async fn apply(
    State(store): State<Store>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    reply(async { save(&store, authority(&headers)?, body).await }.await)
}

async fn secret(
    State(store): State<Store>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    reply(async { super::save_secret(&store, authority(&headers)?, body).await }.await)
}

pub(crate) fn router(store: Store) -> Router {
    Router::new()
        .route("/admin/v1/models", get(read))
        .route("/admin/v1/models/apply", post(apply))
        .route("/admin/v1/models/secrets", post(secret))
        .with_state(store)
}
