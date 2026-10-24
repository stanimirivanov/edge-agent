//! EdgeAgent market-data service composition root.

#![forbid(unsafe_code)]

use edgeagent_contracts::{Component, ServiceDescriptor};
use edgeagent_service_runtime::run;
use std::process::ExitCode;
mod offline;

const DESCRIPTOR: ServiceDescriptor = ServiceDescriptor::new(
    Component::MarketData,
    "Resolve instruments and publish validated evidence snapshots.",
    &["evidence-snapshot", "instrument-resolution"],
);

fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args
        .first()
        .and_then(|arg| arg.to_str())
        .is_some_and(|arg| arg.starts_with("offline-"))
    {
        match args
            .into_iter()
            .map(std::ffi::OsString::into_string)
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(args) => return offline::run(&args),
            Err(_) => {
                eprintln!("offline arguments must be UTF-8");
                return ExitCode::from(2);
            }
        }
    }
    run(DESCRIPTOR)
}
