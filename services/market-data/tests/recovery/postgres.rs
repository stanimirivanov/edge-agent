//! Test-only PostgreSQL frontend gate: stop terminal COMMIT, not application code.
use super::{Duration, TestError, TestResult};
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Notify, oneshot},
    task::{JoinHandle, JoinSet},
};

pub struct CommitGate {
    pub url: String,
    reached: Arc<Notify>,
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<TestResult>,
}
impl CommitGate {
    pub async fn start(original: &str) -> Result<Self, TestError> {
        let mut url = super::fixture::postgres_url(original)?;
        let upstream = format!("127.0.0.1:{}", url.port().unwrap_or(5432));
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        url.set_host(Some("127.0.0.1"))?;
        url.set_port(Some(listener.local_addr()?.port()))
            .map_err(|()| "proxy port")?;
        let reached = Arc::new(Notify::new());
        let signal = reached.clone();
        let (stop, mut shutdown) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    _ = &mut shutdown => break,
                    accepted = listener.accept() => {
                        let (client, _) = accepted?;
                        if connections.len() >= 8 { return Err("wire connection bound".into()); }
                        let upstream = upstream.clone();
                        let signal = signal.clone();
                        connections.spawn(async move { forward(client, &upstream, signal).await });
                    }
                    Some(result) = connections.join_next(), if !connections.is_empty() => {
                        // Killed clients close abruptly. The parent verifies rollback, not TCP teardown status.
                        let _ = result?;
                    }
                }
            }
            connections.abort_all();
            while connections.join_next().await.is_some() {}
            Ok(())
        });
        Ok(Self {
            url: url.into(),
            reached,
            stop: Some(stop),
            task,
        })
    }
    pub async fn reached(&self) -> TestResult {
        tokio::time::timeout(Duration::from_secs(60), self.reached.notified()).await?;
        Ok(())
    }
    pub async fn close(&mut self) -> TestResult {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        tokio::time::timeout(Duration::from_secs(10), &mut self.task).await???;
        Ok(())
    }
}
impl Drop for CommitGate {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn packet(client: &mut TcpStream) -> Result<Vec<u8>, TestError> {
    let length = client.read_u32().await?;
    if !(8..=8192).contains(&length) {
        return Err("startup frame bound".into());
    }
    let mut bytes = vec![0; usize::try_from(length)?];
    bytes
        .get_mut(..4)
        .ok_or("header")?
        .copy_from_slice(&length.to_be_bytes());
    client
        .read_exact(bytes.get_mut(4..).ok_or("startup")?)
        .await?;
    Ok(bytes)
}
fn sql(kind: u8, bytes: &[u8]) -> Option<&[u8]> {
    match kind {
        b'Q' => bytes.strip_suffix(&[0]),
        b'P' => {
            let start = bytes.iter().position(|b| *b == 0)? + 1;
            let rest = bytes.get(start..)?;
            rest.get(..rest.iter().position(|b| *b == 0)?)
        }
        _ => None, // Bind bytes are never interpreted as SQL.
    }
}
fn terminal_insert(statement: &[u8]) -> bool {
    statement
        .windows(b"INSERT INTO edgeagent_acquisition_receipts".len())
        .any(|w| w == b"INSERT INTO edgeagent_acquisition_receipts")
}
async fn forward(mut client: TcpStream, address: &str, reached: Arc<Notify>) -> TestResult {
    let mut startup = packet(&mut client).await?;
    // Test fixture only: explicitly decline TLS/GSS negotiation, never inspect credentials.
    if matches!(startup.get(4..8), Some([4, 210, 22, 47] | [4, 210, 22, 48])) {
        client.write_all(b"N").await?;
        startup = packet(&mut client).await?;
    }
    if startup.get(4..8) != Some(&[0, 3, 0, 0]) {
        return Err("unsupported startup protocol".into());
    }
    let mut server = TcpStream::connect(address).await?;
    server.write_all(&startup).await?;
    let (mut cr, mut cw) = client.into_split();
    let (mut sr, mut sw) = server.into_split();
    let requests = async {
        let mut armed = false;
        loop {
            let kind = cr.read_u8().await?;
            let length = cr.read_u32().await?;
            if !(4..=2_097_152).contains(&length) {
                return Err::<(), TestError>("frontend frame bound".into());
            }
            let mut body = vec![0; usize::try_from(length - 4)?];
            cr.read_exact(&mut body).await?;
            if let Some(statement) = sql(kind, &body) {
                armed |= terminal_insert(statement);
                if armed && statement.eq_ignore_ascii_case(b"COMMIT") {
                    reached.notify_one();
                    std::future::pending::<()>().await;
                }
            }
            sw.write_u8(kind).await?;
            sw.write_u32(length).await?;
            sw.write_all(&body).await?;
        }
    };
    tokio::select! { result = requests => result, result = tokio::io::copy(&mut sr, &mut cw) => { result?; Ok(()) } }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_query_and_parse_frames_arm_terminal_commit() -> TestResult {
        let statement =
            b"INSERT INTO edgeagent_acquisition_receipts (request_id,event) VALUES ($1,$2)";
        let mut query = statement.to_vec();
        query.push(0);
        assert!(terminal_insert(sql(b'Q', &query).ok_or("query")?));
        let mut parse = b"cached\0".to_vec();
        parse.extend_from_slice(&query);
        parse.extend_from_slice(&[0, 0]);
        assert!(terminal_insert(sql(b'P', &parse).ok_or("parse")?));
        assert_eq!(sql(b'B', &query), None);
        assert_eq!(sql(b'P', b"unterminated"), None);
        assert_eq!(sql(b'Q', b"COMMIT"), None);
        assert_eq!(sql(b'Q', b"COMMIT\0"), Some(b"COMMIT".as_slice()));
        Ok(())
    }
}
