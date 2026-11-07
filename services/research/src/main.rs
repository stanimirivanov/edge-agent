//! EdgeAgent research service composition root.

#![forbid(unsafe_code)]

use edgeagent_contracts::{Component, ServiceDescriptor};
use edgeagent_service_runtime::run;
use edgeagent_telemetry::exporter::Telemetry;
use std::process::ExitCode;

const DESCRIPTOR: ServiceDescriptor = ServiceDescriptor::new(
    Component::Research,
    "Compute deterministic research and publish validated artifacts.",
    &["artifact-publication", "deterministic-research"],
);

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|a| a.starts_with("offline-")) {
        let telemetry = match args.first().map(String::as_str) {
            Some("offline-worker") => start_telemetry("offline-worker"),
            Some("offline-relay") => start_telemetry("offline-relay"),
            _ => Ok(None),
        };
        let Ok(telemetry) = telemetry else {
            eprintln!("research telemetry configuration rejected");
            return ExitCode::from(2);
        };
        let result = edgeagent_research::runtime::run(&args);
        if let Some(telemetry) = telemetry {
            telemetry.shutdown();
        }
        match result {
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

fn start_telemetry(
    role: &'static str,
) -> Result<Option<Telemetry>, Box<dyn std::error::Error + Send + Sync>> {
    Telemetry::start(
        "EDGEAGENT_RESEARCH_OTLP_ENDPOINT",
        "edgeagent.research.event-spine",
        role,
        Component::Research.as_str(),
    )
}
