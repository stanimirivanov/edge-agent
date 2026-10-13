//! Reviewed capabilities, privileged exceptions, and executable gate shapes.

use std::collections::BTreeSet;

use yaml_rust2::Yaml;

use super::{field, keys, text};

pub(super) fn check(name: &str, root: &Yaml) -> Vec<String> {
    let mut problems = Vec::new();
    allowed_keys(
        root,
        &["name", "on", "permissions", "concurrency", "jobs", "env"],
        "workflow",
        &mut problems,
    );
    if !permissions(field(root, "permissions"), false) {
        problems.push("workflow.permissions must be exactly contents: read".to_owned());
    }
    if !triggers(name, field(root, "on")) {
        problems.push("workflow.on must use the reviewed PR/main or tag-only trigger; privileged and manual triggers are forbidden".to_owned());
    }
    super::credentials::scan_except(root, "workflow", &["jobs"], &mut problems);
    super::environment::workflow(name, root, &mut problems);
    let expected_jobs: &[&str] = match name {
        "repository-quality.yml" => &[
            "architecture",
            "security",
            "verify",
            "container-smoke",
            "local-platform-smoke",
            "supply-chain",
        ],
        "m08-durable-execution.yml" => &["architecture", "proof"],
        "release-images.yml" => &["plan", "publish"],
        _ => return vec!["workflow needs an explicit security policy".to_owned()],
    };
    let Some(jobs) = field(root, "jobs") else {
        return vec!["workflow.jobs must contain the reviewed jobs".to_owned()];
    };
    if keys(jobs) != Some(expected_jobs.iter().copied().collect()) {
        problems.push("workflow.jobs must match the exact reviewed job inventory".to_owned());
    }
    for job_id in expected_jobs {
        if let Some(job) = field(jobs, job_id) {
            check_job(name, job_id, job, &mut problems);
        }
    }
    if name == "release-images.yml" {
        super::release::check(root, &mut problems);
    }
    super::supply_chain::check(name, root, &mut problems);
    problems
}

fn permissions(node: Option<&Yaml>, publish: bool) -> bool {
    let Some(node) = node else {
        return false;
    };
    let expected = if publish {
        &[
            "contents",
            "packages",
            "id-token",
            "attestations",
            "artifact-metadata",
        ][..]
    } else {
        &["contents"][..]
    };
    keys(node) == Some(expected.iter().copied().collect())
        && expected
            .iter()
            .all(|key| text(node, key) == Some(if *key == "contents" { "read" } else { "write" }))
}

fn triggers(name: &str, node: Option<&Yaml>) -> bool {
    let Some(node) = node else {
        return false;
    };
    let release = name == "release-images.yml";
    let expected = if release {
        &["push"][..]
    } else {
        &["push", "pull_request"][..]
    };
    if keys(node) != Some(expected.iter().copied().collect()) {
        return false;
    }
    let Some(push) = field(node, "push") else {
        return false;
    };
    let (filter, values) = if release {
        ("tags", &["v*.*.*"][..])
    } else {
        ("branches", &["main"][..])
    };
    if keys(push) != Some([filter].into_iter().collect())
        || !string_sequence(field(push, filter), values)
    {
        return false;
    }
    release
        || field(node, "pull_request").is_some_and(|value| {
            value.is_null() || value.as_hash().is_some_and(|map| map.is_empty())
        })
}

fn check_job(name: &str, job_id: &str, job: &Yaml, problems: &mut Vec<String>) {
    let location = format!("jobs.{job_id}");
    allowed_keys(
        job,
        &[
            "name",
            "runs-on",
            "needs",
            "timeout-minutes",
            "steps",
            "strategy",
            "outputs",
            "permissions",
            "env",
        ],
        &location,
        problems,
    );
    let publish = name == "release-images.yml" && job_id == "publish";
    if (publish || field(job, "permissions").is_some())
        && !permissions(field(job, "permissions"), publish)
    {
        problems.push(format!(
            "{location}.permissions must match the job's exact least-privilege policy"
        ));
    }
    if !field(job, "timeout-minutes")
        .and_then(Yaml::as_i64)
        .is_some_and(|value| (1..=60).contains(&value))
    {
        problems.push(format!(
            "{location}.timeout-minutes must be a literal integer between 1 and 60"
        ));
    }
    if !runner(name, job_id, job) {
        problems.push(format!(
            "{location}.runs-on must use the reviewed GitHub-hosted runner or finite OS matrix"
        ));
    }
    check_needs(name, job_id, job, problems);
    super::credentials::scan_except(job, &location, &["steps"], problems);
    super::environment::job(name, job_id, job, problems);
    let Some(steps) = field(job, "steps").and_then(Yaml::as_vec) else {
        problems.push(format!(
            "{location}.steps must be an executable step sequence, not a reusable workflow"
        ));
        return;
    };
    let mut ids = BTreeSet::new();
    let mut checkouts = 0;
    for (index, step) in steps.iter().enumerate() {
        if let Some(id) = text(step, "id")
            && !ids.insert(id)
        {
            problems.push(format!("{location}.steps: step IDs must be unique"));
        }
        super::steps::check(name, job_id, step, index, &mut checkouts, problems);
    }
    if checkouts != 1 {
        problems.push(format!(
            "{location}: must contain exactly one reviewed checkout"
        ));
    }
    super::gates::required(name, job_id, steps, problems);
}

fn runner(name: &str, job_id: &str, job: &Yaml) -> bool {
    if name == "repository-quality.yml" && job_id == "verify" {
        text(job, "runs-on") == Some("${{ matrix.os }}")
            && field(job, "strategy")
                .and_then(|value| field(value, "matrix"))
                .is_some_and(|matrix| {
                    keys(matrix) == Some(["os"].into_iter().collect())
                        && string_sequence(
                            field(matrix, "os"),
                            &["ubuntu-latest", "windows-latest"],
                        )
                })
    } else if name == "m08-durable-execution.yml" && job_id == "proof" {
        text(job, "runs-on") == Some("ubuntu-latest")
            && field(job, "strategy")
                .and_then(|value| field(value, "matrix"))
                .is_some_and(|matrix| {
                    keys(matrix) == Some(["candidate"].into_iter().collect())
                        && string_sequence(
                            field(matrix, "candidate"),
                            &[
                                "event-spine-proof",
                                "durable-execution",
                                "dbos-proof",
                                "temporal-proof",
                                "flawless-proof",
                            ],
                        )
                })
    } else {
        text(job, "runs-on") == Some("ubuntu-latest")
    }
}

fn check_needs(name: &str, id: &str, job: &Yaml, problems: &mut Vec<String>) {
    let expected: &[&str] = match (name, id) {
        ("repository-quality.yml", "architecture")
        | ("m08-durable-execution.yml", "architecture")
        | ("release-images.yml", "plan") => &[],
        ("repository-quality.yml", "security") | ("m08-durable-execution.yml", "proof") => {
            &["architecture"]
        }
        ("repository-quality.yml", _) => &["architecture", "security"],
        ("release-images.yml", "publish") => &["plan"],
        _ => &[],
    };
    let valid = if expected.is_empty() {
        field(job, "needs").is_none()
    } else if expected.len() == 1 {
        text(job, "needs") == expected.first().copied()
            || string_sequence(field(job, "needs"), expected)
    } else {
        string_sequence(field(job, "needs"), expected)
    };
    if !valid {
        problems.push(format!(
            "jobs.{id}.needs must preserve the reviewed architecture/security dependency order"
        ));
    }
}

pub(super) fn allowed_keys(
    node: &Yaml,
    allowed: &[&str],
    location: &str,
    problems: &mut Vec<String>,
) {
    if !keys(node).is_some_and(|keys| keys.is_subset(&allowed.iter().copied().collect())) {
        problems.push(format!(
            "{location}: unreviewed keys, skip/continue-on-error overrides, or malformed mapping"
        ));
    }
}

pub(super) fn string_sequence(node: Option<&Yaml>, expected: &[&str]) -> bool {
    node.and_then(Yaml::as_vec).is_some_and(|values| {
        values
            .iter()
            .map(Yaml::as_str)
            .collect::<Option<Vec<_>>>()
            .as_deref()
            == Some(expected)
    })
}
