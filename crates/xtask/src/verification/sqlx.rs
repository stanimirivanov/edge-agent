//! Freshness proof for offline SQLx metadata against the isolated Compose schema.

use std::{
    env,
    ffi::OsStr,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Output, Stdio},
    thread,
};

const LOCAL_DATABASE_URL: &str =
    "postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent";
const SQLX_VERSION: &str = "sqlx-cli 0.9.0";
const COMPOSE_PSQL: &[&str] = &[
    "compose",
    "--env-file",
    "deploy/local/.env.example",
    "-f",
    "deploy/local/compose.yaml",
    "exec",
    "-T",
    "postgres",
    "psql",
    "-X",
    "-v",
    "ON_ERROR_STOP=1",
    "-U",
    "edgeagent",
    "-d",
    "edgeagent",
];

pub(super) fn run() -> ExitCode {
    match verify(super::workspace_root()) {
        Ok(()) => {
            println!("SQLx metadata matches the isolated local inbox and outbox schema");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("SQLx metadata verification failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn verify(root: &Path) -> Result<(), String> {
    // Guard every target before any migration or metadata generation. Pin the
    // child environment too, so ambient configuration cannot redirect writes.
    validate_environment(|name| env::var(name).ok())?;
    let migrations = inbox_migrations(&root.join("crates/inbox-postgres/migrations"))?
        .into_iter()
        .map(|path| {
            fs::read(&path)
                .map(|bytes| (path, bytes))
                .map_err(|error| format!("cannot read an inbox migration: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let version = command(root, "cargo", &["sqlx", "--version"])
        .output()
        .map_err(|error| format!("cannot inspect the pinned SQLx CLI: {error}"))?;
    validate_version(&version)?;

    run_command(
        root,
        "cargo",
        &[
            "sqlx",
            "migrate",
            "run",
            "--source",
            "crates/outbox-postgres/migrations",
            "--no-dotenv",
        ],
        "outbox schema assembly",
    )?;

    // The adapters both start their migration ledgers at version 1. Applying
    // inbox SQL directly avoids pretending they share a deployment ledger.
    for (path, bytes) in migrations {
        println!("Applying inbox migration {}", path.display());
        apply_inbox_migration(root, &bytes)?;
    }
    run_command(
        root,
        "cargo",
        &[
            "sqlx",
            "prepare",
            "--workspace",
            "--no-dotenv",
            "--",
            "--locked",
            "--all-targets",
        ],
        "offline metadata generation",
    )?;
    let status = command(root, "git", &["status", "--porcelain", "--", ".sqlx"])
        .output()
        .map_err(|error| format!("cannot inspect generated metadata changes: {error}"))?;
    report_output(&status);
    if !status.status.success() {
        return Err("Git could not inspect .sqlx metadata".to_owned());
    }
    if !status.stdout.is_empty() {
        return Err(".sqlx metadata changed; review and commit the generated query metadata, then rerun the proof".to_owned());
    }
    Ok(())
}

fn validate_environment(read: impl Fn(&str) -> Option<String>) -> Result<(), String> {
    if read("DATABASE_URL").as_deref() != Some(LOCAL_DATABASE_URL) {
        return Err("DATABASE_URL must identify the exact checked-in isolated Compose PostgreSQL profile; other databases are refused".to_owned());
    }
    if read("SQLX_OFFLINE").as_deref() != Some("false") {
        return Err("SQLX_OFFLINE must equal false for metadata generation".to_owned());
    }
    if read("EDGEAGENT_SQLX_METADATA_LOCAL").as_deref() != Some("1") {
        return Err(
            "EDGEAGENT_SQLX_METADATA_LOCAL=1 must explicitly authorize local schema assembly"
                .to_owned(),
        );
    }
    Ok(())
}

fn inbox_migrations(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("cannot enumerate inbox migrations: {error}"))?
    {
        let entry = entry.map_err(|error| format!("cannot read a migration entry: {error}"))?;
        if entry.path().extension() != Some(OsStr::new("sql")) {
            continue;
        }
        if !entry
            .file_type()
            .map_err(|error| format!("cannot inspect a migration entry: {error}"))?
            .is_file()
        {
            return Err(
                "inbox SQL migrations must be regular files, not directories or symbolic links"
                    .to_owned(),
            );
        }
        paths.push(entry.path());
    }
    paths.sort();
    if paths.is_empty() {
        return Err("inbox schema assembly requires at least one SQL migration".to_owned());
    }
    Ok(paths)
}

fn validate_version(output: &Output) -> Result<(), String> {
    if !output.status.success()
        || std::str::from_utf8(&output.stdout).map(str::trim) != Ok(SQLX_VERSION)
    {
        return Err("SQLx metadata requires the pinned sqlx-cli 0.9.0; install the version documented in docs/development/sqlx-metadata.md".to_owned());
    }
    Ok(())
}

fn command(root: &Path, program: &str, args: &[&str]) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(root)
        .args(args)
        .env("DATABASE_URL", LOCAL_DATABASE_URL)
        .env("SQLX_OFFLINE", "false");
    command
}

fn run_command(root: &Path, program: &str, args: &[&str], stage: &str) -> Result<(), String> {
    let output = command(root, program, args)
        .output()
        .map_err(|error| format!("cannot start {stage}: {error}"))?;
    report_output(&output);
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("{stage} returned {}", output.status))
    }
}

fn apply_inbox_migration(root: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut child = command(root, "docker", COMPOSE_PSQL)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start inbox schema assembly: {error}"))?;
    // Drain both diagnostic pipes while a scoped writer supplies SQL. Waiting
    // until write_all completes can deadlock when psql fills its output pipe.
    let (write_result, output) = thread::scope(|scope| {
        let writer = child.stdin.take().map(|mut stdin| {
            scope.spawn(move || {
                stdin
                    .write_all(bytes)
                    .map_err(|error| format!("cannot pipe an inbox SQL migration: {error}"))
            })
        });
        let output = child.wait_with_output();
        let write_result = writer
            .ok_or_else(|| "inbox schema assembly did not provide standard input".to_owned())
            .and_then(|writer| {
                writer
                    .join()
                    .map_err(|_| "inbox SQL writer terminated unexpectedly".to_owned())?
            });
        (write_result, output)
    });
    // Reap psql even after a broken pipe or writer failure; no migration child
    // survives an ordinary coordinator-side error.
    let output = output.map_err(|error| format!("cannot await inbox schema assembly: {error}"))?;
    report_output(&output);
    write_result?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("inbox schema assembly returned {}", output.status))
    }
}

fn report_output(output: &Output) {
    // Keep diagnostics on their original stream without publishing a database
    // URL even if an external tool echoes its environment in an error.
    print!(
        "{}",
        String::from_utf8_lossy(&output.stdout)
            .replace(LOCAL_DATABASE_URL, "[isolated PostgreSQL URL]")
    );
    eprint!(
        "{}",
        String::from_utf8_lossy(&output.stderr)
            .replace(LOCAL_DATABASE_URL, "[isolated PostgreSQL URL]")
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    fn environment(name: &str) -> Option<String> {
        match name {
            "DATABASE_URL" => Some(LOCAL_DATABASE_URL.to_owned()),
            "SQLX_OFFLINE" => Some("false".to_owned()),
            "EDGEAGENT_SQLX_METADATA_LOCAL" => Some("1".to_owned()),
            _ => None,
        }
    }

    #[test]
    fn only_explicit_isolated_online_environment_authorizes_schema_assembly() {
        assert!(validate_environment(environment).is_ok());
        for variable in [
            "DATABASE_URL",
            "SQLX_OFFLINE",
            "EDGEAGENT_SQLX_METADATA_LOCAL",
        ] {
            assert!(
                validate_environment(|name| {
                    if name == variable {
                        None
                    } else {
                        environment(name)
                    }
                })
                .is_err()
            );
            assert!(
                validate_environment(|name| {
                    if name == variable {
                        Some("invalid-private-sentinel".to_owned())
                    } else {
                        environment(name)
                    }
                })
                .is_err_and(|error| !error.contains("invalid-private-sentinel"))
            );
        }
    }

    #[test]
    fn checked_in_inbox_migrations_are_complete_and_lexically_ordered() -> Result<(), Box<dyn Error>>
    {
        let directory = super::super::workspace_root().join("crates/inbox-postgres/migrations");
        let migrations = inbox_migrations(&directory)?;
        let names = migrations
            .iter()
            .map(|path| path.file_name().ok_or("migration filename"))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            names,
            [
                "0001_message_inbox.sql",
                "0002_message_quarantine.sql",
                "0003_message_quarantine_replay.sql",
                "0004_message_attempt_range.sql",
            ]
            .map(OsStr::new)
        );
        Ok(())
    }
}
