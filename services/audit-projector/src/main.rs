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
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args
        .first()
        .is_some_and(|a| a.starts_with("offline-") || a == "serve")
    {
        match edgeagent_audit_projector::runtime::run(&args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => {
                eprintln!("projector failed closed");
                ExitCode::from(2)
            }
        }
    } else {
        run(DESCRIPTOR)
    }
}
