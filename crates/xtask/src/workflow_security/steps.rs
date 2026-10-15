//! Reviewed action capabilities and per-step execution boundaries.

use super::{field, keys, policy::allowed_keys, text};
use yaml_rust2::Yaml;

const ACTIONS: &[(&str, &str)] = &[
    (
        "actions/checkout",
        "3d3c42e5aac5ba805825da76410c181273ba90b1",
    ),
    (
        "actions/setup-python",
        "5fda3b95a4ea91299a34e894583c3862153e4b97",
    ),
    (
        "EmbarkStudios/cargo-deny-action",
        "3c6349835b2b7b196a839186cb8b78e02f7b5f25",
    ),
    (
        "anchore/sbom-action/download-syft",
        "3ad7283483fc7af8ff2b4ea19663c2d5ca935e26",
    ),
    (
        "actions/upload-artifact",
        "043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
    ),
    (
        "docker/login-action",
        "dbcb813823bdd20940b903addbd779551569679f",
    ),
    (
        "docker/setup-buildx-action",
        "f87e5991a6d7451dcb8d9637bfbc97413f497069",
    ),
    (
        "docker/build-push-action",
        "c3c9e263c25d99ce0380d002d59b67737d91b0dc",
    ),
    ("actions/attest", "1e69f48acb82d1966a394da916b4c1698aa569d6"),
    (
        "sigstore/cosign-installer",
        "6f9f17788090df1f26f669e9d70d6ae9567deba6",
    ),
];

pub(super) fn check(
    name: &str,
    job_id: &str,
    step: &Yaml,
    index: usize,
    checkouts: &mut usize,
    problems: &mut Vec<String>,
) {
    let location = format!("jobs.{job_id}.steps[{index}]");
    allowed_keys(
        step,
        &["name", "id", "uses", "with", "run", "env", "if"],
        &location,
        problems,
    );
    let run = text(step, "run");
    let uses = text(step, "uses");
    if run.is_some() == uses.is_some() {
        problems.push(format!(
            "{location}: must specify exactly one literal run or reviewed uses action"
        ));
    }
    if field(step, "if").is_some() && !super::gates::diagnostic_or_cleanup(name, job_id, step) {
        problems.push(format!(
            "{location}.if: validation and security steps must not be conditional"
        ));
    }
    let action = uses.and_then(|uses| uses.split_once('@'));
    if let Some((repository, sha)) = action {
        if !ACTIONS.contains(&(repository, sha)) || !action_allowed(name, job_id, repository) {
            problems.push(format!(
                "{location}.uses must use a reviewed action's exact full commit SHA and job scope"
            ));
        }
        if repository == "actions/checkout" {
            *checkouts += 1;
            let config = field(step, "with");
            let safe = config.is_some_and(|config| {
                field(config, "persist-credentials").and_then(Yaml::as_bool) == Some(false)
                    && keys(config).is_some_and(|keys| {
                        keys.is_subset(
                            &["persist-credentials", "fetch-depth"].into_iter().collect(),
                        )
                    })
            });
            if !safe {
                problems.push(format!("{location}.with: checkout must disable persisted credentials and must not override token, repository, ref, SSH, or unsafe PR protection"));
            }
            if name == "repository-quality.yml"
                && job_id == "security"
                && config
                    .and_then(|config| field(config, "fetch-depth"))
                    .and_then(Yaml::as_i64)
                    != Some(0)
            {
                problems.push(format!("{location}.with.fetch-depth must be 0 for complete reachable-history secret scanning"));
            }
        }
    } else if uses.is_some() {
        problems.push(format!("{location}.uses: local, container, reusable, and unpinned actions require explicit review"));
    }
    super::inputs::check(
        name,
        job_id,
        step,
        action.map(|(repository, _)| repository),
        &location,
        problems,
    );
    if let Some(run) = run
        && !super::gates::safe_interpolation(run)
    {
        problems.push(format!("{location}.run: unsafe or unreviewed expression interpolation; pass untrusted values through quoted environment variables"));
    }
    super::environment::step(name, job_id, step, &location, problems);
    super::credentials::scan_step(
        name,
        job_id,
        step,
        action.map(|(repository, _)| repository),
        &location,
        problems,
    );
}

fn action_allowed(name: &str, job: &str, action: &str) -> bool {
    if action == "actions/checkout" {
        return true;
    }
    match (name, job) {
        ("repository-quality.yml", "supply-chain") => matches!(
            action,
            "actions/setup-python"
                | "EmbarkStudios/cargo-deny-action"
                | "anchore/sbom-action/download-syft"
                | "actions/upload-artifact"
        ),
        (
            "repository-quality.yml",
            "security" | "verify" | "container-smoke" | "local-platform-smoke",
        )
        | ("release-images.yml", "plan") => action == "actions/setup-python",
        ("m08-durable-execution.yml", "proof") | ("dependency-audit.yml", "audit") => {
            action == "EmbarkStudios/cargo-deny-action"
        }
        ("release-images.yml", "publish") => {
            action != "EmbarkStudios/cargo-deny-action" && action != "actions/upload-artifact"
        }
        _ => false,
    }
}
