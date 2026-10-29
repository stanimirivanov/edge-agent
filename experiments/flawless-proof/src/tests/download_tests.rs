use super::*;
use std::{net::TcpListener, time::Instant};
use tempfile::TempDir;

fn serve(responses: Vec<&'static [u8]>) -> (String, thread::JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("local artifact fixture");
    let url = format!(
        "http://{}/artifact",
        listener.local_addr().expect("fixture address")
    );
    listener
        .set_nonblocking(true)
        .expect("bounded fixture accept");
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut sent = 0;
        for response in responses {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            return sent;
                        }
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(e) => panic!("artifact fixture accept: {e}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("fixture read bound");
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .expect("fixture write bound");
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                stream
                    .read_exact(&mut byte)
                    .expect("artifact request headers");
                request.push(byte[0]);
                assert!(request.len() <= 8192);
            }
            stream.write_all(response).expect("artifact response");
            sent += 1;
        }
        sent
    });
    (url, worker)
}
const COMPLETE: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nverified";
const TRUNCATED: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nx";
const UNAVAILABLE: &[u8] =
    b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const MISSING: &[u8] = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const UNSIZED: &[u8] = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nverified";
fn policy() -> Policy {
    Policy {
        timeout: Duration::from_secs(5),
        backoff: Duration::ZERO,
        attempts: 3,
        max_bytes: 32,
    }
}
fn hash() -> String {
    format!("{:x}", Sha256::digest(b"verified"))
}

#[test]
fn truncated_body_restarts_fresh_and_exposes_only_verified_binary() {
    let temp = TempDir::new().expect("artifact directory");
    let path = temp.path().join("flawless");
    let (url, server) = serve(vec![TRUNCATED, COMPLETE]);
    download(
        &Client::builder().no_proxy().build().expect("local client"),
        &url,
        &path,
        &hash(),
        policy(),
    )
    .expect("retry truncated body");
    assert_eq!(server.join().expect("fixture finished"), 2);
    assert_eq!(fs::read(&path).expect("verified file"), b"verified");
    assert!(!path.with_extension("partial").exists());
    assert!(DOWNLOAD.timeout > Duration::from_secs(20));
    assert_eq!(DOWNLOAD.attempts, 3);
}

#[test]
fn transient_status_retries_with_the_same_pinned_digest() {
    let temp = TempDir::new().expect("artifact directory");
    let path = temp.path().join("flawless");
    let (url, server) = serve(vec![UNAVAILABLE, COMPLETE]);
    download(
        &Client::builder().no_proxy().build().expect("local client"),
        &url,
        &path,
        &hash(),
        policy(),
    )
    .expect("retry transient HTTP status");
    assert_eq!(server.join().expect("fixture finished"), 2);
    assert_eq!(fs::read(&path).expect("verified file"), b"verified");
}

#[test]
fn terminal_rejections_never_publish_an_executable_or_retry() {
    for (response, expected_hash, max_bytes) in [
        (MISSING, hash(), 32),
        (COMPLETE, "incorrect".into(), 32),
        (COMPLETE, hash(), 4),
        (UNSIZED, hash(), 4),
    ] {
        let temp = TempDir::new().expect("artifact directory");
        let path = temp.path().join("flawless");
        let (url, server) = serve(vec![response]);
        let result = download(
            &Client::builder().no_proxy().build().expect("local client"),
            &url,
            &path,
            &expected_hash,
            Policy {
                max_bytes,
                ..policy()
            },
        );
        assert!(matches!(
            result,
            Err(Error::Status(StatusCode::NOT_FOUND) | Error::DigestMismatch | Error::TooLarge)
        ));
        assert_eq!(server.join().expect("fixture finished"), 1);
        assert!(!path.exists());
        assert!(!path.with_extension("partial").exists());
    }
}

#[test]
fn exhausted_body_retries_remove_all_partial_bytes() {
    let temp = TempDir::new().expect("artifact directory");
    let path = temp.path().join("flawless");
    let (url, server) = serve(vec![TRUNCATED; 3]);
    let result = download(
        &Client::builder().no_proxy().build().expect("local client"),
        &url,
        &path,
        &hash(),
        policy(),
    );
    assert!(matches!(result, Err(Error::Read(_))));
    assert_eq!(server.join().expect("fixture finished"), 3);
    assert!(!path.exists());
    assert!(!path.with_extension("partial").exists());
}
