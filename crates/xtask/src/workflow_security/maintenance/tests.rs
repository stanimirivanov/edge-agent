use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use super::check;

const DIRECTORIES: &[&str] = &[
    ".",
    "experiments/event-spine-proof",
    "experiments/durable-execution",
    "experiments/dbos-proof",
    "experiments/temporal-proof",
    "experiments/flawless-proof",
];
const DEPENDABOT: &str = include_str!("../../../../../.github/dependabot.yml");
const PROOFS: &str = include_str!("../../../../../.github/workflows/m08-durable-execution.yml");

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "edgeagent-maintenance-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        let fixture = Self(path);
        for directory in DIRECTORIES {
            fixture.write(&format!("{directory}/Cargo.toml"), "[workspace]\n")?;
            fixture.write(&format!("{directory}/Cargo.lock"), "version = 4\n")?;
        }
        fixture.write(".github/dependabot.yml", DEPENDABOT)?;
        fixture.write(".github/workflows/m08-durable-execution.yml", PROOFS)?;
        let mut audit =
            "jobs:\n  audit:\n    strategy:\n      matrix:\n        include:\n".to_owned();
        for directory in DIRECTORIES {
            let workspace = if *directory == "." {
                "production"
            } else {
                directory.strip_prefix("experiments/").unwrap_or(directory)
            };
            let manifest = if *directory == "." {
                "Cargo.toml".to_owned()
            } else {
                format!("{directory}/Cargo.toml")
            };
            let policy = if *directory == "." {
                "deny.toml"
            } else {
                "experiments/deny.toml"
            };
            audit.push_str(&format!(
                "          - workspace: {workspace}\n            manifest: {manifest}\n            policy: {policy}\n"
            ));
        }
        fixture.write(".github/workflows/dependency-audit.yml", &audit)?;
        Ok(fixture)
    }

    fn write(&self, relative: &str, source: &str) -> io::Result<()> {
        let path = self.0.join(relative);
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("missing fixture parent"))?;
        fs::create_dir_all(parent)?;
        fs::write(path, source)
    }

    fn replace(&self, relative: &str, before: &str, after: &str) -> io::Result<()> {
        let source = fs::read_to_string(self.0.join(relative))?;
        assert!(source.contains(before), "mutation must change a fixture");
        self.write(relative, &source.replacen(before, after, 1))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn independent_workspaces_have_exact_maintenance_and_audit_coverage() -> io::Result<()> {
    let fixture = Fixture::new()?;
    assert_eq!(check(&fixture.0), Vec::<String>::new());
    Ok(())
}

#[test]
fn checked_in_maintenance_covers_every_source_lockfile() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert_eq!(check(&root), Vec::<String>::new());
}

#[test]
fn missing_extra_duplicate_or_globbed_update_directories_are_rejected() -> io::Result<()> {
    for replacement in [
        "",
        "      - /experiments/dbos-proof\n      - /not-a-workspace\n",
        "      - /experiments/dbos-proof\n      - /experiments/dbos-proof\n",
        "      - /experiments/*\n",
        "      - /experiments/../dbos-proof\n",
    ] {
        let fixture = Fixture::new()?;
        fixture.replace(
            ".github/dependabot.yml",
            "      - /experiments/dbos-proof\n",
            replacement,
        )?;
        assert!(
            check(&fixture.0)
                .iter()
                .any(|problem| problem.contains("dependabot.yml"))
        );
    }
    Ok(())
}

#[test]
fn maintenance_cannot_drop_transitives_disable_updates_or_rewrite_manifests() -> io::Result<()> {
    for (before, after) in [
        ("dependency-type: all", "dependency-type: direct"),
        ("interval: daily", "interval: weekly"),
        ("open-pull-requests-limit: 5", "open-pull-requests-limit: 0"),
        ("lockfile-only", "increase"),
        ("group-by: dependency-name", "group-by: directory"),
        (
            "    directories:",
            "    target-branch: bypass\n    directories:",
        ),
        ("    directories:", "    ignore: []\n    directories:"),
        ("version: 2", "version: 2\nregistries: {}"),
    ] {
        let fixture = Fixture::new()?;
        fixture.replace(".github/dependabot.yml", before, after)?;
        assert!(
            check(&fixture.0)
                .iter()
                .any(|problem| problem.contains("dependabot.yml"))
        );
    }
    Ok(())
}

#[test]
fn new_or_removed_workspace_locks_cannot_escape_any_inventory() -> io::Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("experiments/new-proof/Cargo.toml", "[workspace]\n")?;
    fixture.write("experiments/new-proof/Cargo.lock", "version = 4\n")?;
    let problems = check(&fixture.0);
    for path in [
        "verification/inventory.rs",
        "dependabot.yml",
        "dependency-audit.yml",
        "m08-durable-execution.yml",
    ] {
        assert!(
            problems.iter().any(|problem| problem.contains(path)),
            "{problems:?}"
        );
    }

    let fixture = Fixture::new()?;
    fs::remove_file(fixture.0.join("experiments/dbos-proof/Cargo.lock"))?;
    assert_eq!(check(&fixture.0).len(), 4);
    Ok(())
}

#[test]
fn local_audit_registry_cannot_omit_duplicate_or_misroute_a_workspace() {
    let roots = DIRECTORIES
        .iter()
        .map(|directory| {
            if *directory == "." {
                "/".to_owned()
            } else {
                format!("/{directory}")
            }
        })
        .collect();
    let targets = crate::verification::inventory::dependency_policy_targets();
    let mut problems = Vec::new();
    super::local_audits(&roots, &targets, &mut problems);
    assert!(problems.is_empty());

    for broken in [
        targets.iter().copied().skip(1).collect::<Vec<_>>(),
        targets.iter().copied().chain([targets[0]]).collect(),
        targets
            .iter()
            .map(|(manifest, _)| (*manifest, "deny.toml"))
            .collect(),
    ] {
        let mut problems = Vec::new();
        super::local_audits(&roots, &broken, &mut problems);
        assert_eq!(problems.len(), 1);
    }
}

#[test]
fn missing_manifests_root_locks_and_malformed_yaml_fail_closed() -> io::Result<()> {
    let fixture = Fixture::new()?;
    fs::remove_file(fixture.0.join("Cargo.lock"))?;
    assert!(
        check(&fixture.0)
            .iter()
            .any(|problem| problem.contains("root workspace"))
    );

    let fixture = Fixture::new()?;
    fixture.write("orphan/Cargo.lock", "version = 4\n")?;
    assert!(
        check(&fixture.0)
            .iter()
            .any(|problem| problem.contains("adjacent Cargo.toml"))
    );

    for source in [
        "version: 2\nversion: 3\n",
        "version: &version 2\nupdates: *version\n",
        "version: !unsafe 2\n",
        "version: 2\n---\nupdates: []\n",
    ] {
        let fixture = Fixture::new()?;
        fixture.write(".github/dependabot.yml", source)?;
        assert!(!check(&fixture.0).is_empty());
    }
    Ok(())
}

#[test]
fn audit_and_proof_matrix_coverage_cannot_omit_duplicate_or_misroute_workspaces() -> io::Result<()>
{
    for (relative, before, after) in [
        (
            ".github/workflows/dependency-audit.yml",
            "          - workspace: dbos-proof\n            manifest: experiments/dbos-proof/Cargo.toml\n            policy: experiments/deny.toml\n",
            "",
        ),
        (
            ".github/workflows/dependency-audit.yml",
            "            manifest: Cargo.toml",
            "            manifest: ../Cargo.toml",
        ),
        (
            ".github/workflows/dependency-audit.yml",
            "            policy: experiments/deny.toml",
            "            policy: deny.toml",
        ),
        (
            ".github/workflows/dependency-audit.yml",
            "        include:",
            "        exclude: []\n        include:",
        ),
        (
            ".github/workflows/dependency-audit.yml",
            "          - workspace: production",
            "          - workspace: production\n            manifest: Cargo.toml\n            policy: deny.toml\n          - workspace: production",
        ),
        (
            ".github/workflows/m08-durable-execution.yml",
            "          - dbos-proof\n",
            "",
        ),
        (
            ".github/workflows/m08-durable-execution.yml",
            "          - dbos-proof\n",
            "          - dbos-proof\n          - dbos-proof\n",
        ),
    ] {
        let fixture = Fixture::new()?;
        fixture.replace(relative, before, after)?;
        assert!(
            check(&fixture.0)
                .iter()
                .any(|problem| problem.contains(relative))
        );
    }
    Ok(())
}

#[test]
fn generated_locks_and_workspace_members_do_not_create_resolution_roots() -> io::Result<()> {
    let fixture = Fixture::new()?;
    fixture.write("target/generated/Cargo.lock", "version = 4\n")?;
    fixture.write(".git/generated/Cargo.lock", "version = 4\n")?;
    fixture.write(
        "experiments/flawless-proof/workflows/m08/Cargo.toml",
        "[package]\nname = 'm08'\n",
    )?;
    assert_eq!(check(&fixture.0), Vec::<String>::new());
    Ok(())
}

#[test]
fn diagnostics_do_not_echo_untrusted_credentials() -> io::Result<()> {
    let fixture = Fixture::new()?;
    fixture.replace(
        ".github/dependabot.yml",
        "version: 2",
        "version: 2\nregistries:\n  private:\n    token: synthetic-secret-do-not-print",
    )?;
    let problems = check(&fixture.0);
    assert!(!problems.is_empty());
    assert!(
        problems
            .iter()
            .all(|problem| !problem.contains("synthetic-secret-do-not-print"))
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn source_symlinks_are_rejected_without_traversal() -> io::Result<()> {
    let fixture = Fixture::new()?;
    std::os::unix::fs::symlink(fixture.0.join("experiments"), fixture.0.join("escaped"))?;
    assert!(
        check(&fixture.0)
            .iter()
            .any(|problem| problem.contains("symlinks and reparse"))
    );
    Ok(())
}

#[test]
fn census_requires_a_real_root_directory() -> io::Result<()> {
    let fixture = Fixture::new()?;
    assert_eq!(
        super::lock_roots(&fixture.0).map(|roots| roots.len()),
        Ok(6)
    );
    assert!(
        super::lock_roots(&fixture.0.join("Cargo.toml"))
            .is_err_and(|reason| reason.contains("real directories"))
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn census_rejects_a_symlinked_initial_root() -> io::Result<()> {
    let fixture = Fixture::new()?;
    let link_owner = Fixture::new()?;
    let link = link_owner.0.join("source-root");
    std::os::unix::fs::symlink(&fixture.0, &link)?;
    assert!(
        super::lock_roots(&link).is_err_and(|reason| reason.contains("symlinks or reparse points"))
    );
    Ok(())
}
