//! Real Linux process/listener observations, without a model or response metadata.
use edgeagent_contracts::explanation_profile::Deployment;
use edgeagent_research::{
    explanation_deployment::{Observation, Verifier},
    explanation_model::{DeploymentLease, DeploymentVerifier},
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    net::{Ipv4Addr, SocketAddrV4, TcpListener},
    process::Stdio,
};
use tokio::io::{AsyncBufReadExt as _, BufReader};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn observe(pid: u32, port: u16) -> Result<Observation> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    Ok(Observation {
        pid,
        port,
        start: stat
            .rsplit_once(')')
            .ok_or("stat")?
            .1
            .split_whitespace()
            .nth(19)
            .ok_or("start")?
            .into(),
        command: fs::read(format!("/proc/{pid}/cmdline"))?
            .split(|b| *b == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8(part.to_vec()))
            .collect::<std::result::Result<_, _>>()?,
        executable: Sha256::digest(fs::read(format!("/proc/{pid}/exe"))?)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    })
}
fn identity() -> Deployment {
    Deployment {
        runtime: "a".repeat(64),
        weights: "b".repeat(64),
    }
}
#[tokio::test]
async fn shared_verifier_preserves_identity_endpoint_and_listener_refusals() -> Result {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, listener.local_addr()?.port());
    let observation = observe(std::process::id(), endpoint.port())?;
    let verifier = Verifier::new(observation.clone(), identity())?;
    let lease = verifier.acquire(endpoint).await?;
    assert_eq!(lease.endpoint(), endpoint);
    assert_eq!(lease.identity(), &identity());
    drop(lease);
    assert!(
        verifier
            .acquire(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, endpoint.port()))
            .await
            .is_err()
    );
    for field in 0..5 {
        let mut changed = observation.clone();
        match field {
            0 => changed.pid = 0,
            1 => changed.port = 0,
            2 => changed.start = "0".into(),
            3 => changed.command.push("unexpected".into()),
            _ => changed.executable = "f".repeat(64),
        };
        assert!(
            Verifier::new(changed, identity()).is_err(),
            "identity substitution {field}"
        );
    }
    drop(listener);
    assert!(verifier.verify().is_err());
    assert!(verifier.acquire(endpoint).await.is_err());
    Ok(())
}
#[tokio::test]
async fn shared_verifier_rejects_foreign_listener_and_non_sibling_process() -> Result {
    let mut child=tokio::process::Command::new("python3").arg("-c").arg("import socket,sys; s=socket.socket(); s.bind(('127.0.0.1',0)); s.listen(); print(s.getsockname()[1],flush=True); sys.stdin.read()")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true).spawn()?;
    let mut stdout = BufReader::new(child.stdout.take().ok_or("stdout")?);
    let mut line = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stdout.read_line(&mut line),
    )
    .await??;
    let port = line.trim().parse()?;
    // This process does not own the child's socket, despite an exclusive listener.
    assert!(Verifier::new(observe(std::process::id(), port)?, identity()).is_err());
    // The socket owner is a child of this helper, not its supervisor's sibling.
    assert!(Verifier::new(observe(child.id().ok_or("pid")?, port)?, identity()).is_err());
    child.kill().await?;
    child.wait().await?;
    Ok(())
}
