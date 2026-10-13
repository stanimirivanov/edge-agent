//! Actual dependency and SBOM steps, not quoted command fragments, provide evidence.

use yaml_rust2::Yaml;

use super::{field, text};

pub(super) fn check(name: &str, root: &Yaml, problems: &mut Vec<String>) {
    let job = match name {
        "repository-quality.yml" => "supply-chain",
        "m08-durable-execution.yml" => "proof",
        _ => return,
    };
    let Some(steps) = field(root, "jobs")
        .and_then(|jobs| field(jobs, job))
        .and_then(|job| field(job, "steps"))
        .and_then(Yaml::as_vec)
    else {
        return;
    };
    let denies: Vec<_> = steps
        .iter()
        .filter(|step| action(step) == Some("EmbarkStudios/cargo-deny-action"))
        .collect();
    let arguments = if job == "proof" {
        "--locked --workspace --config experiments/deny.toml"
    } else {
        "--locked"
    };
    if denies.len() != 1
        || !denies.iter().all(|step| {
            field(step, "with").is_some_and(|config| {
                text(config, "rust-version") == Some("1.98.1")
                    && text(config, "arguments") == Some(arguments)
                    && text(config, "command") == Some("check")
                    && (job != "proof"
                        || text(config, "manifest-path")
                            == Some("experiments/${{ matrix.candidate }}/Cargo.toml"))
            })
        })
    {
        problems.push(format!(
            "jobs.{job}: pinned cargo-deny must execute the actual locked policy check exactly once"
        ));
    }
    if job != "supply-chain" {
        return;
    }
    for command in [
        "cargo install --locked cargo-cyclonedx --version \"$CARGO_CYCLONEDX_VERSION\"",
        "python scripts/generate_sboms.py rust --output artifacts/sbom",
        "python scripts/generate_sboms.py images --output artifacts/sbom --syft \"${SYFT_BINARY}\"",
        "python scripts/verify_supply_chain.py --artifacts artifacts/sbom",
    ] {
        if steps
            .iter()
            .filter(|step| text(step, "run") == Some(command))
            .count()
            != 1
        {
            problems.push("jobs.supply-chain: actual locked SBOM installation, generation, and artifact validation commands are required without bypass suffixes".to_owned());
        }
    }
    let syft: Vec<_> = steps
        .iter()
        .filter(|step| action(step) == Some("anchore/sbom-action/download-syft"))
        .collect();
    if syft.len() != 1
        || !syft.iter().all(|step| {
            text(step, "id") == Some("syft")
                && field(step, "with").is_some_and(|config| {
                    text(config, "syft-version") == Some("v${{ env.SYFT_VERSION }}")
                })
        })
    {
        problems.push(
            "jobs.supply-chain: pinned Syft installation must own the protected syft output"
                .to_owned(),
        );
    }
    let uploads: Vec<_> = steps
        .iter()
        .filter(|step| action(step) == Some("actions/upload-artifact"))
        .collect();
    if uploads.len() != 1
        || !uploads.iter().all(|step| {
            field(step, "with").is_some_and(|config| {
                text(config, "name") == Some("edgeagent-sboms-${{ github.sha }}")
                    && text(config, "path") == Some("artifacts/sbom/*.cdx.json")
                    && text(config, "if-no-files-found") == Some("error")
                    && field(config, "retention-days").and_then(Yaml::as_i64) == Some(14)
            })
        })
    {
        problems.push("jobs.supply-chain: complete SBOM evidence must be uploaded with fail-closed missing-artifact handling and reviewed retention".to_owned());
    }
}

fn action(step: &Yaml) -> Option<&str> {
    text(step, "uses")?
        .split_once('@')
        .map(|(action, _)| action)
}
