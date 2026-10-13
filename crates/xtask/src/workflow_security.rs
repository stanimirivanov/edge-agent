//! Offline, fail-closed policy for the repository's reviewed GitHub workflows.

mod credentials;
mod environment;
mod gates;
mod inputs;
mod parse;
mod policy;
mod release;
mod steps;
mod supply_chain;

#[cfg(test)]
mod tests;

use std::{collections::BTreeSet, fs, io::Read, path::Path, process::ExitCode};

use yaml_rust2::Yaml;

const WORKFLOWS: &[&str] = &[
    "m08-durable-execution.yml",
    "release-images.yml",
    "repository-quality.yml",
];

/// Check parsed workflow behavior rather than comments or command substrings.
pub(crate) fn run() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    match check(&root) {
        Ok(()) => {
            println!("workflow security policy passed for all three reviewed workflows");
            ExitCode::SUCCESS
        }
        Err(problems) => {
            for problem in problems {
                eprintln!("{problem}; see docs/development/ci-security.md");
            }
            ExitCode::FAILURE
        }
    }
}

fn check(root: &Path) -> Result<(), Vec<String>> {
    let directory = root.join(".github/workflows");
    let entries = fs::read_dir(&directory).map_err(|_| {
        vec![".github/workflows: cannot enumerate the reviewed workflow inventory".to_owned()]
    })?;
    let mut problems = Vec::new();
    let mut actual = BTreeSet::new();
    for entry in entries {
        let Ok(entry) = entry else {
            problems
                .push(".github/workflows: cannot inspect a workflow directory entry".to_owned());
            continue;
        };
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            problems.push(".github/workflows: filenames must be valid UTF-8".to_owned());
            continue;
        };
        if !name.ends_with(".yml") && !name.ends_with(".yaml") {
            problems.push(format!(
                ".github/workflows/{name}: unreviewed workflow directory entry"
            ));
            continue;
        }
        actual.insert(name.to_owned());
        if !WORKFLOWS.contains(&name) {
            problems.push(format!(
                ".github/workflows/{name}: workflow needs an explicit security policy"
            ));
            continue;
        }
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            problems.push(format!(
                ".github/workflows/{name}: workflow must be a regular file, not a symlink"
            ));
            continue;
        }
        let mut source = String::new();
        let read = fs::File::open(&path).and_then(|file| {
            // Bound allocation while reading, not only after materializing an untrusted file.
            file.take(512 * 1024 + 1).read_to_string(&mut source)
        });
        if read.is_err() {
            problems.push(format!(
                ".github/workflows/{name}: cannot read UTF-8 workflow"
            ));
            continue;
        }
        problems.extend(check_source(name, &source));
    }
    for name in WORKFLOWS {
        if !actual.contains(*name) {
            problems.push(format!(
                ".github/workflows/{name}: required reviewed workflow is missing"
            ));
        }
    }
    problems.sort();
    problems.dedup();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

fn check_source(name: &str, source: &str) -> Vec<String> {
    let path = format!(".github/workflows/{name}");
    match parse::workflow(source) {
        Ok(root) => policy::check(name, &root)
            .into_iter()
            .map(|problem| format!("{path}: {problem}"))
            .collect(),
        // Parser diagnostics can contain arbitrary scalar values, including credentials.
        Err(reason) => vec![format!("{path}: {reason}")],
    }
}

fn field<'a>(node: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    node.as_hash()?.get(&Yaml::String(key.to_owned()))
}

fn text<'a>(node: &'a Yaml, key: &str) -> Option<&'a str> {
    field(node, key)?.as_str()
}

fn keys(node: &Yaml) -> Option<BTreeSet<&str>> {
    node.as_hash()?.keys().map(Yaml::as_str).collect()
}
