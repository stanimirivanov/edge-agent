//! Fresh-process native-root proof, also run inside the MarketData runtime image.
use super::*;
use std::{process::Stdio, sync::Arc};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

#[tokio::test]
async fn native_roots_and_isolated_tls_peers() -> TestResult {
    if let Ok(endpoint) = std::env::var("EDGEAGENT_TLS_PROOF_ENDPOINT") {
        let fixture = Fixture::new(1)?;
        let cancel = Cancellation::default();
        let admitted = fixture.admit(&cancel)?;
        let result = match HttpTransport::new(fixture.config(&endpoint)?) {
            Ok(client) => client.start(&admitted, at(NOW)?)?.get(None, &cancel).await,
            Err(error) => Err(error),
        };
        if std::env::var("EDGEAGENT_TLS_PROOF_EXPECT")? == "trusted" {
            assert_eq!(result?.bytes(), b"{}");
        } else {
            let error = result.err().ok_or("untrusted peer accepted")?;
            if std::env::var("EDGEAGENT_TLS_PROOF_EXPECT")? == "missing-roots" {
                assert_eq!(error.kind(), HttpErrorKind::Configuration);
            } else {
                assert_eq!(
                    error.kind(),
                    HttpErrorKind::Provider(P::Transient {
                        retry_after_ms: None
                    })
                );
            }
        }
        return Ok(());
    }
    // Check the unmodified store before injecting any generated test certificate.
    let native = rustls_native_certs::load_native_certs();
    assert!(!native.certs.is_empty(), "shipped native TLS roots missing");
    let fixture = Fixture::new(1)?;
    HttpTransport::new(fixture.config("https://127.0.0.1/data")?)?;
    let directory = std::env::temp_dir().join(format!("edgeagent-tls-{}", std::process::id()));
    std::fs::create_dir(&directory)?;
    let result = qualify(&directory).await;
    std::fs::remove_dir_all(directory)?;
    result
}

async fn qualify(directory: &std::path::Path) -> TestResult {
    let empty = directory.join("empty");
    std::fs::create_dir(&empty)?;
    for mode in ["trusted", "untrusted", "wrong-host", "missing-roots"] {
        let certificate = rcgen::generate_simple_self_signed(vec![if mode == "wrong-host" {
            "localhost".into()
        } else {
            "127.0.0.1".into()
        }])?;
        let pem = directory.join("root.pem");
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let der = STANDARD.encode(certificate.cert.der());
        let body = der
            .as_bytes()
            .chunks(64)
            .map(std::str::from_utf8)
            .collect::<Result<Vec<_>, _>>()?
            .join("\n");
        std::fs::write(
            &pem,
            format!("-----BEGIN CERTIFICATE-----\n{body}\n-----END CERTIFICATE-----\n"),
        )?;
        let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.signing_key.serialize_der())
                .into(),
        )?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let peer = tokio::spawn(async move {
            let (socket, _) = listener.accept().await?;
            let mut socket = tokio_rustls::TlsAcceptor::from(Arc::new(tls))
                .accept(socket)
                .await?;
            let mut bytes = Vec::new();
            while !bytes.ends_with(b"\r\n\r\n") && bytes.len() < 8192 {
                bytes.push(socket.read_u8().await?);
            }
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        });
        let mut command = tokio::process::Command::new(std::env::current_exe()?);
        command
            .args([
                "--exact",
                "tests::image_tls::native_roots_and_isolated_tls_peers",
                "--nocapture",
            ])
            .env_clear()
            .env(
                "EDGEAGENT_TLS_PROOF_ENDPOINT",
                format!("https://127.0.0.1:{}/data", address.port()),
            )
            .env("EDGEAGENT_TLS_PROOF_EXPECT", mode)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if mode != "untrusted" {
            command.env("SSL_CERT_DIR", &empty).env(
                "SSL_CERT_FILE",
                if mode == "missing-roots" {
                    empty.join("missing.pem")
                } else {
                    pem.clone()
                },
            );
        }
        let result = tokio::time::timeout(Duration::from_secs(10), command.output()).await;
        peer.abort();
        let output = result??;
        assert!(
            output.status.success(),
            "{mode}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}
