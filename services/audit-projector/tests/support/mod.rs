use edgeagent_contracts::{
    Component, MessageEnvelope, MessageMetadata,
    research::{ResearchRecord, ResearchRequest},
};
use edgeagent_research_application::publication::{ABSTAINED, PUBLISHED};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub fn record(id: &str, abstained: bool) -> Result<ResearchRecord> {
    let bytes = if abstained {
        include_bytes!("../../../../crates/contracts/fixtures/research/v1/abstention.json")
            .as_slice()
    } else {
        include_bytes!("../../../../crates/contracts/fixtures/research/v1/artifact.json").as_slice()
    };
    let mut record = ResearchRecord::decode(bytes)?;
    match &mut record {
        ResearchRecord::Artifact(a) => a.request.request_id = id.into(),
        ResearchRecord::Abstention(a) => a.request.request_id = id.into(),
        _ => return Err("terminal".into()),
    };
    Ok(record)
}
pub fn request(record: &ResearchRecord) -> Result<ResearchRequest> {
    match record {
        ResearchRecord::Artifact(a) => Ok(a.request.clone()),
        ResearchRecord::Abstention(a) => Ok(a.request.clone()),
        _ => Err("terminal".into()),
    }
}
pub fn event(record: &ResearchRecord) -> Result<MessageEnvelope> {
    let req = request(record)?;
    let key = ResearchRecord::Request(req.clone()).digest()?;
    let partition = format!("research-request/{}/{}", req.scope, req.request_id);
    let def = if matches!(record, ResearchRecord::Artifact(_)) {
        PUBLISHED
    } else {
        ABSTAINED
    };
    Ok(MessageEnvelope::from_payload(
        MessageMetadata {
            id: record.digest()?,
            source: Component::Research.source_uri().into(),
            message_type: def.message_type().into(),
            subject: partition.clone(),
            time: "2026-10-29T00:00:00Z".into(),
            data_schema: def.data_schema().into(),
            correlation_id: key.clone(),
            causation_id: "cause-1".into(),
            idempotency_key: key,
            partition_key: partition,
            trace_parent: "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
            trace_state: None,
        },
        record,
    )?)
}
pub struct Server {
    pub address: std::net::SocketAddr,
    task: tokio::task::JoinHandle<()>,
}
impl Server {
    pub async fn new(
        store: Arc<edgeagent_audit_projector::store::ProjectionStore>,
    ) -> Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, edgeagent_audit_projector::http::router(store)).await;
        });
        Ok(Self { address, task })
    }
    pub async fn query(&self, method: &str, path: &str, headers: &str) -> Result<String> {
        tokio::time::timeout(Duration::from_secs(10), async {
            let mut socket = tokio::net::TcpStream::connect(self.address).await?;
            let host = if headers.contains("Host:") {
                String::new()
            } else {
                format!("Host: {}\r\n", self.address)
            };
            socket
                .write_all(
                    format!("{method} {path} HTTP/1.1\r\n{host}Connection: close\r\n{headers}\r\n")
                        .as_bytes(),
                )
                .await?;
            let mut result = String::new();
            socket.read_to_string(&mut result).await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(result)
        })
        .await?
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
pub fn body(response: &str) -> Result<&str> {
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .ok_or_else(|| "HTTP response framing".into())
}
