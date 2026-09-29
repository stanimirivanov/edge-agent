//! Repository-owned development commands for EdgeAgent.

mod architecture;

use std::{env, process::ExitCode};

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    let command = arguments.next();

    match (command.as_deref(), arguments.next()) {
        (Some("architecture"), None) => run_architecture_check(),
        (None | Some("help" | "--help" | "-h"), None) => {
            print_usage();
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("invalid xtask command or arguments");
            print_usage();
            ExitCode::FAILURE
        }
    }
}

fn run_architecture_check() -> ExitCode {
    match architecture::check() {
        Ok(report) => {
            println!(
                "architecture policy passed for {} workspace packages and {} internal dependencies",
                report.package_count(),
                report.dependency_count()
            );
            for exception in report.temporary_exceptions() {
                println!("temporary exception: {exception}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("architecture policy failed:\n{error}");
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    eprintln!(
        "EdgeAgent repository tasks\n\n\
         Usage:\n\
           cargo xtask architecture    Verify workspace dependency policy"
    );
}
