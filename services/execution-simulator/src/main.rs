//! EdgeAgent dry-run execution simulator composition root.

#![forbid(unsafe_code)]

use edgeagent_contracts::{Component, ServiceDescriptor};
use edgeagent_service_runtime::run;
use std::process::ExitCode;

const DESCRIPTOR: ServiceDescriptor = ServiceDescriptor::new(
    Component::ExecutionSimulator,
    "Simulate dry-run order lifecycles and portfolio ledger transitions.",
    &["dry-run-order-lifecycle", "simulated-ledger"],
);

fn main() -> ExitCode {
    run(DESCRIPTOR)
}
