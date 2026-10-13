//! Bind release privilege to digest-scoped publication, evidence, and verification.

use yaml_rust2::Yaml;

use super::{field, text};

const PLAN: &str = "python scripts/prepare_release.py --tag \"${RELEASE_TAG}\" --repository \"${RELEASE_REPOSITORY}\" --registry \"${RELEASE_REGISTRY}\" --github-output \"$GITHUB_OUTPUT\"";
const SBOM: &str = "python scripts/generate_sboms.py release --service \"${SERVICE}\" --image \"${IMAGE}\" --digest \"${DIGEST}\" --output artifacts/sbom --syft \"${SYFT_BINARY}\"";
const SIGN: &str = "cosign sign --yes \"${IMAGE}@${DIGEST}\"";
const VERIFY: &str = "cosign verify --certificate-identity \"${CERTIFICATE_IDENTITY}\" --certificate-oidc-issuer \"${OIDC_ISSUER}\" \"${IMAGE}@${DIGEST}\"";
const ATTEST_VERIFY: &str = "gh attestation verify \"oci://${IMAGE}@${DIGEST}\" --repo \"${REPOSITORY}\" --predicate-type https://slsa.dev/provenance/v1\ngh attestation verify \"oci://${IMAGE}@${DIGEST}\" --repo \"${REPOSITORY}\" --predicate-type https://cyclonedx.org/bom";

pub(super) fn is_attestation_verification(run: &str) -> bool {
    canonical(run) == canonical(ATTEST_VERIFY)
}

pub(super) fn check(root: &Yaml, problems: &mut Vec<String>) {
    for (key, expected) in [
        ("REGISTRY", "ghcr.io"),
        ("PLATFORM", "linux/amd64"),
        ("COSIGN_VERSION", "v3.1.3"),
        ("SYFT_VERSION", "1.52.0"),
    ] {
        if field(root, "env").and_then(|env| text(env, key)) != Some(expected) {
            problems.push(format!(
                "workflow.env.{key}: release tool and target must retain the reviewed literal value"
            ));
        }
    }
    if field(root, "concurrency")
        .and_then(|value| field(value, "cancel-in-progress"))
        .and_then(Yaml::as_bool)
        != Some(false)
    {
        problems.push(
            "workflow.concurrency: release must not cancel in-progress publication".to_owned(),
        );
    }
    let Some(jobs) = field(root, "jobs") else {
        return;
    };
    let Some(plan) = field(jobs, "plan") else {
        return;
    };
    let Some(publish) = field(jobs, "publish") else {
        return;
    };
    if field(plan, "outputs").is_none_or(|outputs| {
        text(outputs, "matrix") != Some("${{ steps.plan.outputs.matrix }}")
            || text(outputs, "version") != Some("${{ steps.plan.outputs.version }}")
    }) {
        problems.push(
            "jobs.plan.outputs: release outputs must originate in the protected planner step"
                .to_owned(),
        );
    }
    if field(publish, "strategy").is_none_or(|strategy| {
        text(strategy, "matrix") != Some("${{ fromJSON(needs.plan.outputs.matrix) }}")
    }) {
        problems.push(
            "jobs.publish.strategy: release matrix must originate in the protected planner job"
                .to_owned(),
        );
    }
    let Some(plan_steps) = field(plan, "steps").and_then(Yaml::as_vec) else {
        return;
    };
    required_run(
        plan_steps,
        "plan",
        PLAN,
        &[
            ("RELEASE_TAG", "${{ github.ref_name }}"),
            ("RELEASE_REPOSITORY", "${{ github.repository }}"),
            ("RELEASE_REGISTRY", "${{ env.REGISTRY }}"),
        ],
        Some("plan"),
        problems,
    );
    let Some(steps) = field(publish, "steps").and_then(Yaml::as_vec) else {
        return;
    };
    let digest_env = [
        ("IMAGE", "${{ matrix.image }}"),
        ("DIGEST", "${{ steps.build.outputs.digest }}"),
    ];
    required_run(
        steps,
        "publish",
        SBOM,
        &[
            ("SERVICE", "${{ matrix.service }}"),
            ("IMAGE", "${{ matrix.image }}"),
            ("DIGEST", "${{ steps.build.outputs.digest }}"),
            ("SYFT_BINARY", "${{ steps.syft.outputs.cmd }}"),
        ],
        None,
        problems,
    );
    required_run(steps, "publish", SIGN, &digest_env, None, problems);
    required_run(
        steps,
        "publish",
        VERIFY,
        &[
            ("IMAGE", "${{ matrix.image }}"),
            ("DIGEST", "${{ steps.build.outputs.digest }}"),
            (
                "CERTIFICATE_IDENTITY",
                "https://github.com/${{ github.workflow_ref }}",
            ),
            ("OIDC_ISSUER", "https://token.actions.githubusercontent.com"),
        ],
        None,
        problems,
    );
    required_run(
        steps,
        "publish",
        ATTEST_VERIFY,
        &[
            ("IMAGE", "${{ matrix.image }}"),
            ("DIGEST", "${{ steps.build.outputs.digest }}"),
            ("GH_TOKEN", "${{ github.token }}"),
            ("REPOSITORY", "${{ github.repository }}"),
        ],
        None,
        problems,
    );
    required_action(
        steps,
        "docker/login-action",
        &[
            ("registry", "${{ env.REGISTRY }}"),
            ("username", "${{ github.actor }}"),
            ("password", "${{ secrets.GITHUB_TOKEN }}"),
        ],
        None,
        problems,
    );
    required_action(
        steps,
        "docker/build-push-action",
        &[
            ("context", "."),
            ("file", "Dockerfile"),
            ("platforms", "${{ env.PLATFORM }}"),
        ],
        Some("build"),
        problems,
    );
    let Some(build) = action_steps(steps, "docker/build-push-action").next() else {
        return;
    };
    let config = field(build, "with");
    for (key, expected) in [
        ("push", true),
        ("pull", true),
        ("provenance", false),
        ("sbom", false),
    ] {
        if config
            .and_then(|config| field(config, key))
            .and_then(Yaml::as_bool)
            != Some(expected)
        {
            problems.push(format!("jobs.publish.build.with.{key}: must retain the reviewed publication/evidence policy"));
        }
    }
    let attestations: Vec<_> = action_steps(steps, "actions/attest").collect();
    if attestations.len() != 2
        || attestations
            .iter()
            .filter(|step| {
                field(step, "with").is_some_and(|config| {
                    text(config, "sbom-path")
                        == Some("artifacts/sbom/${{ matrix.binary }}.image.cdx.json")
                })
            })
            .count()
            != 1
        || attestations
            .iter()
            .filter(|step| {
                field(step, "with").is_some_and(|config| field(config, "sbom-path").is_none())
            })
            .count()
            != 1
    {
        problems.push("jobs.publish: exactly one digest-scoped provenance attestation and one SBOM attestation are required".to_owned());
    }
    for step in attestations {
        if field(step, "with").is_none_or(|config| {
            text(config, "subject-name") != Some("${{ matrix.image }}")
                || text(config, "subject-digest") != Some("${{ steps.build.outputs.digest }}")
                || text(config, "subject-version") != Some("${{ needs.plan.outputs.version }}")
                || field(config, "push-to-registry").and_then(Yaml::as_bool) != Some(true)
        }) {
            problems.push("jobs.publish: attestations must bind the protected build digest and attach evidence to the registry".to_owned());
        }
    }
    let indexes = [
        "docker/login-action",
        "docker/build-push-action",
        "actions/attest",
    ]
    .map(|action| steps.iter().position(|step| is_action(step, action)));
    if !matches!(indexes, [Some(login), Some(build), Some(attest)] if login < build && build < attest)
    {
        problems.push(
            "jobs.publish: login, build, and evidence creation must retain their reviewed order"
                .to_owned(),
        );
    }
}

fn canonical(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn required_run(
    steps: &[Yaml],
    job: &str,
    command: &str,
    env: &[(&str, &str)],
    id: Option<&str>,
    problems: &mut Vec<String>,
) {
    let matches: Vec<_> = steps
        .iter()
        .filter(|step| text(step, "run").is_some_and(|run| canonical(run) == canonical(command)))
        .collect();
    if matches.len() != 1
        || !matches.iter().all(|step| {
            id.is_none_or(|id| text(step, "id") == Some(id))
                && field(step, "env").is_some_and(|config| {
                    env.iter()
                        .all(|(key, value)| text(config, key) == Some(value))
                })
        })
    {
        problems.push(format!("jobs.{job}: protected release command and its exact environment bindings are missing, changed, or duplicated"));
    }
}

fn required_action(
    steps: &[Yaml],
    action: &str,
    config: &[(&str, &str)],
    id: Option<&str>,
    problems: &mut Vec<String>,
) {
    let matches: Vec<_> = action_steps(steps, action).collect();
    if matches.len() != 1
        || !matches.iter().all(|step| {
            id.is_none_or(|id| text(step, "id") == Some(id))
                && field(step, "with").is_some_and(|values| {
                    config
                        .iter()
                        .all(|(key, value)| text(values, key) == Some(value))
                })
        })
    {
        problems.push(format!("jobs.publish: protected {action} invocation and its credential/digest bindings are required exactly once"));
    }
}

fn is_action(step: &Yaml, action: &str) -> bool {
    text(step, "uses")
        .and_then(|uses| uses.split_once('@'))
        .is_some_and(|(repository, _)| repository == action)
}

fn action_steps<'a>(steps: &'a [Yaml], action: &'a str) -> impl Iterator<Item = &'a Yaml> {
    steps.iter().filter(move |step| is_action(step, action))
}
