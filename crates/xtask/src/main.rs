//! Repository-owned development commands for EdgeAgent.

mod architecture;
mod repository;
mod verification;

use std::{env, process::ExitCode};

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    let command = arguments.next();

    let remaining: Vec<_> = arguments.collect();
    match command.as_deref() {
        Some("verify") => verification::run(&remaining),
        Some("sqlx-metadata") if remaining.is_empty() => verification::sqlx_metadata(),
        Some("checks") if remaining.is_empty() => verification::print_inventory(),
        Some("route") => verification::route(&remaining),
        Some("architecture") if remaining.is_empty() => run_architecture_check(),
        Some("repository") if remaining.is_empty() => run_repository_check(),
        None | Some("help" | "--help" | "-h") if remaining.is_empty() => {
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

fn run_repository_check() -> ExitCode {
    match repository::check() {
        Ok(report) if report.is_empty() => {
            println!(
                "repository policy passed for {} Markdown documents",
                report.document_count()
            );
            ExitCode::SUCCESS
        }
        Ok(report) => {
            eprintln!("repository policy failed:");
            for violation in report.violations() {
                eprintln!("{violation}");
            }
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("repository policy could not run: {error}");
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
           cargo xtask architecture    Verify workspace dependency policy\n\
           cargo xtask repository      Verify documentation and governance policy\n\
           cargo xtask verify [PROFILE] [--check ID] [--python EXECUTABLE]\n\
           cargo xtask checks          List canonical checks and evidence boundaries\n\
           cargo xtask route PATH...   Route Git-relative paths to guides and checks"
    );
}
