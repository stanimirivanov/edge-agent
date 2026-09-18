//! Dependency-free bootstrap behavior shared by EdgeAgent service binaries.

#![forbid(unsafe_code)]

use edgeagent_contracts::{COMPONENT_CONTRACT_VERSION, ServiceDescriptor};
use std::env;
use std::ffi::OsString;
use std::process::ExitCode;

const USAGE: &str = "Usage: <service> [describe|self-check|version|help]";

/// Run the common bootstrap command surface for a service.
///
/// This first foundation deliberately does not start network listeners. Later
/// milestones will extend each composition root with owned runtime behavior.
#[must_use]
pub fn run(descriptor: ServiceDescriptor) -> ExitCode {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    let outcome = evaluate(descriptor, &arguments);
    if outcome.success {
        println!("{}", outcome.message);
        ExitCode::SUCCESS
    } else {
        eprintln!("{}", outcome.message);
        ExitCode::from(2)
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Outcome {
    success: bool,
    message: String,
}

fn evaluate(descriptor: ServiceDescriptor, arguments: &[OsString]) -> Outcome {
    if let Err(error) = descriptor.validate() {
        return Outcome {
            success: false,
            message: format!("invalid service descriptor: {error}"),
        };
    }

    if arguments.len() > 1 {
        return Outcome {
            success: false,
            message: format!("unexpected arguments\n{USAGE}"),
        };
    }

    let command = arguments.first().and_then(|argument| argument.to_str());
    match command {
        None | Some("help" | "--help" | "-h") => Outcome {
            success: true,
            message: USAGE.to_owned(),
        },
        Some("describe" | "--describe") => Outcome {
            success: true,
            message: render_descriptor(descriptor),
        },
        Some("self-check" | "--self-check") => Outcome {
            success: true,
            message: format!("status=ok\ncomponent={}", descriptor.component.as_str()),
        },
        Some("version" | "--version" | "-V") => Outcome {
            success: true,
            message: env!("CARGO_PKG_VERSION").to_owned(),
        },
        Some(command) => Outcome {
            success: false,
            message: format!("unsupported command: {command}\n{USAGE}"),
        },
    }
}

fn render_descriptor(descriptor: ServiceDescriptor) -> String {
    format!(
        "contract={COMPONENT_CONTRACT_VERSION}\ncomponent={}\nsummary={}\ncapabilities={}",
        descriptor.component.as_str(),
        descriptor.summary,
        descriptor.capabilities.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::{Outcome, evaluate};
    use edgeagent_contracts::{Component, ServiceDescriptor};
    use std::ffi::OsString;

    const DESCRIPTOR: ServiceDescriptor = ServiceDescriptor::new(
        Component::Gateway,
        "Accept requests and compose queries.",
        &["command-intake", "query-composition"],
    );

    #[test]
    fn describe_is_stable_and_machine_readable() {
        let outcome = evaluate(DESCRIPTOR, &[OsString::from("describe")]);

        assert_eq!(
            outcome,
            Outcome {
                success: true,
                message: concat!(
                    "contract=edgeagent.component.v1\n",
                    "component=edgeagent.gateway\n",
                    "summary=Accept requests and compose queries.\n",
                    "capabilities=command-intake,query-composition"
                )
                .to_owned(),
            }
        );
    }

    #[test]
    fn unknown_command_fails_with_usage() {
        let outcome = evaluate(DESCRIPTOR, &[OsString::from("serve")]);

        assert!(!outcome.success);
        assert!(outcome.message.contains("unsupported command: serve"));
        assert!(outcome.message.contains("Usage:"));
    }
}
