//! Narrow read-only exception for scheduled dependency detection, not arbitrary automation.

use yaml_rust2::Yaml;

use super::{field, keys, text};

pub(super) fn triggers(node: &Yaml) -> bool {
    let Some(actual) = keys(node) else {
        return false;
    };
    let scheduled = ["schedule"].into_iter().collect();
    let manual = ["schedule", "workflow_dispatch"].into_iter().collect();
    if actual != scheduled && actual != manual {
        return false;
    }
    let schedule_valid = field(node, "schedule")
        .and_then(Yaml::as_vec)
        .is_some_and(|schedules| {
            schedules.len() == 1
                && schedules.first().is_some_and(|schedule| {
                    keys(schedule) == Some(["cron"].into_iter().collect())
                        && text(schedule, "cron") == Some("27 3 * * *")
                })
        });
    schedule_valid
        && field(node, "workflow_dispatch").is_none_or(|dispatch| {
            dispatch.is_null() || dispatch.as_hash().is_some_and(|map| map.is_empty())
        })
}

pub(super) fn job(id: &str, job: &Yaml, steps: &[Yaml], problems: &mut Vec<String>) {
    let (timeout, count) = if id == "audit" { (15, 2) } else { (5, 4) };
    if field(job, "timeout-minutes").and_then(Yaml::as_i64) != Some(timeout)
        || steps.len() != count
        || field(job, "outputs").is_some()
        || (id == "architecture" && field(job, "strategy").is_some())
    {
        problems.push(format!(
            "jobs.{id}: scheduled audits must retain the bounded, read-only bootstrap and audit step inventory"
        ));
    }
    let expected_runs: &[&str] = if id == "architecture" {
        &[
            "rustup toolchain install 1.98.1 --profile minimal",
            "cargo xtask workflow-security",
            "cargo xtask verify --check repository-policy --check architecture-graph",
        ]
    } else {
        &[]
    };
    let actual_runs: Vec<_> = steps.iter().filter_map(|step| text(step, "run")).collect();
    if actual_runs != expected_runs {
        problems.push(format!(
            "jobs.{id}: scheduled audits may execute only the reviewed security bootstrap, not builds, proofs, image jobs, or arbitrary commands"
        ));
    }
    // Preserve checkout-before-audit even when all reviewed steps still exist.
    if steps.first().and_then(|step| text(step, "uses"))
        != Some("actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1")
    {
        problems.push(format!(
            "jobs.{id}: the reviewed checkout must precede scheduled commands and audits"
        ));
    }
}

pub(super) fn matrix(job: &Yaml) -> bool {
    let Some(strategy) = field(job, "strategy") else {
        return false;
    };
    if keys(strategy) != Some(["fail-fast", "matrix"].into_iter().collect())
        || field(strategy, "fail-fast").and_then(Yaml::as_bool) != Some(false)
    {
        return false;
    }
    let Some(matrix) = field(strategy, "matrix") else {
        return false;
    };
    if keys(matrix) != Some(["include"].into_iter().collect()) {
        return false;
    }
    let Some(rows) = field(matrix, "include").and_then(Yaml::as_vec) else {
        return false;
    };
    let expected = [
        ("production", "Cargo.toml", "deny.toml"),
        (
            "event-spine-proof",
            "experiments/event-spine-proof/Cargo.toml",
            "experiments/deny.toml",
        ),
        (
            "durable-execution",
            "experiments/durable-execution/Cargo.toml",
            "experiments/deny.toml",
        ),
        (
            "dbos-proof",
            "experiments/dbos-proof/Cargo.toml",
            "experiments/deny.toml",
        ),
        (
            "temporal-proof",
            "experiments/temporal-proof/Cargo.toml",
            "experiments/deny.toml",
        ),
        (
            "flawless-proof",
            "experiments/flawless-proof/Cargo.toml",
            "experiments/deny.toml",
        ),
    ];
    rows.len() == expected.len()
        && rows
            .iter()
            .zip(expected)
            .all(|(row, (workspace, manifest, policy))| {
                keys(row) == Some(["workspace", "manifest", "policy"].into_iter().collect())
                    && text(row, "workspace") == Some(workspace)
                    && text(row, "manifest") == Some(manifest)
                    && text(row, "policy") == Some(policy)
            })
}
