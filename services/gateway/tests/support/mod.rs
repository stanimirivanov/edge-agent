#![allow(
    clippy::disallowed_methods,
    reason = "Test-only isolated schema administration."
)]
use edgeagent_contracts::research::ResearchRequest;
use edgeagent_gateway::http;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub fn request() -> ResearchRequest {
    ResearchRequest {
        request_id: "request-1".into(),
        scope: "operator".into(),
        implementation: "02".repeat(32),
        configuration: "03".repeat(32),
        decision_at_ms: 100,
        lookback_sessions: 1,
        instruments: vec![format!("{:032x}", 1)],
    }
}
pub struct Server {
    pub address: std::net::SocketAddr,
    task: tokio::task::JoinHandle<()>,
}
impl Server {
    pub async fn new<S: edgeagent_gateway::admission::AdmissionStore>(
        store: Arc<S>,
    ) -> Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, http::router(store)).await;
        });
        Ok(Self { address, task })
    }
    pub async fn post(&self, body: &[u8], headers: &str) -> Result<String> {
        post(self.address, body, headers).await
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
pub async fn post(address: std::net::SocketAddr, body: &[u8], headers: &str) -> Result<String> {
    tokio::time::timeout(std::time::Duration::from_secs(5),async {
        let mut socket=tokio::net::TcpStream::connect(address).await?;
        let host = if headers.contains("Host:") { String::new() } else { format!("Host: {address}\r\n") };
        socket.write_all(format!("POST /v1/research-requests HTTP/1.1\r\n{host}Connection: close\r\nContent-Length: {}\r\n{headers}\r\n",body.len()).as_bytes()).await?;
        socket.write_all(body).await?;
        let mut response=String::new();socket.read_to_string(&mut response).await?;
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(response)
    }).await?
}
