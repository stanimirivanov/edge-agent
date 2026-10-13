//! Credential capability is absent except at narrowly scoped release token boundaries.

use super::text;
use yaml_rust2::Yaml;

pub(super) fn scan_step(
    name: &str,
    job: &str,
    step: &Yaml,
    action: Option<&str>,
    location: &str,
    problems: &mut Vec<String>,
) {
    let Some(map) = step.as_hash() else {
        return;
    };
    for (key, value) in map {
        let key = key.as_str().unwrap_or("invalid-key");
        if name == "release-images.yml"
            && job == "publish"
            && key == "with"
            && action == Some("docker/login-action")
        {
            if let Some(config) = value.as_hash() {
                for (field_name, value) in config {
                    if field_name.as_str() == Some("password")
                        && value.as_str() == Some("${{ secrets.GITHUB_TOKEN }}")
                    {
                        continue;
                    }
                    scan_credentials(value, &format!("{location}.with"), problems);
                }
            } else {
                scan_credentials(value, location, problems);
            }
        } else if name == "release-images.yml"
            && job == "publish"
            && key == "env"
            && text(step, "run").is_some_and(super::release::is_attestation_verification)
        {
            if let Some(config) = value.as_hash() {
                for (field_name, value) in config {
                    if field_name.as_str() == Some("GH_TOKEN")
                        && value.as_str() == Some("${{ github.token }}")
                    {
                        continue;
                    }
                    scan_credentials(value, &format!("{location}.env"), problems);
                }
            } else {
                scan_credentials(value, location, problems);
            }
        } else {
            scan_credentials(value, location, problems);
        }
    }
}

pub(super) fn scan_except(
    node: &Yaml,
    location: &str,
    exceptions: &[&str],
    problems: &mut Vec<String>,
) {
    if let Some(map) = node.as_hash() {
        for (key, value) in map {
            if !key.as_str().is_some_and(|key| exceptions.contains(&key)) {
                scan_credentials(value, location, problems);
            }
        }
    }
}

fn scan_credentials(node: &Yaml, location: &str, problems: &mut Vec<String>) {
    match node {
        Yaml::String(value) => {
            let compact: String = value
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect();
            if dangerous_context(value)
                || compact.contains("secrets.")
                || compact.contains("secrets[")
                || compact.contains("github.token")
                || compact.contains("github['token']")
                || compact.contains("github[\"token\"]")
            {
                problems.push(format!("{location}: credential expressions are forbidden outside the two narrowly scoped release token boundaries"));
            }
        }
        Yaml::Hash(map) => {
            for (key, value) in map {
                scan_credentials(key, location, problems);
                scan_credentials(value, location, problems);
            }
        }
        Yaml::Array(values) => {
            for value in values {
                scan_credentials(value, location, problems);
            }
        }
        _ => {}
    }
}

fn dangerous_context(value: &str) -> bool {
    // An exact reviewed expression inventory, not a permissive expression parser.
    // Unknown expressions fail closed, including quoted-delimiter and serialization tricks.
    // Credential expressions occur only at the two explicit release exceptions above.
    const EXPRESSIONS: &[&str] = &[
        "matrix.os",
        "matrix.candidate",
        "matrix.service",
        "matrix.binary",
        "matrix.image",
        "github.ref",
        "github.ref_name",
        "github.repository",
        "github.actor",
        "github.sha",
        "github.workflow_ref",
        "env.REGISTRY",
        "env.PLATFORM",
        "env.COSIGN_VERSION",
        "env.SYFT_VERSION",
        "needs.plan.outputs.version",
        "needs.plan.outputs.matrix",
        "fromJSON(needs.plan.outputs.matrix)",
        "steps.plan.outputs.matrix",
        "steps.plan.outputs.version",
        "steps.build.outputs.digest",
        "steps.syft.outputs.cmd",
    ];
    let mut remainder = value;
    while let Some((_, expression)) = remainder.split_once("${{") {
        let Some((expression, tail)) = expression.split_once("}}") else {
            return true;
        };
        if !EXPRESSIONS.contains(&expression.trim()) {
            return true;
        }
        remainder = tail;
    }
    false
}
