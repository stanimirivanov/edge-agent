//! Explicitly regenerate only current artifacts; retained baselines are read-only.

use std::{env, fs, path::Path, process::ExitCode};

use edgeagent_event_spine_conformance::schemas::{SchemaKind, verify_artifacts};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = env::args().skip(1).collect();
    let write = match arguments.as_slice() {
        [] => false,
        [argument] if argument == "--write" => true,
        _ => return Err("usage: cargo xtask message-schemas [--write]".into()),
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = root.join("schemas/v1");
    if write {
        fs::create_dir_all(&directory)?;
        if !directory.canonicalize()?.starts_with(root.canonicalize()?) {
            return Err("generated schema directory escapes the conformance package".into());
        }
        // Never follow output-file symlinks or repair extra artifacts by deleting
        // them. Explicit generation touches only this finite current inventory.
        for kind in SchemaKind::ALL {
            let path = directory.join(kind.filename());
            match fs::symlink_metadata(&path) {
                Ok(metadata) if !metadata.file_type().is_file() => {
                    return Err("schema output is not a regular file".into());
                }
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(error.into());
                }
                _ => (),
            }
            fs::write(path, kind.artifact()?)?;
        }
    }
    verify_artifacts(&directory)?;
    println!("local acceptance schemas are current; retained baselines were not modified");
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("acceptance schema tooling failed: {error}");
            ExitCode::FAILURE
        }
    }
}
