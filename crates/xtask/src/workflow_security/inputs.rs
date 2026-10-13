//! Closed action inputs prevent a pinned action from receiving unreviewed authority.

use yaml_rust2::Yaml;

use super::{field, keys};

enum Value {
    Text(&'static str),
    Boolean(bool),
    Integer(i64),
}

pub(super) fn check(
    name: &str,
    job: &str,
    step: &Yaml,
    action: Option<&str>,
    location: &str,
    problems: &mut Vec<String>,
) {
    use Value::{Boolean as B, Integer as I, Text as T};
    let expected: &[(&str, Value)] = match action {
        None => &[],
        Some("actions/checkout") if name == "repository-quality.yml" && job == "security" => {
            &[("persist-credentials", B(false)), ("fetch-depth", I(0))]
        }
        Some("actions/checkout") if name == "repository-quality.yml" && job == "supply-chain" => {
            &[("persist-credentials", B(false)), ("fetch-depth", I(1))]
        }
        Some("actions/checkout") => &[("persist-credentials", B(false))],
        Some("actions/setup-python") => &[("python-version", T("3.13"))],
        Some("EmbarkStudios/cargo-deny-action") if name == "m08-durable-execution.yml" => &[
            ("rust-version", T("1.98.1")),
            (
                "manifest-path",
                T("experiments/${{ matrix.candidate }}/Cargo.toml"),
            ),
            (
                "arguments",
                T("--locked --workspace --config experiments/deny.toml"),
            ),
            ("command", T("check")),
        ],
        Some("EmbarkStudios/cargo-deny-action") => &[
            ("rust-version", T("1.98.1")),
            ("arguments", T("--locked")),
            ("command", T("check")),
        ],
        Some("anchore/sbom-action/download-syft") => {
            &[("syft-version", T("v${{ env.SYFT_VERSION }}"))]
        }
        Some("actions/upload-artifact") => &[
            ("name", T("edgeagent-sboms-${{ github.sha }}")),
            ("path", T("artifacts/sbom/*.cdx.json")),
            ("if-no-files-found", T("error")),
            ("retention-days", I(14)),
        ],
        Some("docker/login-action") => &[
            ("registry", T("${{ env.REGISTRY }}")),
            ("username", T("${{ github.actor }}")),
            ("password", T("${{ secrets.GITHUB_TOKEN }}")),
        ],
        Some("docker/setup-buildx-action") => &[],
        Some("docker/build-push-action") => &[
            ("context", T(".")),
            ("file", T("Dockerfile")),
            ("platforms", T("${{ env.PLATFORM }}")),
            ("pull", B(true)),
            ("push", B(true)),
            ("provenance", B(false)),
            ("sbom", B(false)),
            (
                "tags",
                T(
                    "${{ matrix.image }}:${{ needs.plan.outputs.version }}\n${{ matrix.image }}:${{ github.sha }}",
                ),
            ),
            (
                "build-args",
                T("SERVICE=${{ matrix.service }}\nSOURCE_REVISION=${{ github.sha }}"),
            ),
            (
                "labels",
                T(
                    "org.opencontainers.image.source=https://github.com/${{ github.repository }}\norg.opencontainers.image.revision=${{ github.sha }}\norg.opencontainers.image.version=${{ needs.plan.outputs.version }}",
                ),
            ),
        ],
        Some("actions/attest")
            if field(step, "with").is_some_and(|config| field(config, "sbom-path").is_some()) =>
        {
            &[
                ("subject-name", T("${{ matrix.image }}")),
                ("subject-digest", T("${{ steps.build.outputs.digest }}")),
                ("subject-version", T("${{ needs.plan.outputs.version }}")),
                ("push-to-registry", B(true)),
                (
                    "sbom-path",
                    T("artifacts/sbom/${{ matrix.binary }}.image.cdx.json"),
                ),
            ]
        }
        Some("actions/attest") => &[
            ("subject-name", T("${{ matrix.image }}")),
            ("subject-digest", T("${{ steps.build.outputs.digest }}")),
            ("subject-version", T("${{ needs.plan.outputs.version }}")),
            ("push-to-registry", B(true)),
        ],
        Some("sigstore/cosign-installer") => &[("cosign-release", T("${{ env.COSIGN_VERSION }}"))],
        Some(_) => return,
    };
    let valid = match field(step, "with") {
        None => expected.is_empty(),
        Some(config) => {
            keys(config) == Some(expected.iter().map(|(key, _)| *key).collect())
                && expected.iter().all(|(key, expected)| {
                    field(config, key).is_some_and(|value| match expected {
                        Value::Text(expected) => value
                            .as_str()
                            .is_some_and(|value| value.trim_end() == *expected),
                        Value::Boolean(expected) => value.as_bool() == Some(*expected),
                        Value::Integer(expected) => value.as_i64() == Some(*expected),
                    })
                })
        }
    };
    if !valid {
        problems.push(format!("{location}.with: must match the action's exact reviewed input keys, typed values, and capability bindings"));
    }
}
