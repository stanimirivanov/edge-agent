//! Artifact provisioning has a different latency budget from localhost workflow controls.
use reqwest::{StatusCode, blocking::Client};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
    thread,
    time::Duration,
};

#[derive(Clone, Copy)]
struct Policy {
    timeout: Duration,
    backoff: Duration,
    attempts: u32,
    max_bytes: u64,
}
const DOWNLOAD: Policy = Policy {
    timeout: Duration::from_secs(120),
    backoff: Duration::from_secs(1),
    attempts: 3,
    max_bytes: 512 * 1024 * 1024,
};

#[derive(Debug)]
pub(super) enum Error {
    Transport(reqwest::Error),
    Read(io::Error),
    File(io::Error),
    Status(StatusCode),
    TooLarge,
    DigestMismatch,
    NoAttempts,
}
impl Error {
    fn retryable(&self) -> bool {
        matches!(self, Self::Transport(_) | Self::Read(_))
            || matches!(self, Self::Status(s) if s.is_server_error() || *s == StatusCode::TOO_MANY_REQUESTS)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "artifact request: {e}"),
            Self::Read(e) => write!(f, "artifact body: {e}"),
            Self::File(e) => write!(f, "artifact file: {e}"),
            Self::Status(s) => write!(f, "artifact HTTP status: {s}"),
            Self::TooLarge => f.write_str("artifact exceeds explicit byte limit"),
            Self::DigestMismatch => f.write_str("artifact SHA-256 differs from pinned digest"),
            Self::NoAttempts => f.write_str("artifact policy permits no download attempts"),
        }
    }
}
impl std::error::Error for Error {}

pub(super) fn provision(url: &str, path: &Path, hash: &str) -> Result<(), Error> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(Error::Transport)?;
    download(&client, url, path, hash, DOWNLOAD)
}

fn download(
    client: &Client,
    url: &str,
    path: &Path,
    hash: &str,
    policy: Policy,
) -> Result<(), Error> {
    // A partial file is never the executable path. Every retry truncates it and starts
    // a fresh GET/hash; never concatenate an uncertain prefix or resume unverified bytes.
    let partial = path.with_extension("partial");
    for attempt in 1..=policy.attempts {
        match transfer(client, url, &partial, hash, policy) {
            Ok(()) => {
                if let Err(error) = fs::rename(&partial, path) {
                    let _ = fs::remove_file(&partial);
                    return Err(Error::File(error));
                }
                return Ok(());
            }
            Err(error) => {
                let _ = fs::remove_file(&partial);
                if !error.retryable() || attempt == policy.attempts {
                    return Err(error);
                }
                thread::sleep(policy.backoff * attempt);
            }
        }
    }
    Err(Error::NoAttempts)
}

fn transfer(
    client: &Client,
    url: &str,
    partial: &Path,
    hash: &str,
    policy: Policy,
) -> Result<(), Error> {
    // This deadline covers headers AND the entire streamed body, independent of the
    // proof's short control client. Reqwest's default TLS verification remains enabled.
    let mut response = client
        .get(url)
        .timeout(policy.timeout)
        .send()
        .map_err(Error::Transport)?;
    if !response.status().is_success() {
        return Err(Error::Status(response.status()));
    }
    if response
        .content_length()
        .is_some_and(|n| n > policy.max_bytes)
    {
        return Err(Error::TooLarge);
    }
    let mut file = fs::File::create(partial).map_err(Error::File)?;
    let mut digest = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let n = response.read(&mut buffer).map_err(Error::Read)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        if size > policy.max_bytes {
            return Err(Error::TooLarge);
        }
        digest.update(&buffer[..n]);
        file.write_all(&buffer[..n]).map_err(Error::File)?;
    }
    if format!("{:x}", digest.finalize()) != hash {
        return Err(Error::DigestMismatch);
    }
    file.flush().map_err(Error::File)
}

#[cfg(test)]
#[path = "download_tests.rs"]
mod tests;
