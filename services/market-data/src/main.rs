//! EdgeAgent market-data service composition root.

#![forbid(unsafe_code)]

use edgeagent_contracts::{Component, ServiceDescriptor};
use edgeagent_service_runtime::run;
use std::process::ExitCode;

const DESCRIPTOR: ServiceDescriptor = ServiceDescriptor::new(
    Component::MarketData,
    "Resolve instruments and publish validated evidence snapshots.",
    &["evidence-snapshot", "instrument-resolution"],
);

fn main() -> ExitCode {
    run(DESCRIPTOR)
}
