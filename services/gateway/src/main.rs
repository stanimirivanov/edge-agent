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
    if std::env::args().skip(1).eq(["serve"]) {
        let result = edgeagent_gateway::runtime::Config::parse(|name| std::env::var(name).ok())
            .and_then(|config| {
                tokio::runtime::Runtime::new()
                    .map_err(|_| "gateway runtime unavailable")?
                    .block_on(edgeagent_gateway::runtime::serve(config))
            });
        if let Err(error) = result {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
        ExitCode::SUCCESS
    } else {
        run(DESCRIPTOR)
    }
}
