//! EdgeAgent audit projector composition root.

#![forbid(unsafe_code)]

use edgeagent_contracts::{Component, ServiceDescriptor};
use edgeagent_service_runtime::run;
use edgeagent_telemetry::exporter::Telemetry;
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
        let telemetry = if args.iter().map(String::as_str).eq(["offline-worker"]) {
            Telemetry::start(
                "EDGEAGENT_PROJECTOR_OTLP_ENDPOINT",
                "edgeagent.audit-projector.event-spine",
                "offline-worker",
                Component::AuditProjector.as_str(),
            )
        } else {
            Ok(None)
        };
        let Ok(telemetry) = telemetry else {
            eprintln!("projector telemetry configuration rejected");
            return ExitCode::from(2);
        };
        let result = edgeagent_audit_projector::runtime::run(&args);
        if let Some(telemetry) = telemetry {
            telemetry.shutdown();
        }
        match result {
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
