//! Explicit generation touches only the finite current evidence artifacts, not old schema versions.
use std::{path::Path, process::ExitCode};
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let write = match args.as_slice() {
        [] => false,
        [a] if a == "--write" => true,
        _ => return Err("usage: evidence_schemas [--write]".into()),
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("workspace root")?
        .join("contracts");
    if write {
        for (relative, bytes) in edgeagent_event_spine_conformance::evidence::artifacts()? {
            let path = root.join(relative);
            let parent = path.parent().ok_or("artifact parent")?;
            std::fs::create_dir_all(parent)?;
            if !parent.canonicalize()?.starts_with(root.canonicalize()?) {
                return Err("evidence output escapes contracts".into());
            }
            match std::fs::symlink_metadata(&path) {
                Ok(m) if !m.file_type().is_file() => {
                    return Err("evidence output is not regular".into());
                }
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
            std::fs::write(path, bytes)?;
        }
    }
    edgeagent_event_spine_conformance::evidence::verify(&root)?;
    println!("evidence schemas and golden artifacts are current; no retained versions changed");
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("evidence artifact tooling: {e}");
            ExitCode::FAILURE
        }
    }
}
