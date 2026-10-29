//! Bounded local operator HTTP adapter; intentionally no browser or public auth profile.
use crate::admission::{AdmissionStore, Kind, validate};
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use edgeagent_contracts::research::{MAX_RESEARCH_BYTES, ResearchRequest};
use std::{sync::Arc, time::Duration};
/// Admission-only composition retained for callers without a projector endpoint.
pub fn router<S: AdmissionStore>(store: Arc<S>) -> Router {
    router_with_queries(store, None)
}
/// Compose admission and bounded projector queries for one configured operator.
pub fn router_with_queries<S: AdmissionStore>(
    store: Arc<S>,
    projector: Option<Arc<crate::query::ProjectorClient>>,
) -> Router {
    let queries = crate::query::router(store.scope().to_owned(), projector);
    Router::new()
        .route("/v1/research-requests", post(submit::<S>))
        .layer(DefaultBodyLimit::max(MAX_RESEARCH_BYTES))
        .with_state(store)
        .merge(queries)
}
async fn submit<S: AdmissionStore>(
    State(store): State<Arc<S>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !local_request(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(str::trim)
        != Some("application/json")
    {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    let Ok(request) = serde_json::from_slice::<ResearchRequest>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if request.scope != store.scope() {
        return StatusCode::FORBIDDEN.into_response();
    }
    if validate(&request, store.scope()).is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match tokio::time::timeout(Duration::from_secs(20), store.admit(request)).await {
        Ok(Ok(accepted)) => match serde_json::to_vec(&accepted) {
            Ok(body) => (
                StatusCode::ACCEPTED,
                [("content-type", "application/json")],
                body,
            )
                .into_response(),
            Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
        },
        Ok(Err(e)) if e.kind() == Kind::Conflict => StatusCode::CONFLICT.into_response(),
        Ok(Err(e)) if e.kind() == Kind::Invalid => StatusCode::BAD_REQUEST.into_response(),
        _ => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

/// Shared local operator boundary for reads and admission.
pub(crate) fn local_request(headers: &HeaderMap) -> bool {
    !headers.contains_key("origin")
        && headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<axum::http::uri::Authority>().ok())
            .is_some_and(|v| {
                v.host() == "localhost"
                    || v.host()
                        .trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            })
}
