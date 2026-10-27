//! Owned ephemeral verified-TLS peer, scripted against the real transport.
use super::*;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub(super) struct Peer {
    pub url: String,
    pub roots: rustls::RootCertStore,
    pub commands: Arc<Mutex<Vec<Vec<u8>>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort()
    }
}
pub(super) async fn start(frames: Vec<(bool, u8, Vec<u8>)>, silent: bool) -> TestResult<Peer> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use sha1::{Digest, Sha1};
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()])?;
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert.der().clone())?;
    let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(
        vec![cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
    )?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("wss://{}/v2/iex", listener.local_addr()?);
    let commands = Arc::new(Mutex::new(Vec::new()));
    let recorded = commands.clone();
    let task = tokio::spawn(async move {
        let work = async {
            let (tcp, _) = listener.accept().await?;
            let mut socket = tokio_rustls::TlsAcceptor::from(Arc::new(tls))
                .accept(tcp)
                .await?;
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
                request.push(socket.read_u8().await?)
            }
            let request = std::str::from_utf8(&request)?;
            let key = request
                .lines()
                .find_map(|s| {
                    s.split_once(':')
                        .filter(|(k, _)| k.eq_ignore_ascii_case("sec-websocket-key"))
                        .map(|(_, v)| v.trim())
                })
                .ok_or("key")?;
            let mut hash = Sha1::new();
            hash.update(key);
            hash.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
            socket.write_all(format!("HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: {}\r\n\r\n",STANDARD.encode(hash.finalize())).as_bytes()).await?;
            socket.flush().await?;
            if silent {
                std::future::pending::<()>().await
            }
            let mut ws =
                fastwebsockets::WebSocket::after_handshake(socket, fastwebsockets::Role::Server);
            ws.write_frame(fastwebsockets::Frame::text(
                fastwebsockets::Payload::Borrowed(WELCOME),
            ))
            .await?;
            let auth = ws.read_frame().await?;
            let auth: serde_json::Value = serde_json::from_slice(&auth.payload)?;
            if auth != serde_json::json!({"action":"auth","key":"invented","secret":"invented"}) {
                return Err("auth".into());
            }
            recorded
                .lock()
                .map_err(|_| "lock")?
                .push(serde_json::to_vec(&auth)?);
            ws.write_frame(fastwebsockets::Frame::text(
                fastwebsockets::Payload::Borrowed(AUTH),
            ))
            .await?;
            let sub = ws.read_frame().await?;
            let sub: serde_json::Value = serde_json::from_slice(&sub.payload)?;
            if sub != serde_json::json!({"action":"subscribe","trades":["SYNTH1"]}) {
                return Err("subscription".into());
            }
            recorded
                .lock()
                .map_err(|_| "lock")?
                .push(serde_json::to_vec(&sub)?);
            ws.write_frame(fastwebsockets::Frame::text(
                fastwebsockets::Payload::Borrowed(ACK),
            ))
            .await?;
            for (fin, op, bytes) in frames {
                let opcode = match op {
                    0 => fastwebsockets::OpCode::Continuation,
                    1 => fastwebsockets::OpCode::Text,
                    9 => fastwebsockets::OpCode::Ping,
                    _ => return Err("opcode".into()),
                };
                ws.write_frame(fastwebsockets::Frame::new(
                    fin,
                    opcode,
                    None,
                    fastwebsockets::Payload::Owned(bytes),
                ))
                .await?;
                if op == 9 {
                    let frame = ws.read_frame().await?;
                    if frame.opcode != fastwebsockets::OpCode::Pong
                        || frame.payload.as_ref() != b"ping"
                    {
                        return Err("pong".into());
                    }
                }
            }
            std::future::pending::<()>().await;
            TestResult::Ok(())
        };
        let _ = work.await;
    });
    Ok(Peer {
        url,
        roots,
        commands,
        task,
    })
}
