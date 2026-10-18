//! Owned OTLP/HTTP receiver: decode wire evidence, never infer it from health.

use super::{TestError, TestResult};
use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::post,
};
use opentelemetry_proto::tonic::collector::{
    metrics::v1::ExportMetricsServiceRequest, trace::v1::ExportTraceServiceRequest,
};
use prost::Message;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use tokio::{
    sync::{Notify, oneshot},
    task::JoinHandle,
};

pub enum Wire {
    Traces(ExportTraceServiceRequest),
    Metrics(ExportMetricsServiceRequest),
}
pub struct Record {
    pub wire: Wire,
    pub bytes: Vec<u8>,
    pub accepted: bool,
}

#[derive(Clone)]
struct Capture {
    records: Arc<Mutex<Vec<Record>>>,
    mode: Arc<AtomicU8>,
    overflow: Arc<AtomicBool>,
    release: Arc<Notify>,
}

pub struct Receiver {
    pub endpoint: String,
    capture: Capture,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<Result<(), std::io::Error>>,
}

impl Receiver {
    pub async fn start() -> Result<Self, TestError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = format!("http://{}", listener.local_addr()?);
        let capture = Capture {
            records: Arc::default(),
            mode: Arc::default(),
            overflow: Arc::default(),
            release: Arc::default(),
        };
        let app = Router::new()
            .route("/v1/traces", post(traces))
            .route("/v1/metrics", post(metrics))
            .layer(DefaultBodyLimit::max(262_144))
            .with_state(capture.clone());
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
        });
        Ok(Self {
            endpoint,
            capture,
            stop: Some(stop),
            task,
        })
    }

    /// 0 accepts, 1 rejects with HTTP 503, 2 withholds the response until cleanup.
    pub fn mode(&self, mode: u8) {
        self.capture.mode.store(mode, Ordering::Relaxed);
    }

    pub fn records(&self) -> Result<std::sync::MutexGuard<'_, Vec<Record>>, TestError> {
        if self.capture.overflow.load(Ordering::Relaxed) {
            return Err("OTLP evidence bound exceeded".into());
        }
        self.capture
            .records
            .lock()
            .map_err(|_| "OTLP evidence lock poisoned".into())
    }

    pub async fn close(&mut self) -> TestResult {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.capture.release.notify_waiters();
        match tokio::time::timeout(std::time::Duration::from_secs(2), &mut self.task).await {
            Ok(result) => {
                result??;
                Ok(())
            }
            Err(error) => {
                self.task.abort();
                let _ = (&mut self.task).await;
                Err(error.into())
            }
        }
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        self.capture.release.notify_waiters();
        self.task.abort();
    }
}

async fn traces(
    State(state): State<Capture>,
    headers: HeaderMap,
    bytes: Bytes,
) -> (StatusCode, [(&'static str, &'static str); 1], Vec<u8>) {
    let decoded = ExportTraceServiceRequest::decode(bytes.clone()).map(Wire::Traces);
    receive(state, headers, bytes, decoded).await
}
async fn metrics(
    State(state): State<Capture>,
    headers: HeaderMap,
    bytes: Bytes,
) -> (StatusCode, [(&'static str, &'static str); 1], Vec<u8>) {
    let decoded = ExportMetricsServiceRequest::decode(bytes.clone()).map(Wire::Metrics);
    receive(state, headers, bytes, decoded).await
}
async fn receive(
    state: Capture,
    headers: HeaderMap,
    bytes: Bytes,
    decoded: Result<Wire, prost::DecodeError>,
) -> (StatusCode, [(&'static str, &'static str); 1], Vec<u8>) {
    let response_headers = [("content-type", "application/x-protobuf")];
    if headers.get("content-type").and_then(|v| v.to_str().ok()) != Some("application/x-protobuf") {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, response_headers, vec![]);
    }
    let Ok(wire) = decoded else {
        return (StatusCode::BAD_REQUEST, response_headers, vec![]);
    };
    let mode = state.mode.load(Ordering::Relaxed);
    if let Ok(mut records) = state.records.lock() {
        if records.len() >= 64 {
            state.overflow.store(true, Ordering::Relaxed);
            return (StatusCode::INSUFFICIENT_STORAGE, response_headers, vec![]);
        }
        records.push(Record {
            wire,
            bytes: bytes.to_vec(),
            accepted: mode == 0,
        });
    } else {
        return (StatusCode::INTERNAL_SERVER_ERROR, response_headers, vec![]);
    }
    if mode == 2 {
        state.release.notified().await;
    }
    (
        if mode == 0 {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        response_headers,
        vec![],
    )
}
