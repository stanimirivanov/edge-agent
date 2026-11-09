//! Local internal read-only queries. Not a public authentication or Gateway surface.
use crate::{
    error::{Error, Kind},
    explanation_authority::Client,
    store::ProjectionStore,
};
use axum::{
    Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use std::{sync::Arc, time::Duration};
/// Read metadata and exact canonical outcome through separate bounded GET endpoints.
pub fn router(store: Arc<ProjectionStore>) -> Router {
    with_authority(store, None)
}

pub(crate) fn with_authority(store: Arc<ProjectionStore>, authority: Option<Client>) -> Router {
    let authority = authority.map(Arc::new);
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
        .route(
            "/internal/v1/research-requests/{scope}/{id}/explanation",
            get(
                move |State(store): State<Arc<ProjectionStore>>,
                      Path((scope, id)): Path<(String, String)>,
                      headers: HeaderMap| {
                    let authority = authority.clone();
                    async move {
                        let mut response = if !allowed(&headers) || scope != store.scope() {
                            error(StatusCode::FORBIDDEN, "forbidden")
                        } else if matches!(id.as_str(), "." | "..") {
                            error(StatusCode::BAD_REQUEST, "invalid_request")
                        } else {
                            tokio::time::timeout(
                                Duration::from_secs(8),
                                explanation(&store, &scope, &id, authority.as_deref()),
                            )
                            .await
                            .unwrap_or_else(|_| {
                                error(StatusCode::SERVICE_UNAVAILABLE, "unavailable")
                            })
                        };
                        response.headers_mut().insert(
                            header::CACHE_CONTROL,
                            header::HeaderValue::from_static("no-store"),
                        );
                        response
                    }
                },
            ),
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
        Ok(Ok(Some(view))) => match view.status_bytes() {
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

async fn explanation(
    store: &ProjectionStore,
    scope: &str,
    id: &str,
    authority: Option<&Client>,
) -> Response {
    let view = match store.get(scope, id).await {
        Ok(Some(view)) => view,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_observed"),
        Err(e) => return failure(&e),
    };
    match view.status {
        "gap" => return error(StatusCode::CONFLICT, "gap"),
        "invalidated" | "expired" | "withdrawn" | "corrected" => {
            return error(StatusCode::GONE, view.status);
        }
        "pending" => return error(StatusCode::ACCEPTED, "pending"),
        _ => {}
    }
    let (digest, bytes) = match store.verified_explanation(view).await {
        Ok(Some(explanation)) => explanation,
        Ok(None) => return error(StatusCode::NOT_FOUND, "not_observed"),
        Err(e) => return failure(&e),
    };
    let Some(authority) = authority else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "unavailable");
    };
    match authority.allows(scope, id, &digest).await {
        Ok(true) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            bytes,
        )
            .into_response(),
        Ok(false) => error(StatusCode::FORBIDDEN, "forbidden"),
        Err(e) => failure(&e),
    }
}
