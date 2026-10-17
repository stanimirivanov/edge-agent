//! Selection, prerequisite resolution, fail-closed discovery, and execution evidence.

use std::{
    collections::BTreeSet,
    env,
    ffi::OsString,
    io,
    process::{Command, ExitCode, Stdio},
    time::Instant,
};

use super::{Check, CommandSpec, Discovery, Program, Requirement, inventory, workspace_root};

#[derive(Debug)]
pub(super) struct Options {
    profile: String,
    ids: Vec<String>,
    python: OsString,
    sbom_output: OsString,
}

impl Options {
    pub(super) fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut options = Self {
            profile: "verify".to_owned(),
            ids: Vec::new(),
            python: env::var_os("PYTHON").unwrap_or_else(|| "python".into()),
            sbom_output: "artifacts/sbom".into(),
        };
        let mut profile_seen = false;
        let mut arguments = arguments.iter();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--python" | "--sbom-output" | "--check" => {
                    let value = arguments
                        .next()
                        .filter(|v| !v.is_empty() && !v.starts_with("--"))
                        .ok_or_else(|| format!("{argument} requires a value"))?;
                    match argument.as_str() {
                        "--python" => options.python = value.into(),
                        "--sbom-output" => options.sbom_output = value.into(),
                        _ => options.ids.push(value.clone()),
                    }
                }
                value if !value.starts_with('-') && !profile_seen => {
                    options.profile = value.to_owned();
                    profile_seen = true;
                }
                _ => return Err(format!("unexpected argument: {argument}")),
            }
        }
        if profile_seen && !options.ids.is_empty() {
            return Err("choose a profile OR --check IDs, not both".to_owned());
        }
        if !inventory::PROFILES.contains(&options.profile.as_str()) {
            return Err(format!(
                "unknown profile {}; choose {}",
                options.profile,
                inventory::PROFILES.join(", ")
            ));
        }
        Ok(options)
    }

    pub(super) fn select<'a>(&self, checks: &'a [Check]) -> Result<Vec<&'a Check>, String> {
        let mut ids = BTreeSet::new();
        for id in &self.ids {
            if !ids.insert(id) || !checks.iter().any(|c| c.id == id) {
                return Err(format!("unknown or duplicate check ID: {id}"));
            }
        }
        let selected: Vec<_> = checks
            .iter()
            .filter(|c| {
                if ids.is_empty() {
                    c.profiles.contains(&self.profile.as_str())
                } else {
                    self.ids.iter().any(|id| id == c.id)
                }
            })
            .collect();
        if selected.is_empty() {
            return Err("selection contains no checks".to_owned());
        }
        Ok(selected)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Status {
    Passed,
    Failed(String),
    NotRun(String),
}

impl Status {
    fn label(&self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed(_) => "failed",
            Self::NotRun(_) => "not-run",
        }
    }
}

#[derive(Debug)]
pub(super) struct Outcome {
    pub(super) id: &'static str,
    pub(super) status: Status,
    elapsed_ms: u128,
}

// Substitution exists only to test the command protocol, never to fabricate user evidence.
pub(super) trait Executor {
    fn prerequisite(&mut self, requirement: Requirement) -> Result<(), String>;
    fn execute(&mut self, command: &CommandSpec, capture: bool) -> io::Result<Execution>;
}

#[derive(Debug)]
pub(super) struct Execution {
    pub(super) success: bool,
    pub(super) stdout: Vec<u8>,
}

struct ProcessExecutor<'a> {
    options: &'a Options,
}

pub(super) fn subprocess(command: &CommandSpec, options: &Options) -> io::Result<Command> {
    let program: OsString = match command.program {
        Program::Cargo => "cargo".into(),
        Program::Python => options.python.clone(),
        // Invoke the running binary, not nested cargo run / recursive verification.
        Program::Xtask => env::current_exe()?.into_os_string(),
    };
    let mut child = Command::new(program);
    child
        .current_dir(workspace_root())
        .stdin(Stdio::null())
        .stderr(Stdio::inherit());
    // The credential-free compile path must not inherit an accidental online
    // SQLx setting. Only the explicitly authorized metadata action uses a DB.
    if !(command.program == Program::Xtask && command.args == ["sqlx-metadata"]) {
        child.env("SQLX_OFFLINE", "true");
    }
    for argument in &command.args {
        if *argument == "artifacts/sbom" {
            child.arg(&options.sbom_output);
        } else {
            child.arg(argument);
        }
    }
    Ok(child)
}

pub(super) fn resolve_executable_choice(
    default: &str,
    variable: &str,
    explicit: Option<OsString>,
) -> Result<OsString, String> {
    match explicit {
        None => Ok(default.into()),
        Some(path)
            if std::path::Path::new(&path).is_absolute()
                && std::path::Path::new(&path).is_file() =>
        {
            Ok(path)
        }
        Some(_) => Err(format!(
            "{variable} must name an existing absolute executable path"
        )),
    }
}

impl Executor for ProcessExecutor<'_> {
    fn prerequisite(&mut self, requirement: Requirement) -> Result<(), String> {
        match requirement {
            Requirement::Environment(name) => {
                if env::var_os(name).is_some_and(|v| !v.is_empty()) {
                    Ok(())
                } else {
                    Err(format!("requires nonempty {name}"))
                }
            }
            Requirement::EnvironmentEquals(name, value) => {
                if env::var(name).is_ok_and(|v| v == value) {
                    Ok(())
                } else {
                    Err(format!("requires {name}={value}"))
                }
            }
            Requirement::Executable(name) => {
                // No shell lookup or command interpolation. The version probe has no side effects.
                let mut probe = if let Some(subcommand) = name.strip_prefix("cargo-") {
                    let mut command = Command::new("cargo");
                    command.arg(subcommand);
                    command
                } else {
                    Command::new(name)
                };
                match probe
                    .arg("--version")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                {
                    Ok(status) if status.success() => Ok(()),
                    _ => Err(format!("requires executable {name}")),
                }
            }
            Requirement::ExecutableOrEnvironment(default, variable) => {
                let executable =
                    resolve_executable_choice(default, variable, env::var_os(variable))?;
                match Command::new(executable)
                    .arg("--version")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                {
                    Ok(status) if status.success() => Ok(()),
                    _ => Err(format!("requires executable {default} or valid {variable}")),
                }
            }
            Requirement::CommandProbe(name, args) => {
                // The checksum installer has a repository-local default too.
                // Resolve the same artifact as the scanner wrapper so local
                // installation does not require mutating the caller's PATH.
                let installed = workspace_root()
                    .join("artifacts/security-tools/gitleaks-8.30.1")
                    .join(format!("gitleaks{}", env::consts::EXE_SUFFIX));
                let program = if name == "gitleaks" && installed.is_file() {
                    installed.into_os_string()
                } else {
                    name.into()
                };
                match Command::new(program)
                    .args(args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                {
                    Ok(status) if status.success() => Ok(()),
                    _ => Err(format!(
                        "requires successful probe: {name} {}",
                        args.join(" ")
                    )),
                }
            }
        }
    }

    fn execute(&mut self, command: &CommandSpec, capture: bool) -> io::Result<Execution> {
        let mut child = subprocess(command, self.options)?;
        if capture {
            let output = child.output()?;
            Ok(Execution {
                success: output.status.success(),
                stdout: output.stdout,
            })
        } else {
            // Metadata JSON is not diagnostic evidence. Keep errors visible
            // without flooding the human/agent loop with the entire graph.
            let stdout =
                if command.program == Program::Cargo && command.args.first() == Some(&"metadata") {
                    Stdio::null()
                } else {
                    Stdio::inherit()
                };
            Ok(Execution {
                success: child.stdout(stdout).status()?.success(),
                stdout: Vec::new(),
            })
        }
    }
}

/// Compare actual libtest identities, not Cargo's zero-test-success exit status.
pub(super) fn verify_discovery(discovery: &Discovery, stdout: &[u8]) -> Result<(), String> {
    let stdout =
        std::str::from_utf8(stdout).map_err(|_| "test discovery output is not UTF-8".to_owned())?;
    let actual: Vec<_> = stdout
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .collect();
    let actual_set: BTreeSet<_> = actual.iter().copied().collect();
    let expected: BTreeSet<_> = discovery.expected.iter().copied().collect();
    if actual.len() != actual_set.len() {
        return Err(
            "discovery returned duplicate test identities; use an unambiguous target".to_owned(),
        );
    }
    if actual_set != expected {
        let missing = expected
            .difference(&actual_set)
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        let unexpected = actual_set
            .difference(&expected)
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "test inventory drift: missing=[{missing}], unexpected=[{unexpected}]; update the owned proof contract, not a test filter"
        ));
    }
    if discovery.ignored != discovery.args.contains(&"--ignored") {
        return Err("discovery ignored-state contract does not match argv".to_owned());
    }
    Ok(())
}

fn execute_check(check: &Check, executor: &mut impl Executor) -> Status {
    for prerequisite in check.prerequisites {
        if let Err(reason) = executor.prerequisite(*prerequisite) {
            return Status::NotRun(reason);
        }
    }
    if let Some(discovery) = &check.discovery {
        let command = CommandSpec {
            program: Program::Cargo,
            args: discovery.args.clone(),
        };
        match executor.execute(&command, true) {
            Ok(output) if output.success => {
                if let Err(reason) = verify_discovery(discovery, &output.stdout) {
                    return Status::Failed(reason);
                }
            }
            Ok(_) => return Status::Failed("test discovery command failed".to_owned()),
            Err(error) => {
                return Status::NotRun(format!(
                    "test discovery command could not start ({:?})",
                    error.kind()
                ));
            }
        }
        // The census check's discovery is its entire proof. Do not list twice.
        if check.id == "ignored-inventory" {
            return Status::Passed;
        }
    }
    match executor.execute(&check.command, false) {
        Ok(output) if output.success => Status::Passed,
        Ok(_) => Status::Failed("command returned a nonzero status; see output above".to_owned()),
        Err(error) => Status::NotRun(format!(
            "command could not start ({:?}); check executable/tool installation",
            error.kind()
        )),
    }
}

pub(super) fn execute(selected: &[&Check], executor: &mut impl Executor) -> Vec<Outcome> {
    let mut stopped = false;
    selected
        .iter()
        .map(|check| {
            let start = Instant::now();
            let status = if stopped {
                Status::NotRun("blocked by preceding failed or unavailable check".to_owned())
            } else {
                println!(
                    "\n>>> {} [{}]: {}",
                    check.id,
                    check.tier,
                    check.command.display()
                );
                execute_check(check, executor)
            };
            stopped |= status != Status::Passed;
            Outcome {
                id: check.id,
                status,
                elapsed_ms: start.elapsed().as_millis(),
            }
        })
        .collect()
}

pub(super) fn run(options: &Options) -> ExitCode {
    let checks = inventory::checks();
    let selected = match inventory::validate(&checks)
        .and_then(|()| super::routes::validate())
        .and_then(|()| options.select(&checks))
    {
        Ok(selected) => selected,
        Err(error) => {
            eprintln!("verification selection failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    if !options.ids.is_empty() || options.profile != "verify" {
        println!("Focused verification only: this is not the complete default acceptance gate.");
    }
    if options.ids.is_empty() {
        println!("Selected profile: {}", options.profile);
    } else {
        println!("Selected check IDs: {}", options.ids.join(","));
    }
    let outcomes = execute(&selected, &mut ProcessExecutor { options });
    println!("\nExecution evidence (only selected checks; unselected T3 checks are not claimed):");
    for outcome in &outcomes {
        println!(
            "{}\t{}\t{}ms",
            outcome.id,
            outcome.status.label(),
            outcome.elapsed_ms
        );
        if let Status::Failed(reason) | Status::NotRun(reason) = &outcome.status {
            println!("  {reason}");
        }
    }
    if outcomes.iter().all(|o| o.status == Status::Passed) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
