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
    run(DESCRIPTOR)
}
