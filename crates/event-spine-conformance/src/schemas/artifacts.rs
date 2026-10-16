use std::{collections::BTreeSet, fs, path::Path};

use super::SchemaKind;

/// Require the complete generated artifact inventory and byte-for-byte freshness.
///
/// This tooling API reads only the caller-selected local directory. It neither
/// resolves schema URIs nor updates files, including on failure.
///
/// # Errors
/// Returns an actionable tooling diagnostic for missing, extra, nonregular, or
/// stale files, or an I/O failure. Diagnostics never include schema contents.
pub fn verify_artifacts(directory: &Path) -> Result<(), String> {
    if fs::symlink_metadata(directory)
        .map_err(|_| "schema directory missing")?
        .file_type()
        .is_symlink()
    {
        return Err("schema directory must not be a symlink".into());
    }
    let mut actual = BTreeSet::new();
    for entry in fs::read_dir(directory).map_err(|_| "cannot list schema directory")? {
        let entry = entry.map_err(|_| "cannot read schema directory entry")?;
        if !entry
            .file_type()
            .map_err(|_| "cannot inspect schema entry")?
            .is_file()
        {
            return Err("schema inventory contains a nonregular file".into());
        }
        let filename = entry
            .file_name()
            .into_string()
            .map_err(|_| "invalid schema filename")?;
        actual.insert(filename);
    }
    let expected: BTreeSet<_> = SchemaKind::ALL
        .iter()
        .map(|kind| kind.filename().to_owned())
        .collect();
    if actual != expected {
        return Err(
            "schema inventory differs; expected exactly apply-probe.json and probe-applied.json"
                .into(),
        );
    }
    for kind in SchemaKind::ALL {
        let actual =
            fs::read(directory.join(kind.filename())).map_err(|_| "cannot read schema artifact")?;
        if actual
            != kind
                .artifact()
                .map_err(|_| "cannot generate local schema")?
                .as_bytes()
        {
            return Err(format!(
                "stale schema {}; run cargo xtask message-schemas --write and review compatibility",
                kind.filename()
            ));
        }
    }
    Ok(())
}
