use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use reqwest::blocking::{Client, multipart};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const VERSION: &str = "1.0.0-beta.3";

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const SERVER_ARTIFACT: (&str, &str, &str) = (
    "x64-windows/flawless.exe",
    "flawless.exe",
    "b06723cce23b6e599f39440f6781b68ea45d6a96a3c0761715fbcb1bd729fccf",
);

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const SERVER_ARTIFACT: (&str, &str, &str) = (
    "x64-linux/flawless",
    "flawless",
    "b511598babcde8a8bc7898f6e0c7b06b3f51a2d26227fba1cc480b06af5a215c",
);

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct MockEffects {
    address: SocketAddr,
    a: Arc<AtomicUsize>,
    b: Arc<AtomicUsize>,
    c: Arc<AtomicUsize>,
    release_first_b: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    b_started: mpsc::Receiver<()>,
    c_completed: mpsc::Receiver<()>,
}

impl MockEffects {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind synthetic effects server");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let address = listener.local_addr().expect("effects server address");
        let a = Arc::new(AtomicUsize::new(0));
        let b = Arc::new(AtomicUsize::new(0));
        let c = Arc::new(AtomicUsize::new(0));
        let release_first_b = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let (b_tx, b_started) = mpsc::channel();
        let (c_tx, c_completed) = mpsc::channel();
        let a_worker = Arc::clone(&a);
        let b_worker = Arc::clone(&b);
        let c_worker = Arc::clone(&c);
        let release_worker = Arc::clone(&release_first_b);
        let stop_worker = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !stop_worker.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => handle_effect(
                        stream,
                        &a_worker,
                        &b_worker,
                        &c_worker,
                        &release_worker,
                        &stop_worker,
                        &b_tx,
                        &c_tx,
                    ),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("effects server accept: {error}"),
                }
            }
        });
        Self {
            address,
            a,
            b,
            c,
            release_first_b,
            stop,
            thread: Some(thread),
            b_started,
            c_completed,
        }
    }
}

impl Drop for MockEffects {
    fn drop(&mut self) {
        self.release_first_b.store(true, Ordering::SeqCst);
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_effect(
    mut stream: TcpStream,
    a: &AtomicUsize,
    b: &AtomicUsize,
    c: &AtomicUsize,
    release_first_b: &AtomicBool,
    stop: &AtomicBool,
    b_started: &mpsc::Sender<()>,
    c_completed: &mpsc::Sender<()>,
) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set request timeout");
    let mut request = Vec::new();
    let mut buffer = [0_u8; 2048];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let n = stream.read(&mut buffer).expect("read synthetic request");
        assert!(n > 0, "effect connection closed before headers");
        request.extend_from_slice(&buffer[..n]);
        assert!(request.len() < 8192, "oversized synthetic request");
    }
    let request_line = String::from_utf8_lossy(&request);
    let path = request_line
        .split_whitespace()
        .nth(1)
        .expect("request path");
    match path {
        "/a" => {
            a.fetch_add(1, Ordering::SeqCst);
        }
        "/b" => {
            if b.fetch_add(1, Ordering::SeqCst) == 0 {
                b_started.send(()).expect("signal B started");
                while !release_first_b.load(Ordering::SeqCst) && !stop.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
        "/c" => {
            c.fetch_add(1, Ordering::SeqCst);
            c_completed.send(()).expect("signal C complete");
        }
        other => panic!("unexpected effect path: {other}"),
    }
    let _ =
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
}

fn open_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("reserve ephemeral port")
        .local_addr()
        .expect("ephemeral port")
        .port()
}

fn provision_server(temp: &Path, client: &Client) -> PathBuf {
    let (artifact, filename, expected_hash) = SERVER_ARTIFACT;
    let url = format!("https://downloads.flawless.dev/{VERSION}/{artifact}");
    let bytes = client
        .get(url)
        .send()
        .expect("download pinned official Flawless server")
        .error_for_status()
        .expect("Flawless server download status")
        .bytes()
        .expect("read Flawless binary");
    let actual_hash = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(
        actual_hash, expected_hash,
        "Flawless binary digest mismatch"
    );
    let path = temp.join(filename);
    fs::write(&path, bytes).expect("save verified Flawless server");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&path)
            .expect("Flawless metadata")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).expect("make Flawless executable");
    }
    path
}

fn build_workflow(temp: &Path) -> PathBuf {
    let installed = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .expect("rustup required by pinned Rust toolchain");
    assert!(installed.status.success(), "rustup target inventory failed");
    if !String::from_utf8_lossy(&installed.stdout).contains("wasm32-unknown-unknown") {
        let status = Command::new("rustup")
            .args(["target", "add", "wasm32-unknown-unknown"])
            .status()
            .expect("auto-provision Wasm standard library");
        assert!(status.success(), "Wasm target auto-provisioning failed");
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("workflows/m08/Cargo.toml");
    let target = temp.join("wasm-target");
    let status = Command::new("cargo")
        .args(["build", "--locked", "--manifest-path"])
        .arg(manifest)
        .args(["--target", "wasm32-unknown-unknown"])
        .env("CARGO_TARGET_DIR", &target)
        .status()
        .expect("build Flawless workflow");
    assert!(status.success(), "Flawless Wasm build failed");
    target.join("wasm32-unknown-unknown/debug/edgeagent_flawless_m08.wasm")
}

fn start_server(binary: &Path, data: &Path, port: u16, client: &Client) -> Server {
    let child = Command::new(binary)
        .args(["up", "--working-dir"])
        .arg(data)
        .args(["--bind", &format!("127.0.0.1:{port}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start Flawless server");
    let mut server = Server(child);
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Some(status) = server.0.try_wait().expect("poll Flawless server") {
            panic!("Flawless server exited before ready: {status}");
        }
        if client
            .get(format!("http://127.0.0.1:{port}/version"))
            .send()
            .is_ok()
        {
            return server;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("Flawless server did not become ready");
}

#[cfg(any(
    all(target_os = "windows", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "x86_64")
))]
#[test]
fn recorded_effects_resume_after_server_process_crash() {
    let temp = TempDir::new().expect("isolated test directory");
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .expect("HTTP client");
    let binary = provision_server(temp.path(), &client);
    let wasm = build_workflow(temp.path());
    let effects = MockEffects::start();
    let data = temp.path().join("server-data");
    let port = open_port();
    let mut server = start_server(&binary, &data, port, &client);
    let base = format!("http://127.0.0.1:{port}");
    let response = client
        .post(format!("{base}/api/module/deploy"))
        .multipart(
            multipart::Form::new().part(
                "module",
                multipart::Part::bytes(fs::read(wasm).expect("read Wasm module"))
                    .file_name("edgeagent_flawless_m08.wasm"),
            ),
        )
        .send()
        .expect("deploy Flawless module");
    assert!(
        response.status().is_success(),
        "module deploy: {response:?}"
    );

    let effect_base = format!("http://{}", effects.address);
    let start_client = client.clone();
    let start_request = thread::spawn(move || {
        start_client
            .post(format!("{base}/api/workflow/start"))
            .json(&serde_json::json!({
                "module": "edgeagent_m08",
                "version": "0.0.1",
                "workflow": "m08",
                "input": serde_json::to_string(&effect_base).expect("encode workflow input"),
            }))
            .send()
    });
    effects
        .b_started
        .recv_timeout(Duration::from_secs(20))
        .expect("B must start before crash");
    server.0.kill().expect("kill Flawless server during B");
    server.0.wait().expect("reap crashed Flawless server");
    effects.release_first_b.store(true, Ordering::SeqCst);
    drop(server);
    let _ = start_request.join(); // The in-flight HTTP call may fail on server crash.

    let _restarted = start_server(&binary, &data, port, &client);
    effects
        .c_completed
        .recv_timeout(Duration::from_secs(30))
        .expect("workflow must resume and finish C");
    assert_eq!(effects.a.load(Ordering::SeqCst), 1, "A effect repeated");
    assert!(effects.b.load(Ordering::SeqCst) >= 2, "B was not retried");
    assert_eq!(effects.c.load(Ordering::SeqCst), 1, "C effect count");
}
