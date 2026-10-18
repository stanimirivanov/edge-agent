//! Owned dependency lifecycle around the canonical verification runner.

use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const IMAGE: &str = "postgres:18.6-alpine3.23";
const REPORT_ROOT: &str = "artifacts/m02-qualification";

pub(crate) fn run() -> ExitCode {
    match qualify() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("M02 qualification failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn qualify() -> Result<ExitCode, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system clock unavailable")?
        .as_nanos();
    let run_id = format!("m02-{}-{timestamp}", std::process::id());
    let parent = super::verification::workspace_root().join(REPORT_ROOT);
    fs::create_dir_all(&parent).map_err(|_| "cannot create report parent")?;
    let directory = parent.join(&run_id);
    fs::create_dir(&directory).map_err(|_| "cannot exclusively create run directory")?;
    let report = directory.join("results.tsv");
    fs::write(
        &report,
        format!("kind\tidentity\tresult\telapsed_ms\nfixture\t{IMAGE}\tpinned\t-\n"),
    )
    .map_err(|_| "cannot initialize report")?;
    println!("M02 qualification report: {}", report.display());
    let mut database = Database {
        directory,
        label: format!("edgeagent.qualification={run_id}"),
        cleaned: false,
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let database_url = database.start()?;
        append(&report, "setup\towned-postgres\tpassed\t-\n")?;
        Ok::<_, String>(super::verification::qualify(&database_url, &report))
    }));
    let cleanup = database.close();
    append(
        &report,
        if cleanup.is_ok() {
            "cleanup\towned-postgres\tpassed\t-\n"
        } else {
            "cleanup\towned-postgres\tfailed\t-\n"
        },
    )?;
    let code = match outcome {
        Ok(Ok(code)) => code,
        Ok(Err(error)) => {
            append(&report, "setup\tqualification\tfailed-not-qualified\t-\n")?;
            eprintln!("{error}");
            ExitCode::FAILURE
        }
        Err(_) => {
            append(
                &report,
                "execution\tqualification\tpanicked-not-qualified\t-\n",
            )?;
            ExitCode::FAILURE
        }
    };
    cleanup?;
    append(
        &report,
        if code == ExitCode::SUCCESS {
            "qualification\tM02\tpassed\t-\n"
        } else {
            "qualification\tM02\tfailed\t-\n"
        },
    )?;
    Ok(code)
}

fn append(path: &Path, text: &str) -> Result<(), String> {
    OpenOptions::new()
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|_| "cannot retain qualification evidence".into())
}

struct Database {
    directory: PathBuf,
    label: String,
    cleaned: bool,
}

impl Database {
    fn start(&self) -> Result<String, String> {
        docker(
            &["version", "--format", "{{.Server.Version}}"],
            Duration::from_secs(15),
        )?;
        let cidfile = self.directory.join("postgres.cid");
        let cidfile = cidfile.to_str().ok_or("owned path must be UTF-8")?;
        let id = docker(
            &[
                "create",
                "--label",
                &self.label,
                "--cidfile",
                cidfile,
                "--security-opt",
                "no-new-privileges:true",
                "--memory",
                "256m",
                "--publish",
                "127.0.0.1::5432",
                "--env",
                "POSTGRES_USER=edgeagent",
                "--env",
                "POSTGRES_PASSWORD=fixture-only",
                "--env",
                "POSTGRES_DB=edgeagent",
                IMAGE,
            ],
            Duration::from_secs(180),
        )?;
        let id = container_id(&id)?;
        docker(&["start", id], Duration::from_secs(30))?;
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if docker(
                &[
                    "exec",
                    id,
                    "pg_isready",
                    "-h",
                    "127.0.0.1",
                    "-U",
                    "edgeagent",
                    "-d",
                    "edgeagent",
                ],
                Duration::from_secs(5),
            )
            .is_ok()
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err("owned PostgreSQL did not become ready".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let mapping = docker(&["port", id, "5432/tcp"], Duration::from_secs(5))?;
        let port = loopback_port(&mapping)?;
        Ok(format!(
            "postgresql://edgeagent:fixture-only@127.0.0.1:{port}/edgeagent"
        ))
    }

    fn close(&mut self) -> Result<(), String> {
        // Exact per-run label also recovers a create that succeeded but whose
        // CLI receipt was lost. Never enumerate/delete unrelated containers.
        let ids = docker(
            &[
                "container",
                "ls",
                "--all",
                "--quiet",
                "--no-trunc",
                "--filter",
                &format!("label={}", self.label),
            ],
            Duration::from_secs(10),
        )?;
        for id in ids.lines() {
            docker(
                &["rm", "--force", "--volumes", container_id(id)?],
                Duration::from_secs(20),
            )?;
        }
        self.cleaned = true;
        Ok(())
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = self.close();
        }
    }
}

fn container_id(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(value)
    } else {
        Err("invalid owned container receipt".into())
    }
}

fn loopback_port(mapping: &str) -> Result<u16, String> {
    let port = mapping
        .trim()
        .strip_prefix("127.0.0.1:")
        .ok_or("owned database must bind loopback")?
        .parse::<u16>()
        .map_err(|_| "invalid owned database port")?;
    if port == 0 {
        return Err("invalid owned database port".into());
    }
    Ok(port)
}

fn docker(arguments: &[&str], timeout: Duration) -> Result<String, String> {
    let mut child = Command::new("docker")
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Docker is required for automatic M02 qualification")?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(
                    "owned Docker operation exceeded its deadline or could not be awaited".into(),
                );
            }
        }
    };
    // Every fixed command returns bounded metadata, never logs/data. Bound the
    // read too, and do not echo CLI diagnostics or database URLs into reports.
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or("Docker stdout absent")?
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read owned Docker receipt")?;
    if !status.success() || bytes.len() > 4096 {
        return Err("owned Docker operation failed".into());
    }
    String::from_utf8(bytes).map_err(|_| "invalid Docker receipt encoding".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_targets_and_network_bindings_fail_closed() -> Result<(), String> {
        assert_eq!(container_id(&"a".repeat(64))?, "a".repeat(64));
        for invalid in ["", "foreign-container", "abc", "../", "aaaaaaaa;docker rm"] {
            assert!(container_id(invalid).is_err());
        }
        assert_eq!(loopback_port("127.0.0.1:54321\n")?, 54321);
        for invalid in [
            "0.0.0.0:5432",
            "[::]:5432",
            "127.0.0.1:0",
            "127.0.0.1:1\n127.0.0.1:2",
        ] {
            assert!(loopback_port(invalid).is_err());
        }
        assert!(
            include_str!("../../../deploy/local/stack.toml")
                .contains(&format!("image = \"{IMAGE}\""))
        );
        Ok(())
    }
}
