//! Exact environment routes prevent redirecting a protected command or scanner.

use yaml_rust2::Yaml;

use super::{field, keys, text};

const DATABASE_URL: &str =
    "postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent";

pub(super) fn workflow(name: &str, root: &Yaml, problems: &mut Vec<String>) {
    let expected = if name == "release-images.yml" {
        &[
            ("REGISTRY", "ghcr.io"),
            ("PLATFORM", "linux/amd64"),
            ("COSIGN_VERSION", "v3.1.3"),
            ("SYFT_VERSION", "1.52.0"),
        ][..]
    } else {
        &[]
    };
    check(root, expected, "workflow.env", problems);
}

pub(super) fn job(name: &str, id: &str, node: &Yaml, problems: &mut Vec<String>) {
    let expected = if name == "repository-quality.yml" && id == "supply-chain" {
        &[
            ("CARGO_CYCLONEDX_VERSION", "0.5.9"),
            ("SYFT_VERSION", "1.52.0"),
        ][..]
    } else {
        &[]
    };
    check(node, expected, &format!("jobs.{id}.env"), problems);
}

pub(super) fn step(name: &str, job: &str, step: &Yaml, location: &str, problems: &mut Vec<String>) {
    let run = text(step, "run").unwrap_or("");
    let expected: &[(&str, &str)] = match (name, job) {
        ("repository-quality.yml", "local-platform-smoke")
            if run == "cargo xtask verify sqlx-metadata" =>
        {
            &[
                ("DATABASE_URL", DATABASE_URL),
                ("SQLX_OFFLINE", "false"),
                ("EDGEAGENT_SQLX_METADATA_LOCAL", "1"),
            ]
        }
        ("repository-quality.yml", "local-platform-smoke")
            if run == "cargo xtask verify local-platform" =>
        {
            &[
                ("EDGEAGENT_NATS_URL", "nats://127.0.0.1:4222"),
                ("EDGEAGENT_POSTGRES_URL", DATABASE_URL),
                ("EDGEAGENT_NATS_COMPOSE_RESTART", "1"),
            ]
        }
        ("repository-quality.yml", "supply-chain")
            if run.starts_with("python scripts/generate_sboms.py images ") =>
        {
            &[("SYFT_BINARY", "${{ steps.syft.outputs.cmd }}")]
        }
        ("release-images.yml", "plan") if run.starts_with("python scripts/prepare_release.py ") => {
            &[
                ("RELEASE_TAG", "${{ github.ref_name }}"),
                ("RELEASE_REPOSITORY", "${{ github.repository }}"),
                ("RELEASE_REGISTRY", "${{ env.REGISTRY }}"),
            ]
        }
        ("release-images.yml", "publish")
            if run.starts_with("python scripts/generate_sboms.py release ") =>
        {
            &[
                ("SERVICE", "${{ matrix.service }}"),
                ("IMAGE", "${{ matrix.image }}"),
                ("DIGEST", "${{ steps.build.outputs.digest }}"),
                ("SYFT_BINARY", "${{ steps.syft.outputs.cmd }}"),
            ]
        }
        ("release-images.yml", "publish") if run.starts_with("cosign sign ") => &[
            ("IMAGE", "${{ matrix.image }}"),
            ("DIGEST", "${{ steps.build.outputs.digest }}"),
        ],
        ("release-images.yml", "publish") if run.starts_with("cosign verify ") => &[
            ("IMAGE", "${{ matrix.image }}"),
            ("DIGEST", "${{ steps.build.outputs.digest }}"),
            (
                "CERTIFICATE_IDENTITY",
                "https://github.com/${{ github.workflow_ref }}",
            ),
            ("OIDC_ISSUER", "https://token.actions.githubusercontent.com"),
        ],
        ("release-images.yml", "publish") if super::release::is_attestation_verification(run) => &[
            ("IMAGE", "${{ matrix.image }}"),
            ("DIGEST", "${{ steps.build.outputs.digest }}"),
            ("GH_TOKEN", "${{ github.token }}"),
            ("REPOSITORY", "${{ github.repository }}"),
        ],
        _ => &[],
    };
    check(step, expected, &format!("{location}.env"), problems);
}

fn check(node: &Yaml, expected: &[(&str, &str)], location: &str, problems: &mut Vec<String>) {
    let valid = match field(node, "env") {
        None => expected.is_empty(),
        Some(env) => {
            keys(env) == Some(expected.iter().map(|(key, _)| *key).collect())
                && expected
                    .iter()
                    .all(|(key, value)| text(env, key) == Some(value))
        }
    };
    if !valid {
        // Never print the supplied value: it may include credentials or an unsafe expression.
        problems.push(format!("{location}: must match the exact reviewed environment bindings; command/scanner redirection overrides are forbidden"));
    }
}
