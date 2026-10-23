//! Mandatory gates retain literal commands, execution order, and failure semantics.

use super::text;
use yaml_rust2::Yaml;

pub(super) fn diagnostic_or_cleanup(name: &str, job: &str, step: &Yaml) -> bool {
    if name != "repository-quality.yml" || job != "local-platform-smoke" {
        return false;
    }
    let prefix = "docker compose --env-file deploy/local/.env.example -f deploy/local/compose.yaml";
    if text(step, "if") == Some("always()")
        && text(step, "uses")
            == Some("actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a")
    {
        return true;
    }
    (text(step, "if") == Some("failure()")
        && text(step, "run") == Some(format!("{prefix} logs --no-color --tail=200").as_str()))
        || (text(step, "if") == Some("always()")
            && text(step, "run") == Some(format!("{prefix} down --volumes").as_str()))
}

pub(super) fn safe_interpolation(run: &str) -> bool {
    let allowed = [
        "matrix.candidate",
        "matrix.service",
        "matrix.binary",
        "matrix.image",
        "github.repository",
        "env.REGISTRY",
        "env.PLATFORM",
        "needs.plan.outputs.version",
        "steps.build.outputs.digest",
        "steps.syft.outputs.cmd",
    ];
    let mut remainder = run;
    while let Some((_, expression)) = remainder.split_once("${{") {
        let Some((expression, tail)) = expression.split_once("}}") else {
            return false;
        };
        if !allowed.contains(&expression.trim()) {
            return false;
        }
        remainder = tail;
    }
    true
}

pub(super) fn required(name: &str, job: &str, steps: &[Yaml], problems: &mut Vec<String>) {
    if name == "repository-quality.yml" && job == "local-platform-smoke" {
        let uploads = steps
            .iter()
            .filter(|step| {
                text(step, "uses")
                    == Some("actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a")
            })
            .collect::<Vec<_>>();
        if uploads.len() != 1
            || uploads
                .first()
                .is_none_or(|step| text(step, "if") != Some("always()"))
        {
            problems.push("jobs.local-platform-smoke: retain exactly one unconditional-on-result M02 evidence upload".into());
        }
    }
    let runs: &[&str] = match (name, job) {
        ("repository-quality.yml", "architecture") => &[
            "cargo xtask workflow-security",
            "cargo xtask repository",
            "cargo xtask architecture",
        ],
        ("m08-durable-execution.yml" | "dependency-audit.yml", "architecture") => &[
            "cargo xtask workflow-security",
            "cargo xtask verify --check repository-policy --check architecture-graph",
        ],
        ("repository-quality.yml", "security") => &[
            "python scripts/install_gitleaks.py",
            "cargo xtask verify --check secret-scanner-contracts",
            "cargo xtask verify --check secret-scan",
        ],
        ("repository-quality.yml", "verify") => &["cargo xtask verify"],
        ("repository-quality.yml", "offline-source-isolation") => &[
            "cargo test --locked -p edgeagent-market-data-replay --test offline --no-run",
            "cargo xtask verify --check offline-source-network-denial",
        ],
        ("repository-quality.yml", "container-smoke") => &["cargo xtask verify image-smoke"],
        ("repository-quality.yml", "local-platform-smoke") => &[
            "cargo xtask verify sqlx-metadata",
            "cargo xtask verify local-platform",
            "cargo xtask m02-qualification",
        ],
        ("m08-durable-execution.yml", "proof") => &[
            "cargo xtask verify m08-${{ matrix.candidate }}-static",
            "cargo xtask verify m08-${{ matrix.candidate }}-test",
        ],
        _ => &[],
    };
    for command in runs {
        if steps
            .iter()
            .filter(|step| text(step, "run").is_some_and(|run| run.trim() == *command))
            .count()
            != 1
        {
            problems.push(format!("jobs.{job}: must execute exactly once the protected command `{command}` without bypass suffixes"));
        }
    }
    if job == "architecture" {
        let first_run = steps.iter().find_map(|step| text(step, "run"));
        if first_run != Some("cargo xtask workflow-security") {
            // Toolchain installation must run first, but policy must precede any repository-owned code.
            let policy_index = steps
                .iter()
                .position(|step| text(step, "run") == Some("cargo xtask workflow-security"));
            let first_owned = steps.iter().position(|step| {
                text(step, "run")
                    .is_some_and(|run| run.contains("cargo xtask") || run.contains("scripts/"))
            });
            if policy_index.is_none() || policy_index != first_owned {
                problems.push(format!("jobs.{job}: workflow-security must be the first repository-owned executable gate"));
            }
        }
    }
    if name == "repository-quality.yml" && job == "security" {
        let expected = [
            (
                "secret-scanner-install",
                "python scripts/install_gitleaks.py",
            ),
            (
                "secret-scanner-contracts",
                "cargo xtask verify --check secret-scanner-contracts",
            ),
            ("secret-scan", "cargo xtask verify --check secret-scan"),
        ];
        let mut previous = None;
        for (id, run) in expected {
            let index = steps
                .iter()
                .position(|step| text(step, "id") == Some(id) && text(step, "run") == Some(run));
            if index.is_none()
                || previous.is_some_and(|previous| index.is_some_and(|index| index <= previous))
            {
                problems.push("jobs.security: scanner install, behavioral proof, and repository scan must retain protected IDs, commands, and order".to_owned());
            }
            previous = index;
        }
    }
}
