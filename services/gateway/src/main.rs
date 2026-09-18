//! EdgeAgent request gateway composition root.

#![forbid(unsafe_code)]

use edgeagent_contracts::{Component, ServiceDescriptor};
use edgeagent_service_runtime::run;
use std::process::ExitCode;

const DESCRIPTOR: ServiceDescriptor = ServiceDescriptor::new(
    Component::Gateway,
    "Authenticate requests, accept commands, and compose queries.",
    &["command-intake", "query-composition"],
);

fn main() -> ExitCode {
    run(DESCRIPTOR)
}
