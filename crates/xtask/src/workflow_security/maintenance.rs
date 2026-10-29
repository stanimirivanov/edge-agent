//! Independent lockfiles must remain covered by maintenance and CI audits.

use std::{collections::BTreeSet, fs, io::Read, path::Path};

use yaml_rust2::Yaml;

use super::{field, keys, text};

#[cfg(test)]
mod tests;

// Match the repository byte sensor; generated caches are not source workspaces.
const EXCLUDED: &[&str] = &[
    ".git",
    ".idea",
    ".local",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".venv",
    ".vscode",
    "__pycache__",
    "artifacts",
    "build",
    "dist",
    "node_modules",
    "target",
];

pub(super) fn check(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let roots = match lock_roots(root) {
        Ok(roots) => roots,
        Err(reason) => return vec![format!("dependency maintenance census: {reason}")],
    };
    local_audits(
        &roots,
        &crate::verification::inventory::dependency_policy_targets(),
        &mut problems,
    );
    if let Some(config) = load(root, ".github/dependabot.yml", &mut problems) {
        dependabot(&config, &roots, &mut problems);
    }
    if let Some(config) = load(
        root,
        ".github/workflows/dependency-audit.yml",
        &mut problems,
    ) {
        audit_matrix(&config, &roots, &mut problems);
    }
    if let Some(config) = load(
        root,
        ".github/workflows/m08-durable-execution.yml",
        &mut problems,
    ) {
        proof_matrix(&config, &roots, &mut problems);
    }
    problems
}

fn load(root: &Path, relative: &str, problems: &mut Vec<String>) -> Option<Yaml> {
    let path = root.join(relative);
    let result = (|| {
        let metadata = fs::symlink_metadata(&path).map_err(|_| "cannot inspect YAML")?;
        if !metadata.is_file() || is_link(&metadata) {
            return Err("YAML must be a regular source file");
        }
        let mut source = String::new();
        fs::File::open(path)
            .and_then(|file| file.take(512 * 1024 + 1).read_to_string(&mut source))
            .map_err(|_| "cannot read bounded UTF-8 YAML")?;
        super::parse::workflow(&source)
    })();
    match result {
        Ok(config) => Some(config),
        Err(reason) => {
            problems.push(format!("{relative}: {reason}"));
            None
        }
    }
}

fn lock_roots(root: &Path) -> Result<BTreeSet<String>, &'static str> {
    let mut pending = vec![(root.to_path_buf(), 0)];
    let mut roots = BTreeSet::new();
    let mut entries = 0;
    while let Some((directory, depth)) = pending.pop() {
        if depth > 128 {
            return Err("source traversal exceeds 128 levels");
        }
        let metadata =
            fs::symlink_metadata(&directory).map_err(|_| "cannot inspect source directory")?;
        if !metadata.is_dir() || is_link(&metadata) {
            return Err(
                "source directories must be real directories, not symlinks or reparse points",
            );
        }
        for entry in fs::read_dir(directory).map_err(|_| "cannot enumerate source directory")? {
            let entry = entry.map_err(|_| "cannot inspect source directory entry")?;
            entries += 1;
            if entries > 100_000 {
                return Err("source traversal exceeds 100000 entries");
            }
            let name = entry.file_name();
            let name = name.to_str().ok_or("source paths must be UTF-8")?;
            if EXCLUDED.contains(&name) {
                continue;
            }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|_| "cannot inspect source path")?;
            if is_link(&metadata) {
                return Err("source symlinks and reparse points are forbidden");
            }
            if metadata.is_dir() {
                pending.push((path, depth + 1));
            } else if name == "Cargo.lock" {
                if !metadata.is_file() {
                    return Err("Cargo.lock must be a regular source file");
                }
                let parent = path.parent().ok_or("Cargo.lock has no parent directory")?;
                let manifest = fs::symlink_metadata(parent.join("Cargo.toml"))
                    .map_err(|_| "Cargo.lock requires an adjacent Cargo.toml")?;
                if !manifest.is_file() || is_link(&manifest) {
                    return Err("Cargo.lock requires a regular adjacent Cargo.toml");
                }
                let relative = parent
                    .strip_prefix(root)
                    .map_err(|_| "lockfile escaped the repository")?;
                let relative = relative.to_str().ok_or("source paths must be UTF-8")?;
                roots.insert(format!("/{}", relative.replace('\\', "/")));
            }
        }
    }
    if !roots.contains("/") {
        return Err("the root workspace Cargo.lock is required");
    }
    Ok(roots)
}

fn is_link(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn exact_keys(node: &Yaml, expected: &[&str]) -> bool {
    keys(node) == Some(expected.iter().copied().collect())
}

fn local_audits(roots: &BTreeSet<String>, targets: &[(&str, &str)], problems: &mut Vec<String>) {
    let expected: BTreeSet<_> = roots
        .iter()
        .map(|directory| {
            let relative = directory.trim_start_matches('/');
            if relative.is_empty() {
                ("Cargo.toml".to_owned(), "deny.toml")
            } else {
                (format!("{relative}/Cargo.toml"), "experiments/deny.toml")
            }
        })
        .collect();
    let actual: BTreeSet<_> = targets
        .iter()
        .map(|(manifest, policy)| ((*manifest).to_owned(), *policy))
        .collect();
    if actual != expected || actual.len() != targets.len() {
        problems.push("crates/xtask/src/verification/inventory.rs: dependency-audit must register every Cargo.lock manifest and its production/experiment policy exactly once".to_owned());
    }
}

fn strings(node: Option<&Yaml>) -> Option<BTreeSet<String>> {
    let values = node?.as_vec()?;
    let unique: BTreeSet<_> = values
        .iter()
        .map(|value| value.as_str().map(str::to_owned))
        .collect::<Option<_>>()?;
    (unique.len() == values.len()).then_some(unique)
}

fn dependabot(config: &Yaml, roots: &BTreeSet<String>, problems: &mut Vec<String>) {
    let updates = field(config, "updates").and_then(Yaml::as_vec);
    let update = updates.and_then(|updates| match updates.as_slice() {
        [update] => Some(update),
        _ => None,
    });
    let valid = exact_keys(config, &["version", "updates"])
        && field(config, "version").and_then(Yaml::as_i64) == Some(2)
        && update.is_some_and(|update| {
            exact_keys(
                update,
                &[
                    "package-ecosystem",
                    "directories",
                    "schedule",
                    "allow",
                    "versioning-strategy",
                    "open-pull-requests-limit",
                    "groups",
                ],
            ) && text(update, "package-ecosystem") == Some("cargo")
                && strings(field(update, "directories")).as_ref() == Some(roots)
                && field(update, "schedule").is_some_and(|schedule| {
                    exact_keys(schedule, &["interval"])
                        && text(schedule, "interval") == Some("daily")
                })
                && field(update, "allow")
                    .and_then(Yaml::as_vec)
                    .is_some_and(|allow| match allow.as_slice() {
                        [rule] => {
                            exact_keys(rule, &["dependency-type"])
                                && text(rule, "dependency-type") == Some("all")
                        }
                        _ => false,
                    })
                && text(update, "versioning-strategy") == Some("lockfile-only")
                && field(update, "open-pull-requests-limit").and_then(Yaml::as_i64) == Some(5)
                && field(update, "groups").is_some_and(|groups| {
                    exact_keys(groups, &["rust-workspaces"])
                        && field(groups, "rust-workspaces").is_some_and(|group| {
                            exact_keys(group, &["patterns", "group-by"])
                                && strings(field(group, "patterns"))
                                    == Some(["*".to_owned()].into_iter().collect())
                                && text(group, "group-by") == Some("dependency-name")
                        })
                })
        });
    if !valid {
        problems.push(".github/dependabot.yml: require exact Cargo.lock directory coverage, daily direct/transitive lockfile-only updates, dependency-name grouping, and five reviewable PRs without exclusions or credential overrides".to_owned());
    }
}

fn audit_matrix(config: &Yaml, roots: &BTreeSet<String>, problems: &mut Vec<String>) {
    let matrix = field(config, "jobs")
        .and_then(|jobs| field(jobs, "audit"))
        .and_then(|job| field(job, "strategy"))
        .and_then(|strategy| field(strategy, "matrix"));
    let entries = matrix
        .and_then(|matrix| field(matrix, "include"))
        .and_then(Yaml::as_vec);
    let expected: BTreeSet<_> = roots
        .iter()
        .map(|directory| {
            let relative = directory.trim_start_matches('/');
            let workspace = if relative.is_empty() {
                "production"
            } else {
                relative.strip_prefix("experiments/").unwrap_or(relative)
            };
            let manifest = if relative.is_empty() {
                "Cargo.toml".to_owned()
            } else {
                format!("{relative}/Cargo.toml")
            };
            let policy = if relative.is_empty() {
                "deny.toml"
            } else {
                "experiments/deny.toml"
            };
            (workspace.to_owned(), manifest, policy.to_owned())
        })
        .collect();
    let actual = entries.and_then(|entries| {
        entries
            .iter()
            .map(|entry| {
                if !exact_keys(entry, &["workspace", "manifest", "policy"]) {
                    return None;
                }
                Some((
                    text(entry, "workspace")?.to_owned(),
                    text(entry, "manifest")?.to_owned(),
                    text(entry, "policy")?.to_owned(),
                ))
            })
            .collect::<Option<BTreeSet<_>>>()
    });
    if !matrix.is_some_and(|matrix| exact_keys(matrix, &["include"]))
        || actual.as_ref() != Some(&expected)
        || entries.map(Vec::len) != actual.as_ref().map(BTreeSet::len)
    {
        problems.push(".github/workflows/dependency-audit.yml: jobs.audit.strategy.matrix.include must cover every Cargo.lock directory exactly once with its literal workspace, manifest, and production/experiment policy".to_owned());
    }
}

fn proof_matrix(config: &Yaml, roots: &BTreeSet<String>, problems: &mut Vec<String>) {
    let matrix = field(config, "jobs")
        .and_then(|jobs| field(jobs, "proof"))
        .and_then(|job| field(job, "strategy"))
        .and_then(|strategy| field(strategy, "matrix"));
    let expected = roots
        .iter()
        .filter_map(|directory| directory.strip_prefix("/experiments/").map(str::to_owned))
        .collect();
    if !matrix.is_some_and(|matrix| exact_keys(matrix, &["candidate"]))
        || matrix.and_then(|matrix| strings(field(matrix, "candidate"))) != Some(expected)
    {
        problems.push(".github/workflows/m08-durable-execution.yml: the proof candidate matrix must cover every experiment Cargo.lock directory exactly once".to_owned());
    }
}
