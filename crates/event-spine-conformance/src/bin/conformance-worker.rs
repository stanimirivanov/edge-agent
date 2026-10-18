//! Finite synthetic composition root. Not an application service or deployment.

#[path = "../worker/mod.rs"]
mod worker;

fn main() -> std::process::ExitCode {
    let telemetry = match worker::start_telemetry() {
        Ok(telemetry) => telemetry,
        Err(_) => {
            eprintln!("conformance telemetry configuration rejected");
            return std::process::ExitCode::FAILURE;
        }
    };
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    else {
        if let Some(telemetry) = telemetry {
            telemetry.shutdown();
        }
        eprintln!("conformance worker runtime unavailable");
        return std::process::ExitCode::FAILURE;
    };
    // Never print a driver/source chain: configuration includes fixture secrets.
    let result = runtime.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(30), worker::run()).await
    });
    // Tokio stdin uses a blocking read internally. A missing parent start byte
    // must not make runtime shutdown wait indefinitely for that read to finish.
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    // Exporter degradation cannot replace the operation's classified result.
    if let Some(telemetry) = telemetry {
        telemetry.shutdown();
    }
    match result {
        Ok(Ok(())) => std::process::ExitCode::SUCCESS,
        Ok(Err(_)) => {
            eprintln!("conformance worker failed; durable outcome may be unresolved");
            std::process::ExitCode::FAILURE
        }
        Err(_) => {
            eprintln!("conformance worker deadline exceeded; durable outcome unresolved");
            std::process::ExitCode::FAILURE
        }
    }
}
