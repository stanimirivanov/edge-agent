//! Bounded single TCP/TLS/HTTP upgrade, with no detached connection driver.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use hyper::{Request, StatusCode, body::Bytes, header};
use sha1::{Digest, Sha1};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

struct Driver(tokio::task::JoinHandle<()>);
impl Drop for Driver {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(super) async fn open(transport: &WssTransport, session: &StreamingSession) -> Result<Socket> {
    let config = &transport.config;
    let host = match config.url.host().ok_or(WssErrorKind::Configuration)? {
        url::Host::Domain(name) => name.to_owned(),
        url::Host::Ipv4(ip) => ip.to_string(),
        url::Host::Ipv6(ip) => ip.to_string(),
    };
    let port = config.url.port().unwrap_or(443);
    // Select exactly one resolved address: TcpStream::connect(host) otherwise tries many addresses.
    let address = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(protocol)?
        .next()
        .ok_or(WssErrorKind::Protocol)?;
    let tcp = TcpStream::connect(address).await.map_err(protocol)?;
    tcp.set_nodelay(true).map_err(protocol)?;
    let server_name = rustls::pki_types::ServerName::try_from(host.to_owned())
        .map_err(|_| WssErrorKind::Configuration)?;
    let tls = TlsConnector::from(transport.tls.clone())
        .connect(server_name, tcp)
        .await
        .map_err(protocol)?;
    let mut nonce = [0_u8; 16];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut nonce)
        .map_err(|_| WssErrorKind::Protocol)?;
    let key = STANDARD.encode(nonce);
    let mut sha = Sha1::new();
    sha.update(key.as_bytes());
    sha.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    // SHA-1 is required only for RFC6455's handshake challenge, never evidence/security digests.
    let expected = STANDARD.encode(sha.finalize());
    let host_header = match config.url.port() {
        Some(p) => format!(
            "{}:{p}",
            config.url.host().ok_or(WssErrorKind::Configuration)?
        ),
        None => config
            .url
            .host()
            .ok_or(WssErrorKind::Configuration)?
            .to_string(),
    };
    let mut request = Request::builder()
        .method("GET")
        .uri(config.url.path())
        .header(header::HOST, host_header)
        .header(header::UPGRADE, "websocket")
        .header(header::CONNECTION, "Upgrade")
        .header("sec-websocket-key", key)
        .header("sec-websocket-version", "13");
    if let Some(value) = &config.authorization {
        request = request.header(header::AUTHORIZATION, value.clone());
    }
    let request = request
        .body(http_body_util::Empty::<Bytes>::new())
        .map_err(protocol)?;
    let (mut sender, connection) = hyper::client::conn::http1::Builder::new()
        .max_buf_size(config.upgrade_bytes)
        .max_headers(32)
        .handshake(TokioIo::new(tls))
        .await
        .map_err(protocol)?;
    let _driver = Driver(tokio::spawn(async move {
        let _ = connection.with_upgrades().await;
    }));
    let mut response = sender.send_request(request).await.map_err(protocol)?;
    let headers = response.headers();
    let exact = |name: &str, expected: &str| {
        headers.get_all(name).iter().count() == 1
            && headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.eq_ignore_ascii_case(expected))
    };
    if response.status() != StatusCode::SWITCHING_PROTOCOLS
        || !exact("upgrade", "websocket")
        || !exact("connection", "Upgrade")
        || headers.get_all("sec-websocket-accept").iter().count() != 1
        || headers
            .get("sec-websocket-accept")
            .and_then(|v| v.to_str().ok())
            != Some(expected.as_str())
        || headers.contains_key("sec-websocket-extensions")
        || headers.contains_key("sec-websocket-protocol")
    {
        return Err(WssErrorKind::Protocol.into());
    }
    let upgraded = hyper::upgrade::on(&mut response).await.map_err(protocol)?;
    let mut socket =
        WebSocket::after_handshake(TokioIo::new(upgraded), fastwebsockets::Role::Client);
    // The codec rejects >=, whereas the admission's ceiling is inclusive.
    socket.set_max_message_size(session.data().limits.frame_bytes.get() as usize + 1);
    socket.set_auto_pong(false);
    socket.set_auto_close(false);
    socket.set_writev(false);
    Ok(socket)
}
fn protocol(e: impl std::error::Error + Send + Sync + 'static) -> WssError {
    WssError::caused(WssErrorKind::Protocol, e)
}
