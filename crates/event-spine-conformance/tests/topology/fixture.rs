use std::{
    error::Error,
    fs,
    future::Future,
    net::TcpListener,
    panic::AssertUnwindSafe,
    path::PathBuf,
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use async_nats::{ConnectOptions, jetstream};
use edgeagent_messaging_nats::topology::{SubjectPermissions, TopologyPlan};
use futures_util::FutureExt;
use tokio::process::Command;

pub type TestError = Box<dyn Error + Send + Sync>;
pub type TestResult = Result<(), TestError>;
pub const DEADLINE: Duration = Duration::from_secs(5);
const IMAGE: &str = "nats:2.14.7-alpine";
static NEXT: AtomicU64 = AtomicU64::new(1);

enum Runtime {
    Native(Box<tokio::process::Child>),
    Container(String),
}
pub struct Broker {
    directory: PathBuf,
    runtime: Option<Runtime>,
    url: String,
}

impl Broker {
    fn new() -> Result<Self, TestError> {
        let directory = std::env::temp_dir().join(format!(
            "edgeagent-topology-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory)?; // Exclusive creation establishes cleanup ownership.
        Ok(Self {
            directory,
            runtime: None,
            url: String::new(),
        })
    }

    async fn start(&mut self, plan: &TopologyPlan, authenticated: bool) -> TestResult {
        let mut users = vec![
            format!(
                "{{ user: provisioner, password: fixture-only, permissions: {} }}",
                plan.provisioner_permissions().nats_config()
            ),
            "{ user: inspector, password: fixture-only }".into(),
        ];
        for role in plan.runtime_permissions() {
            let name = role
                .component()
                .ok_or("runtime component missing")?
                .as_str();
            users.push(format!(
                "{{ user: {name:?}, password: fixture-only, permissions: {} }}",
                role.nats_config()
            ));
            if let Ok(replay) = edgeagent_event_spine_conformance::replay::ReplayPublisherScope::new(
                plan,
                role.component().ok_or("runtime component missing")?,
            ) {
                users.push(format!(
                    "{{ user: {:?}, password: fixture-only, permissions: {} }}",
                    replay.user(),
                    replay.nats_config()
                ));
            }
        }
        let native = std::env::var_os("EDGEAGENT_NATS_SERVER");
        let store = if native.is_some() {
            self.directory
                .join("store")
                .to_string_lossy()
                .replace('\\', "/")
        } else {
            "/data".into()
        };
        fs::write(
            self.directory.join("nats.conf"),
            format!(
                "jetstream {{ store_dir: {store:?}, max_memory_store: 64MB, max_file_store: 256MB }}\n{}\n",
                if authenticated {
                    format!("authorization {{ users: [{}] }}", users.join(",\n"))
                } else {
                    String::new()
                }
            ),
        )?;
        if let Some(executable) = native {
            let version = tokio::time::timeout(
                DEADLINE,
                Command::new(&executable)
                    .arg("--version")
                    .kill_on_drop(true)
                    .output(),
            )
            .await??;
            if !version.status.success()
                || String::from_utf8(version.stdout)?.trim() != "nats-server: v2.14.7"
            {
                return Err("native fixture requires pinned nats-server 2.14.7".into());
            }
            let listener = TcpListener::bind(("127.0.0.1", 0))?;
            let port = listener.local_addr()?.port();
            drop(listener); // A bind race fails startup; it never authorizes a foreign endpoint.
            let child = Command::new(executable)
                .args(["-a", "127.0.0.1", "-p", &port.to_string(), "-c"])
                .arg(self.directory.join("nats.conf"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()?;
            self.runtime = Some(Runtime::Native(Box::new(child)));
            self.url = format!("nats://127.0.0.1:{port}");
        } else {
            let mount = format!(
                "type=bind,source={},target=/etc/nats/fixture.conf,readonly",
                self.directory.join("nats.conf").display()
            );
            let name = self
                .directory
                .file_name()
                .ok_or("fixture name absent")?
                .to_str()
                .ok_or("fixture name invalid")?;
            let id = docker(&[
                "create",
                "--name",
                name,
                "--label",
                "edgeagent.proof=topology",
                "--security-opt",
                "no-new-privileges:true",
                "--memory",
                "256m",
                "--publish",
                "127.0.0.1::4222",
                "--mount",
                &mount,
                IMAGE,
                "-c",
                "/etc/nats/fixture.conf",
            ])
            .await?;
            let id = id.trim();
            if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("invalid owned container receipt".into());
            }
            self.runtime = Some(Runtime::Container(id.to_owned()));
            docker(&["start", id]).await?;
            self.url = container_url(id).await?;
        }
        self.ready().await
    }

    #[allow(
        dead_code,
        reason = "Shared fixture: only the process target uses restart control"
    )]
    pub fn url(&self) -> &str {
        &self.url
    }

    #[allow(
        dead_code,
        reason = "Shared fixture: only the process target uses restart control"
    )]
    pub async fn restart(&mut self) -> TestResult {
        match self.runtime.as_mut().ok_or("owned broker missing")? {
            Runtime::Native(child) => {
                tokio::time::timeout(DEADLINE, child.kill()).await??;
                let port = self.url.rsplit(':').next().ok_or("owned port absent")?;
                **child = Command::new(
                    std::env::var_os("EDGEAGENT_NATS_SERVER").ok_or("native executable absent")?,
                )
                .args(["-a", "127.0.0.1", "-p", port, "-c"])
                .arg(self.directory.join("nats.conf"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()?;
            }
            Runtime::Container(id) => {
                docker(&["kill", id]).await?;
                // Same container's writable layer retains /data. Never remove
                // or recreate the broker between kill and restart.
                docker(&["start", id]).await?;
                // Docker can reallocate an ephemeral host port on restart.
                // Fresh clients must use the owned container's current mapping.
                self.url = container_url(id).await?;
            }
        }
        self.ready().await
    }

    async fn ready(&mut self) -> TestResult {
        // Readiness follows an authenticated round trip, not an arbitrary sleep.
        tokio::time::timeout(Duration::from_secs(30), async {
            let mut poll = tokio::time::interval(Duration::from_millis(50));
            loop {
                poll.tick().await;
                if let Some(Runtime::Native(child)) = &mut self.runtime
                    && child.try_wait()?.is_some()
                {
                    return Err::<(), TestError>("owned broker exited before readiness".into());
                }
                if let Ok(client) = connect(&self.url, "inspector", "_INBOX.inspector").await {
                    client.flush().await?;
                    return Ok(());
                }
            }
        })
        .await??;
        Ok(())
    }

    async fn close(&mut self) -> TestResult {
        match self.runtime.take() {
            Some(Runtime::Native(mut child)) => {
                if child.try_wait()?.is_none() {
                    tokio::time::timeout(DEADLINE, child.kill()).await??;
                }
            }
            Some(Runtime::Container(id)) => {
                docker(&["rm", "--force", "--volumes", &id]).await?;
            }
            None => (),
        }
        fs::remove_dir_all(&self.directory)?;
        Ok(())
    }
}

// Native child kill-on-drop is a final safety net. Normal error, panic, and
// deadline paths await explicit cleanup. Disposable CI also bounds process abort.
impl Drop for Broker {
    fn drop(&mut self) {
        if self.runtime.is_none() {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

async fn docker(arguments: &[&str]) -> Result<String, TestError> {
    let output = tokio::time::timeout(
        Duration::from_secs(60),
        Command::new("docker")
            .args(arguments)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    if !output.status.success() {
        return Err(
            "owned Docker fixture operation failed; inspect isolated proof environment".into(),
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}

async fn container_url(id: &str) -> Result<String, TestError> {
    let mapping = docker(&["port", id, "4222/tcp"]).await?;
    let address = mapping
        .trim()
        .strip_prefix("127.0.0.1:")
        .ok_or("fixture port must bind loopback")?;
    let port: u16 = address.parse()?;
    if port == 0 {
        return Err("invalid owned broker port".into());
    }
    Ok(format!("nats://127.0.0.1:{port}"))
}

pub async fn with_broker<F, Fut>(plan: &TopologyPlan, body: F) -> TestResult
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = TestResult>,
{
    with_controlled_broker(plan, |broker| async move {
        let url = broker.lock().await.url.clone();
        body(url).await
    })
    .await
}

pub async fn with_controlled_broker<F, Fut>(plan: &TopologyPlan, body: F) -> TestResult
where
    F: FnOnce(std::sync::Arc<tokio::sync::Mutex<Broker>>) -> Fut,
    Fut: Future<Output = TestResult>,
{
    controlled_broker(plan, Duration::from_secs(90), true, body).await
}

// M03 retains the product's 30s ACK wait rather than M02's accelerated fixture.
// Its finite multi-worker scenarios need a correspondingly explicit deadline.
#[allow(
    dead_code,
    reason = "Shared M03 fixture, not used by authenticated M02 proof targets"
)]
pub async fn with_offline_broker<F, Fut>(
    plan: &TopologyPlan,
    deadline: Duration,
    body: F,
) -> TestResult
where
    F: FnOnce(std::sync::Arc<tokio::sync::Mutex<Broker>>) -> Fut,
    Fut: Future<Output = TestResult>,
{
    // Trusted-local offline composition has no authenticated NATS factory.
    // This dedicated loopback broker never weakens M02 permission proofs.
    controlled_broker(plan, deadline, false, body).await
}

async fn controlled_broker<F, Fut>(
    plan: &TopologyPlan,
    deadline: Duration,
    authenticated: bool,
    body: F,
) -> TestResult
where
    F: FnOnce(std::sync::Arc<tokio::sync::Mutex<Broker>>) -> Fut,
    Fut: Future<Output = TestResult>,
{
    let broker = std::sync::Arc::new(tokio::sync::Mutex::new(Broker::new()?));
    let outcome = AssertUnwindSafe(async {
        broker.lock().await.start(plan, authenticated).await?;
        tokio::time::timeout(deadline, body(broker.clone())).await?
    })
    .catch_unwind()
    .await;
    let cleanup = broker.lock().await.close().await;
    match outcome {
        Ok(Ok(())) => cleanup,
        Ok(Err(error)) => {
            if cleanup.is_err() {
                eprintln!("owned broker cleanup also failed");
            }
            Err(error)
        }
        Err(panic) => {
            if cleanup.is_err() {
                eprintln!("owned broker cleanup also failed after panic");
            }
            std::panic::resume_unwind(panic)
        }
    }
}

pub async fn connect(url: &str, user: &str, inbox: &str) -> Result<async_nats::Client, TestError> {
    Ok(tokio::time::timeout(
        DEADLINE,
        ConnectOptions::new()
            .user_and_password(user.into(), "fixture-only".into())
            .custom_inbox_prefix(inbox)
            .request_timeout(Some(DEADLINE))
            .connect(url),
    )
    .await??)
}

pub async fn context(
    url: &str,
    role: &SubjectPermissions,
) -> Result<jetstream::Context, TestError> {
    let user = role
        .component()
        .map_or("provisioner", |component| component.as_str());
    Ok(jetstream::ContextBuilder::new()
        .timeout(DEADLINE)
        .build(connect(url, user, role.inbox_prefix()).await?))
}

pub fn pinned_image_matches_local_profile() -> bool {
    include_str!("../../../../deploy/local/stack.toml").contains(&format!("image = \"{IMAGE}\""))
}
