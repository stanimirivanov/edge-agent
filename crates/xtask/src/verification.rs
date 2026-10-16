//! Executable verification policy. Commands never come from documents or a shell.

pub(super) mod inventory;
mod proofs;
mod routes;
mod runner;
mod sqlx;

#[cfg(test)]
mod tests;

use std::{path::Path, process::ExitCode};

pub(crate) const DEFAULT_COMMAND: &str = "cargo xtask verify";
const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

pub(super) fn workspace_root() -> &'static Path {
    Path::new(WORKSPACE_ROOT)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Program {
    Cargo,
    Python,
    Xtask,
}

impl Program {
    fn label(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Python => "python",
            Self::Xtask => "cargo xtask",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct CommandSpec {
    program: Program,
    args: Vec<&'static str>,
}

impl CommandSpec {
    fn display(&self) -> String {
        format!("{} {}", self.program.label(), self.args.join(" "))
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Requirement {
    Environment(&'static str),
    EnvironmentEquals(&'static str, &'static str),
    Executable(&'static str),
    ExecutableOrEnvironment(&'static str, &'static str),
    CommandProbe(&'static str, &'static [&'static str]),
}

impl Requirement {
    fn display(self) -> String {
        match self {
            Self::Environment(name) => format!("nonempty {name}"),
            Self::EnvironmentEquals(name, value) => format!("{name}={value}"),
            Self::Executable(name) => format!("executable {name}"),
            Self::ExecutableOrEnvironment(name, variable) => {
                format!("executable {name} or explicit {variable}")
            }
            Self::CommandProbe(name, args) => {
                format!("successful probe: {name} {}", args.join(" "))
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Discovery {
    args: Vec<&'static str>,
    expected: Vec<&'static str>,
    ignored: bool,
}

#[derive(Clone, Debug)]
pub(super) struct Check {
    id: &'static str,
    tier: &'static str,
    profiles: &'static [&'static str],
    command: CommandSpec,
    prerequisites: &'static [Requirement],
    discovery: Option<Discovery>,
    evidence: &'static str,
}

pub(super) fn run(arguments: &[String]) -> ExitCode {
    match runner::Options::parse(arguments) {
        Ok(options) => runner::run(&options),
        Err(error) => {
            eprintln!("verification selection failed: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(super) fn sqlx_metadata() -> ExitCode {
    sqlx::run()
}

pub(super) fn print_inventory() -> ExitCode {
    match inventory::validate(&inventory::checks()).and_then(|()| routes::validate()) {
        Ok(()) => {
            for check in inventory::checks() {
                println!(
                    "{} [{}] profiles={}",
                    check.id,
                    check.tier,
                    check.profiles.join(",")
                );
                println!("  argv: {}", check.command.display());
                println!(
                    "  prerequisites: {}",
                    check
                        .prerequisites
                        .iter()
                        .map(|r| r.display())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                if let Some(discovery) = check.discovery {
                    println!(
                        "  discovery: cargo {}; expected={}",
                        discovery.args.join(" "),
                        discovery.expected.join(",")
                    );
                }
                println!("  evidence: {}", check.evidence);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("verification inventory failed: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(super) fn route(paths: &[String]) -> ExitCode {
    match routes::print(paths) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("verification routing failed: {error}");
            ExitCode::FAILURE
        }
    }
}
