//! Loopback, payload-free authorization for one exact explanation read.
use axum::{
    Router,
    body::to_bytes,
    extract::Request,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::post,
};
use serde::Deserialize;
use std::{future::Future, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

/// Exact read coordinates. The server resolves the artifact and decision from
/// its retained state.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Read {
    pub scope: String,
    pub request_id: String,
    pub audience: String,
    pub companion: String,
}

/// Single-read answer with no transferable grant or companion content.
pub enum Decision {
    Allowed,
    Denied,
    Unavailable,
}

const DEADLINE: Duration = Duration::from_secs(5);

/// Bind one trusted local operator scope and one reviewed audience.
pub fn router<F>(
    scope: String,
    audience: String,
    check: impl Fn(Read) -> F + Clone + Send + Sync + 'static,
) -> Router
where
    F: Future<Output = Decision> + Send,
{
    let capacity = Arc::new(Semaphore::new(4));
    Router::new().route(
        "/internal/v1/explanation-authorizations",
        post(move |request: Request| {
            let (scope, audience, capacity, check) = (
                scope.clone(),
                audience.clone(),
                capacity.clone(),
                check.clone(),
            );
            async move {
                let status = async {
                    let headers = request.headers();
                    let local_host = headers
                        .get(header::HOST)
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse::<axum::http::uri::Authority>().ok())
                        .is_some_and(|authority| {
                            authority
                                .host()
                                .trim_matches(['[', ']'])
                                .parse::<std::net::IpAddr>()
                                .is_ok_and(|ip| ip.is_loopback())
                        });
                    if !local_host || headers.contains_key(header::ORIGIN) {
                        return StatusCode::FORBIDDEN;
                    }
                    if request.uri().query().is_some()
                        || headers
                            .get(header::CONTENT_TYPE)
                            .is_none_or(|v| v != "application/json")
                        || headers.contains_key(header::CONTENT_ENCODING)
                    {
                        return StatusCode::BAD_REQUEST;
                    }
                    let Ok(_permit) = capacity.try_acquire() else {
                        return StatusCode::SERVICE_UNAVAILABLE;
                    };
                    let Ok(bytes) = to_bytes(request.into_body(), 1024).await else {
                        return StatusCode::BAD_REQUEST;
                    };
                    let Ok(read) = serde_json::from_slice::<Read>(&bytes) else {
                        return StatusCode::BAD_REQUEST;
                    };
                    if !crate::runtime::token(&read.request_id)
                        || matches!(read.request_id.as_str(), "." | "..")
                        || crate::loader::digest(&read.companion).is_err()
                    {
                        return StatusCode::BAD_REQUEST;
                    }
                    if read.scope != scope || read.audience != audience {
                        return StatusCode::FORBIDDEN;
                    }
                    match check(read).await {
                        Decision::Allowed => StatusCode::NO_CONTENT,
                        Decision::Denied => StatusCode::FORBIDDEN,
                        Decision::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
                    }
                };
                let status = tokio::time::timeout(DEADLINE, status)
                    .await
                    .unwrap_or(StatusCode::SERVICE_UNAVAILABLE);
                (status, [(header::CACHE_CONTROL, "no-store")]).into_response()
            }
        }),
    )
}

pub(crate) fn owner_router(
    scope: String,
    audience: edgeagent_contracts::evidence::EvidenceId,
    store: edgeagent_research_postgres::PostgresResearchStore,
    owner: crate::loader::OwnerEndpoint,
) -> Router {
    let store = Arc::new(store);
    let owner = Arc::new(owner);
    router(scope, audience.as_str().into(), move |read| {
        let store = store.clone();
        let owner = owner.clone();
        async move {
            let current = || {
                store.current_explanation_read(
                    &read.scope,
                    &read.request_id,
                    &read.companion,
                    &read.audience,
                    || crate::runtime::clock_ms().unwrap_or(-1),
                )
            };
            let delivery = match current().await {
                Ok(Some(delivery)) => delivery,
                Ok(None) => return Decision::Denied,
                Err(_) => return Decision::Unavailable,
            };
            let bytes = match delivery.to_json() {
                Ok(bytes) => bytes,
                Err(_) => return Decision::Unavailable,
            };
            match check_owner(&owner, &read, &delivery, &bytes).await {
                Ok(Decision::Allowed) => {}
                Ok(refusal) => return refusal,
                Err(_) => return Decision::Unavailable,
            }
            match current().await {
                Ok(Some(confirmed)) if confirmed.to_json().is_ok_and(|value| value == bytes) => {
                    Decision::Allowed
                }
                Ok(None) => Decision::Denied,
                _ => Decision::Unavailable,
            }
        }
    })
}

async fn check_owner(
    owner: &crate::loader::OwnerEndpoint,
    read: &Read,
    delivery: &edgeagent_contracts::MessageEnvelope,
    bytes: &[u8],
) -> Result<Decision, Box<dyn std::error::Error + Send + Sync>> {
    use edgeagent_contracts::research_acquisition::ResearchEvidence;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let ResearchEvidence::Ready { decision, .. } = ResearchEvidence::from_envelope(delivery)?
    else {
        return Ok(Decision::Denied);
    };
    // The outer HTTP deadline bounds the child, which is killed on cancellation.
    let mut child = owner
        .command(&read.scope)
        .args([
            "offline-explanation-authorize",
            decision.request.request.as_str(),
            &read.audience,
        ])
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or("owner stdin unavailable")?;
    let written = stdin.write_all(bytes).await;
    drop(stdin);
    let mut stdout = child.stdout.take().ok_or("owner stdout unavailable")?;
    if stdout.read(&mut [0]).await? != 0 {
        return Ok(Decision::Unavailable);
    }
    Ok(match child.wait().await?.code() {
        Some(0) if written.is_ok() => Decision::Allowed,
        Some(77) => Decision::Denied,
        _ => Decision::Unavailable,
    })
}
