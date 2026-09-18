//! EdgeAgent audit projector composition root.

#![forbid(unsafe_code)]

use edgeagent_contracts::{Component, ServiceDescriptor};
use edgeagent_service_runtime::run;
use std::process::ExitCode;

const DESCRIPTOR: ServiceDescriptor = ServiceDescriptor::new(
    Component::AuditProjector,
    "Build rebuildable audit and query projections from committed facts.",
    &["audit-projection", "query-projection"],
);

fn main() -> ExitCode {
    run(DESCRIPTOR)
}
