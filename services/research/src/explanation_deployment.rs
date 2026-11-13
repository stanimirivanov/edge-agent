//! Shared Linux observations for the existing operator-owned local supervisor.
//! A response's model name cannot establish process or listener ownership. The
//! supervisor must pin libraries/weights and prevent endpoint replacement while
//! a lease is held; this module does not launch, own, lock or reap that process.
use crate::explanation_model::{DeploymentLease, DeploymentVerifier};
use edgeagent_contracts::explanation_profile::Deployment;
use edgeagent_research_application::explanation_model::Failure;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read as _,
    net::{Ipv4Addr, SocketAddrV4},
    os::unix::fs::MetadataExt as _,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Supervisor-supplied observations, independently checked against Linux procfs.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// Supervisor-observed runtime process ID.
    pub pid: u32,
    /// Exclusive IPv4 loopback listening port.
    pub port: u16,
    /// Exact NUL-delimited process arguments, represented as UTF-8 strings.
    pub command: Vec<String>,
    /// Linux process start ticks, fencing PID reuse.
    pub start: String,
    /// SHA-256 of the reviewed executable bytes.
    pub executable: String,
}
impl Observation {
    /// Check process, arguments and listener ownership. Executable hashing is
    /// performed separately by `Verifier::new`; this observation grants nothing.
    pub fn verify(&self) -> Result<()> {
        if self.pid == 0 || self.port == 0 {
            return Err("invalid runtime".into());
        }
        let directory = format!("/proc/{}", self.pid);
        let process = fs::read_to_string(format!("{directory}/stat"))?;
        let start = process
            .rsplit_once(')')
            .ok_or("invalid runtime")?
            .1
            .split_whitespace()
            .nth(19)
            .ok_or("invalid runtime")?;
        let parent = |pid| -> Result<String> {
            let status = fs::read_to_string(format!("/proc/{pid}/status"))?;
            Ok(status
                .lines()
                .find_map(|line| line.strip_prefix("PPid:"))
                .ok_or("missing parent")?
                .trim()
                .to_owned())
        };
        if start != self.start || parent(self.pid)? != parent(std::process::id())? {
            return Err("runtime identity changed".into());
        }
        let command = fs::read(format!("{directory}/cmdline"))?;
        let expected: Vec<_> = self
            .command
            .iter()
            .flat_map(|arg| arg.bytes().chain([0]))
            .collect();
        if command != expected {
            return Err("runtime arguments changed".into());
        }
        let address = format!("0100007F:{:04X}", self.port);
        let tcp = fs::read_to_string("/proc/net/tcp")?;
        let listeners: Vec<_> = tcp
            .lines()
            .filter_map(|line| {
                let fields: Vec<_> = line.split_whitespace().collect();
                (fields.get(1) == Some(&address.as_str()) && fields.get(3) == Some(&"0A"))
                    .then(|| fields.get(9).copied())
                    .flatten()
            })
            .collect();
        let [inode] = listeners.as_slice() else {
            return Err("runtime listener is not exclusive".into());
        };
        let socket = format!("socket:[{inode}]");
        let owned = fs::read_dir(format!("{directory}/fd"))?
            .filter_map(std::result::Result::ok)
            .any(|entry| {
                fs::read_link(entry.path()).is_ok_and(|path| path.to_str() == Some(&socket))
            });
        if !owned {
            return Err("runtime does not own listener".into());
        }
        Ok(())
    }
    fn executable_stamp(&self) -> Result<[i128; 7]> {
        let directory = format!("/proc/{}", self.pid);
        let executable = fs::File::open(format!("{directory}/exe"))?;
        let mut bytes = Vec::new();
        executable.take(268_435_457).read_to_end(&mut bytes)?;
        if bytes.len() > 268_435_456 || digest(&bytes) != self.executable {
            return Err("runtime executable changed".into());
        }
        stamp(&format!("{directory}/exe"))
    }
}

/// Verified process identity and executable stamp for the existing model port.
#[derive(Clone)]
pub struct Verifier {
    stamp: [i128; 7],
    observation: Observation,
    identity: Deployment,
}
fn stamp(path: &str) -> Result<[i128; 7]> {
    let metadata = fs::metadata(path)?;
    Ok([
        i128::from(metadata.dev()),
        i128::from(metadata.ino()),
        i128::from(metadata.len()),
        i128::from(metadata.mtime()),
        i128::from(metadata.mtime_nsec()),
        i128::from(metadata.ctime()),
        i128::from(metadata.ctime_nsec()),
    ])
}
impl Verifier {
    /// Verify the observed process/listener and hash its actual executable before
    /// retaining the same metadata stamp used during subsequent checks.
    /// The supervisor independently pins runtime libraries, weights and identity.
    pub fn new(observation: Observation, identity: Deployment) -> Result<Self> {
        observation.verify()?;
        Ok(Self {
            stamp: observation.executable_stamp()?,
            observation,
            identity,
        })
    }
    /// Recheck process/listener ownership and the initially hashed executable's
    /// metadata stamp. This does not replace the supervisor's file/weights pins.
    pub fn verify(&self) -> Result<()> {
        self.observation.verify()?;
        if stamp(&format!("/proc/{}/exe", self.observation.pid))? != self.stamp {
            return Err("runtime executable changed".into());
        }
        Ok(())
    }
}
/// Lease observation under the supervisor's existing process ownership.
pub struct Lease(SocketAddrV4, Deployment);
impl DeploymentLease for Lease {
    fn endpoint(&self) -> SocketAddrV4 {
        self.0
    }
    fn identity(&self) -> &Deployment {
        &self.1
    }
}
impl DeploymentVerifier for Verifier {
    type Lease = Lease;
    async fn acquire(&self, endpoint: SocketAddrV4) -> std::result::Result<Lease, Failure> {
        self.verify().map_err(|_| Failure::DeploymentMismatch)?;
        if endpoint != SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.observation.port) {
            return Err(Failure::DeploymentMismatch);
        }
        Ok(Lease(endpoint, self.identity.clone()))
    }
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
