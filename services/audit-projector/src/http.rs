//! Local internal read-only queries. Not a public authentication or Gateway surface.
use crate::{
    error::{Error, Kind},
    store::ProjectionStore,
};
use axum::{
    Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use std::sync::Arc;
/// Read metadata and exact canonical outcome through separate bounded GET endpoints.
pub fn router(store: Arc<ProjectionStore>) -> Router {
    Router::new()
        .route("/internal/v1/research-requests/{scope}/{id}", get(status))
        .route(
            "/internal/v1/research-requests/{scope}/{id}/outcome",
            get(outcome),
        )
        .route(
            "/internal/v1/research-requests/{scope}/{id}/decision",
            get(decision),
        )
        .with_state(store)
}
fn allowed(headers: &HeaderMap) -> bool {
    !headers.contains_key("origin")
        && headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<axum::http::uri::Authority>().ok())
            .is_some_and(|a| {
                a.host() == "localhost"
                    || a.host()
                        .trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            })
}
fn error(status: StatusCode, state: &str) -> Response {
    (
        status,
        [("content-type", "application/json")],
        format!("{{\"status\":\"{state}\"}}"),
    )
        .into_response()
}
fn failure(e: &Error) -> Response {
    match e.kind() {
        Kind::Scope => error(StatusCode::FORBIDDEN, "forbidden"),
        Kind::Invalid => error(StatusCode::BAD_REQUEST, "invalid_request"),
        _ => error(StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
    }
}
async fn status(
    State(store): State<Arc<ProjectionStore>>,
    Path((scope, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if !allowed(&headers) {
        return error(StatusCode::FORBIDDEN, "forbidden");
    }
    match tokio::time::timeout(std::time::Duration::from_secs(5), store.get(&scope, &id)).await {
        Ok(Ok(Some(view))) => match serde_json::to_vec(&view) {
            Ok(bytes) => (
                StatusCode::OK,
                [("content-type", "application/json")],
                bytes,
            )
                .into_response(),
            Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
        },
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "not_observed"),
        Ok(Err(e)) => failure(&e),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
    }
}
async fn outcome(
    State(store): State<Arc<ProjectionStore>>,
    Path((scope, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if !allowed(&headers) {
        return error(StatusCode::FORBIDDEN, "forbidden");
    }
    match tokio::time::timeout(std::time::Duration::from_secs(5), store.get(&scope, &id)).await {
        Ok(Ok(Some(view))) if view.status == "gap" => error(StatusCode::CONFLICT, "gap"),
        Ok(Ok(Some(view)))
            if matches!(
                view.status,
                "invalidated" | "expired" | "withdrawn" | "corrected"
            ) =>
        {
            error(StatusCode::GONE, view.status)
        }
        Ok(Ok(Some(view))) => match view.outcome_bytes() {
            Some(bytes) => (
                StatusCode::OK,
                [("content-type", "application/json")],
                bytes.to_vec(),
            )
                .into_response(),
            None => error(StatusCode::ACCEPTED, "pending"),
        },
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "not_observed"),
        Ok(Err(e)) => failure(&e),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
    }
}

async fn decision(
    State(store): State<Arc<ProjectionStore>>,
    Path((scope, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if !allowed(&headers) {
        return error(StatusCode::FORBIDDEN, "forbidden");
    }
    match tokio::time::timeout(std::time::Duration::from_secs(5), store.get(&scope, &id)).await {
        Ok(Ok(Some(view))) if view.status == "gap" => error(StatusCode::CONFLICT, "gap"),
        Ok(Ok(Some(view))) => match view.provenance_bytes() {
            Some(bytes) => (
                StatusCode::OK,
                [("content-type", "application/json")],
                bytes.to_vec(),
            )
                .into_response(),
            None => error(StatusCode::NOT_FOUND, "not_observed"),
        },
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "not_observed"),
        Ok(Err(e)) => failure(&e),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
    }
}
