//! EdgeAgent research service composition root.

#![forbid(unsafe_code)]

use edgeagent_contracts::{Component, ServiceDescriptor};
use edgeagent_service_runtime::run;
use std::process::ExitCode;

const DESCRIPTOR: ServiceDescriptor = ServiceDescriptor::new(
    Component::Research,
    "Compute deterministic research and publish validated artifacts.",
    &["artifact-publication", "deterministic-research"],
);

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|a| a.starts_with("offline-")) {
        match edgeagent_research::runtime::run(&args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => {
                eprintln!("research worker failed closed");
                ExitCode::from(2)
            }
        }
    } else {
        run(DESCRIPTOR)
    }
}
