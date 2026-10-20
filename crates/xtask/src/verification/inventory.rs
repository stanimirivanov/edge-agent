//! One ordered inventory for local and CI acceptance; selectors cannot edit argv.

use super::{Check, CommandSpec, Discovery, Program, Requirement, proofs};

pub(super) const PROFILES: &[&str] = &[
    "verify",
    "fmt",
    "repository",
    "architecture",
    "check",
    "test",
    "local-platform",
    "m02-qualification",
    "image-smoke",
    "supply-chain",
    "dependency-audit",
    "sqlx-metadata",
    "ci-security",
    "m08-event-spine-proof",
    "m08-durable-execution",
    "m08-dbos-proof",
    "m08-temporal-proof",
    "m08-flawless-proof",
    "m08-event-spine-proof-static",
    "m08-durable-execution-static",
    "m08-dbos-proof-static",
    "m08-temporal-proof-static",
    "m08-flawless-proof-static",
    "m08-event-spine-proof-test",
    "m08-durable-execution-test",
    "m08-dbos-proof-test",
    "m08-temporal-proof-test",
    "m08-flawless-proof-test",
];

fn check(
    id: &'static str,
    tier: &'static str,
    profiles: &'static [&'static str],
    program: Program,
    args: &[&'static str],
    evidence: &'static str,
) -> Check {
    Check {
        id,
        tier,
        profiles,
        command: CommandSpec {
            program,
            args: args.to_vec(),
        },
        prerequisites: &[],
        discovery: None,
        evidence,
    }
}

pub(super) fn checks() -> Vec<Check> {
    use Program::{Cargo, Python, Xtask};
    let mut checks = vec![
        check(
            "text-format",
            "T2",
            &["verify", "fmt"],
            Python,
            &["scripts/verify_repository.py", "--format-check"],
            "UTF-8, LF and final-newline contract; not prose correctness.",
        ),
        check(
            "rust-format",
            "T2",
            &["verify", "fmt"],
            Cargo,
            &["fmt", "--all", "--check"],
            "Pinned rustfmt output; not architecture or runtime behavior.",
        ),
        check(
            "repository-bytes",
            "T1",
            &["verify", "check", "repository"],
            Python,
            &["scripts/verify_repository.py"],
            "Required repository inventory and independent byte/document baseline.",
        ),
        check(
            "repository-policy",
            "T1",
            &["verify", "check", "repository"],
            Xtask,
            &["repository"],
            "Rendered local navigation, governance and canonical entrypoint parity; not factual freshness.",
        ),
        check(
            "architecture-declarations",
            "T1",
            &["verify", "check", "architecture"],
            Python,
            &["scripts/verify_architecture.py"],
            "All direct manifest declarations, including inactive target and optional dependencies.",
        ),
        check(
            "workflow-security",
            "T1",
            &[
                "verify",
                "check",
                "architecture",
                "ci-security",
                "dependency-audit",
            ],
            Xtask,
            &["workflow-security"],
            "Parsed CI action, trigger, credential and permission policy; not remote runner isolation or branch protection.",
        ),
        check(
            "secret-policy",
            "T1",
            &["verify", "check", "repository", "ci-security"],
            Python,
            &["scripts/verify_secrets.py", "--policy-only"],
            "Pinned scanner and exact fixture exception configuration; not an executed credential scan.",
        ),
        check(
            "architecture-graph",
            "T1",
            &["verify", "check", "architecture"],
            Xtask,
            &["architecture"],
            "Cargo-normalized internal edges and capability policy; not semantic cohesion.",
        ),
        check(
            "image-contracts",
            "T2",
            &["verify", "check"],
            Python,
            &["scripts/verify_images.py"],
            "Static image build/runtime declarations, not a successful image build.",
        ),
        check(
            "local-stack-contracts",
            "T2",
            &["verify", "check"],
            Python,
            &["scripts/verify_local_stack.py"],
            "Static portable local-profile contracts, not running service readiness.",
        ),
        check(
            "supply-chain-contracts",
            "T2",
            &["verify", "check", "dependency-audit"],
            Python,
            &["scripts/verify_supply_chain.py"],
            "Declared dependency/SBOM policy shape; not fresh advisories or generated SBOM completeness.",
        ),
        check(
            "release-contracts",
            "T2",
            &["verify", "check"],
            Python,
            &["scripts/verify_release.py"],
            "Static release contract shape; not signatures of published artifacts.",
        ),
        check(
            "locked-graph",
            "T2",
            &["verify", "check"],
            Cargo,
            &[
                "metadata",
                "--locked",
                "--offline",
                "--format-version",
                "1",
                "--no-deps",
            ],
            "Root workspace locked metadata resolves without network; not isolated experiment locks.",
        ),
        check(
            "rust-check",
            "T2",
            &["verify", "check"],
            Cargo,
            &["check", "--locked", "--workspace", "--all-targets"],
            "Every root workspace target compiles with committed SQLx offline metadata.",
        ),
        check(
            "rust-clippy",
            "T2",
            &["verify", "check"],
            Cargo,
            &[
                "clippy",
                "--locked",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
            "Workspace lint policy with warnings rejected.",
        ),
        check(
            "sqlx-query-policy",
            "T2",
            &["verify", "check"],
            Python,
            &["scripts/verify_sqlx_query_policy.py"],
            "Real offline Clippy fixtures accept checked SQL and reject outbox runtime/unchecked query APIs, including aliases.",
        ),
        check(
            "harness-tests",
            "T2",
            &["verify", "test"],
            Python,
            &[
                "-m",
                "unittest",
                "discover",
                "-s",
                "tests",
                "-p",
                "test_*.py",
            ],
            "Deterministic Python mutation and script tests.",
        ),
        check(
            "ignored-inventory",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "--workspace",
                "--all-targets",
                "--",
                "--ignored",
                "--list",
            ],
            "Exact ignored-test census; neither discovery nor compilation executes service proofs.",
        ),
        check(
            "rust-tests",
            "T2",
            &["verify", "test"],
            Cargo,
            &["test", "--locked", "--workspace", "--all-targets"],
            "Credential-free unit/integration contracts; ignored service tests remain unproved.",
        ),
        check(
            "rust-doctests",
            "T2",
            &["verify", "test"],
            Cargo,
            &["test", "--locked", "--workspace", "--doc"],
            "Public API positive and compile-fail examples, absent from --all-targets.",
        ),
        check(
            "messaging-error-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-messaging",
                "--test",
                "error_redaction",
                "--",
            ],
            "Explicit payload-safe public error target retained from CI, with a protected named-test census.",
        ),
        check(
            "relay-observation-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-outbox-relay",
                "--test",
                "telemetry",
                "--",
            ],
            "Isolated relay telemetry trust boundary and confirmed outcome matrix; named discovery rejects missing or renamed security regressions.",
        ),
        check(
            "relay-policy-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-outbox-relay",
                "--test",
                "policy_contracts",
                "--",
            ],
            "Typed startup bounds/precision and independent runtime category/source contracts; protected named discovery is credential-free.",
        ),
        check(
            "contracts-preserve-order",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-contracts",
                "--features",
                "serde_json/preserve_order",
            ],
            "Canonical persisted envelope bytes survive serde_json feature unification.",
        ),
        check(
            "message-schema-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-event-spine-conformance",
                "--test",
                "schema_contracts",
                "--features",
                "serde_json/preserve_order",
                "--",
            ],
            "Generated local schema freshness, frozen baselines, typed payload/redaction and conservative optional-addition contracts; no broker/process-recovery claim.",
        ),
        check(
            "image-smoke",
            "T3",
            &["image-smoke"],
            Python,
            &["scripts/verify_images.py", "--build"],
            "OCI builds and non-root/read-only/networkless smoke tests, not cloud deployment.",
        ),
        check(
            "simulation-schema-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-event-spine-conformance",
                "--test",
                "simulation_contracts",
                "--features",
                "serde_json/preserve_order",
                "--",
            ],
            "Protected synthetic admission constructors, schemas, mode rejection and immutable published v1 identities; no provider qualification.",
        ),
        check(
            "sqlx-metadata",
            "T3",
            &["sqlx-metadata"],
            Xtask,
            &["sqlx-metadata"],
            "Regenerated query metadata matches both migration directories; requires fresh isolated local Compose PostgreSQL and pinned SQLx CLI, not runtime atomicity.",
        ),
        check(
            "dependency-policy",
            "T3",
            &["supply-chain", "dependency-audit"],
            Cargo,
            &["deny", "--locked", "check"],
            "Current advisory/license/source/bans policy; requires network and cargo-deny.",
        ),
        check(
            "sbom-rust",
            "T3",
            &["supply-chain"],
            Python,
            &[
                "scripts/generate_sboms.py",
                "rust",
                "--output",
                "artifacts/sbom",
            ],
            "Generated Rust binary SBOMs with pinned cargo-cyclonedx.",
        ),
        check(
            "sbom-images",
            "T3",
            &["supply-chain"],
            Python,
            &[
                "scripts/generate_sboms.py",
                "images",
                "--output",
                "artifacts/sbom",
            ],
            "Generated image SBOMs with pinned Syft and Docker.",
        ),
        check(
            "sbom-completeness",
            "T3",
            &["supply-chain"],
            Python,
            &[
                "scripts/verify_supply_chain.py",
                "--artifacts",
                "artifacts/sbom",
            ],
            "Complete generated artifact set matches declared deployables; not release attestation.",
        ),
    ];
    checks.push(check(
        "secret-scanner-contracts",
        "T3",
        &["ci-security"],
        Python,
        &["scripts/verify_secrets.py", "--self-test"],
        "Actual pinned scanner rejects synthetic credentials and proves path-and-value fixture exceptions; not repository cleanliness.",
    ));
    checks.push(check(
        "secret-scan",
        "T3",
        &["ci-security"],
        Python,
        &["scripts/verify_secrets.py"],
        "Pinned redacted scan of source working tree and complete reachable Git history; not privacy or licensed-data certification.",
    ));
    for entry in &mut checks {
        match entry.id {
            "simulation-schema-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "admission_wire_schema_and_constructor_reject_unsupported_modes_and_invalid_facts",
                        "published_admission_schemas_retain_immutable_content_and_identity",
                    ],
                    ignored: false,
                });
            }
            "message-schema-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "generated_schemas_match_the_complete_committed_inventory",
                        "schema_inventory_rejects_stale_missing_extra_and_nonregular_artifacts",
                        "retained_baselines_preserve_schema_identity_and_existing_field_constraints",
                        "optional_additions_preserve_old_readers_and_default_new_reader_fields",
                        "compatibility_policy_rejects_breaking_and_unclassified_schema_mutations",
                        "typed_command_and_fact_round_trip_exact_golden_envelopes",
                        "payload_fixture_matrix_rejects_invalid_shapes_and_boundary_values",
                        "version_schema_producer_and_partition_mismatches_never_yield_typed_messages",
                        "constructor_serde_and_raw_byte_boundaries_cannot_bypass_validation",
                        "public_diagnostics_redact_payloads_and_rejected_metadata",
                    ],
                    ignored: false,
                });
            }
            "ignored-inventory" => {
                entry.discovery = Some(Discovery {
                    args: entry.command.args.clone(),
                    expected: proofs::ignored_inventory(),
                    ignored: true,
                })
            }
            "messaging-error-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "publish_error_constructors_preserve_all_categories_and_actual_source_presence",
                        "consume_error_constructors_preserve_all_categories_and_actual_source_presence",
                        "inbox_error_constructors_preserve_all_categories_and_actual_source_presence",
                        "outbox_error_constructors_preserve_all_categories_and_actual_source_presence",
                        "source_free_store_failure_terminates_the_inbound_processing_source_chain",
                        "public_messaging_errors_redact_causes_but_preserve_source_chains",
                    ],
                    ignored: false,
                });
            }
            "relay-observation-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec!["relay_observations_respect_validation_and_confirmed_outcomes"],
                    ignored: false,
                });
            }
            "image-smoke" | "sbom-images" => {
                entry.prerequisites = &[Requirement::Executable("docker")]
            }
            "relay-policy-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "attempt_budgets_validate_inclusive_limits_and_round_trip",
                        "backoff_validation_preserves_precision_and_rejects_invalid_intervals",
                        "owner_errors_are_startup_only_and_payload_safe",
                        "storage_failures_preserve_all_runtime_categories_and_real_source_presence",
                        "configuration_and_runtime_types_preserve_worker_auto_traits",
                    ],
                    ignored: false,
                });
            }
            "dependency-policy" => entry.prerequisites = &[Requirement::Executable("cargo-deny")],
            "sbom-rust" => entry.prerequisites = &[Requirement::Executable("cargo-cyclonedx")],
            "sqlx-metadata" => {
                entry.prerequisites = &[
                    Requirement::Environment("DATABASE_URL"),
                    Requirement::EnvironmentEquals("SQLX_OFFLINE", "false"),
                    Requirement::EnvironmentEquals("EDGEAGENT_SQLX_METADATA_LOCAL", "1"),
                    Requirement::Executable("docker"),
                    Requirement::Executable("cargo-sqlx"),
                ]
            }
            "secret-scan" | "secret-scanner-contracts" => {
                entry.prerequisites = &[Requirement::CommandProbe("gitleaks", &["version"])]
            }
            _ => (),
        }
    }
    checks.extend(proofs::checks());
    checks.extend(m08_checks());
    checks
}

// Expose actual registered audit targets to the coverage sensor, not a second
// allowlist that could stay green after a new workspace was added elsewhere.
pub(crate) fn dependency_policy_targets() -> Vec<(&'static str, &'static str)> {
    fn argument(args: &[&'static str], flag: &str, default: &'static str) -> &'static str {
        args.iter()
            .position(|argument| *argument == flag)
            .and_then(|index| args.get(index + 1))
            .copied()
            .unwrap_or(default)
    }

    checks()
        .into_iter()
        .filter(|check| {
            check.profiles.contains(&"dependency-audit")
                && check.command.program == Program::Cargo
                && check.command.args.first() == Some(&"deny")
        })
        .map(|check| {
            (
                argument(&check.command.args, "--manifest-path", "Cargo.toml"),
                argument(&check.command.args, "--config", "deny.toml"),
            )
        })
        .collect()
}

fn m08_checks() -> Vec<Check> {
    let mut checks = Vec::new();
    for (profile, manifest, fmt_id, lint_id, test_id, audit_id) in [
        (
            "m08-event-spine-proof",
            "experiments/event-spine-proof/Cargo.toml",
            "m08-event-spine-format",
            "m08-event-spine-clippy",
            "m08-event-spine-test",
            "m08-event-spine-dependency-policy",
        ),
        (
            "m08-durable-execution",
            "experiments/durable-execution/Cargo.toml",
            "m08-duroxide-format",
            "m08-duroxide-clippy",
            "m08-duroxide-test",
            "m08-duroxide-dependency-policy",
        ),
        (
            "m08-dbos-proof",
            "experiments/dbos-proof/Cargo.toml",
            "m08-dbos-format",
            "m08-dbos-clippy",
            "m08-dbos-test",
            "m08-dbos-dependency-policy",
        ),
        (
            "m08-temporal-proof",
            "experiments/temporal-proof/Cargo.toml",
            "m08-temporal-format",
            "m08-temporal-clippy",
            "m08-temporal-test",
            "m08-temporal-dependency-policy",
        ),
        (
            "m08-flawless-proof",
            "experiments/flawless-proof/Cargo.toml",
            "m08-flawless-format",
            "m08-flawless-clippy",
            "m08-flawless-test",
            "m08-flawless-dependency-policy",
        ),
    ] {
        let mut audit = check(
            audit_id,
            "T3",
            &["dependency-audit"],
            Program::Cargo,
            &[
                "deny",
                "--manifest-path",
                manifest,
                "--locked",
                "--workspace",
                "--config",
                "experiments/deny.toml",
                "check",
            ],
            "Current isolated proof advisory/license/source/bans policy; requires network and cargo-deny, not a runtime proof.",
        );
        audit.prerequisites = &[Requirement::Executable("cargo-deny")];
        checks.push(audit);
        // Profiles are static policy values; each generated check retains one static slice below.
        let static_profiles: &'static [&'static str] = match profile {
            "m08-event-spine-proof" => &["m08-event-spine-proof", "m08-event-spine-proof-static"],
            "m08-durable-execution" => &["m08-durable-execution", "m08-durable-execution-static"],
            "m08-dbos-proof" => &["m08-dbos-proof", "m08-dbos-proof-static"],
            "m08-temporal-proof" => &["m08-temporal-proof", "m08-temporal-proof-static"],
            _ => &["m08-flawless-proof", "m08-flawless-proof-static"],
        };
        let test_profiles: &'static [&'static str] = match profile {
            "m08-event-spine-proof" => &["m08-event-spine-proof", "m08-event-spine-proof-test"],
            "m08-durable-execution" => &["m08-durable-execution", "m08-durable-execution-test"],
            "m08-dbos-proof" => &["m08-dbos-proof", "m08-dbos-proof-test"],
            "m08-temporal-proof" => &["m08-temporal-proof", "m08-temporal-proof-test"],
            _ => &["m08-flawless-proof", "m08-flawless-proof-test"],
        };
        checks.push(check(
            fmt_id,
            "T3",
            static_profiles,
            Program::Cargo,
            &["fmt", "--manifest-path", manifest, "--all", "--check"],
            "Isolated M08 proof formatting, not root workspace acceptance.",
        ));
        checks.push(check(
            lint_id,
            "T3",
            static_profiles,
            Program::Cargo,
            &[
                "clippy",
                "--locked",
                "--workspace",
                "--manifest-path",
                manifest,
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
            "Isolated proof lock and lint policy, not crash recovery.",
        ));
        let mut proof = check(
            test_id,
            "T3",
            test_profiles,
            Program::Cargo,
            &[
                "test",
                "--locked",
                "--workspace",
                "--manifest-path",
                manifest,
                "--all-targets",
            ],
            "Self-provisioned synthetic M08 proof; cold cache needs network; Temporal/Flawless fetch digest-checked binaries, Flawless provisions Wasm; no model calls.",
        );
        if matches!(profile, "m08-event-spine-proof" | "m08-dbos-proof") {
            proof.prerequisites = &[Requirement::Executable("docker")];
        }
        let expected = match profile {
            "m08-event-spine-proof" => {
                vec!["m08_event_spine_alone_does_not_resume_post_ack_step_b"]
            }
            "m08-durable-execution" => vec![
                "completed_step_is_not_reexecuted_after_worker_process_crash",
                "cancellation_of_in_flight_b_is_terminal_without_completing_b",
            ],
            "m08-dbos-proof" => vec![
                "completed_step_is_not_reexecuted_after_worker_process_exit",
                "application_row_and_dbos_enqueue_commit_or_rollback_together",
            ],
            "m08-temporal-proof" => {
                vec![
                    "worker_crash_during_model_step_resumes_without_reexecuting_preparation",
                    "server::tests::shutdown_reaps_unwaited_and_completed_children",
                ]
            }
            _ => vec!["tests::recorded_effects_resume_after_server_process_crash"],
        };
        proof.command.args.push("--");
        let mut args = proof.command.args.clone();
        args.push("--list");
        proof.discovery = Some(Discovery {
            args,
            expected,
            ignored: false,
        });
        checks.push(proof);
    }
    checks
}

pub(super) fn validate(checks: &[Check]) -> Result<(), String> {
    let mut ids = std::collections::BTreeSet::new();
    for check in checks {
        if check.id.is_empty()
            || !ids.insert(check.id)
            || check.command.args.is_empty()
            || check.evidence.is_empty()
        {
            return Err(format!("invalid or duplicate check: {}", check.id));
        }
        if check.profiles.is_empty() || check.profiles.iter().any(|p| !PROFILES.contains(p)) {
            return Err(format!("unknown or absent profile for {}", check.id));
        }
        if check.profiles.contains(&"verify")
            && (check.tier == "T3" || !check.prerequisites.is_empty())
        {
            return Err(format!(
                "default acceptance must be service/credential-free: {}",
                check.id
            ));
        }
        if !matches!(check.tier, "T1" | "T2" | "T3") {
            return Err(format!("unknown tier for {}", check.id));
        }
        if let Some(discovery) = &check.discovery {
            if discovery.expected.is_empty()
                || discovery
                    .expected
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != discovery.expected.len()
                || discovery
                    .args
                    .iter()
                    .filter(|argument| **argument == "--list")
                    .count()
                    != 1
                || discovery.args.last() != Some(&"--list")
                || !discovery.args.contains(&"--")
            {
                return Err(format!("invalid discovery contract for {}", check.id));
            }
            if discovery.ignored != discovery.args.contains(&"--ignored")
                || discovery.args.contains(&"--exact")
            {
                return Err(format!("invalid discovery scope for {}", check.id));
            }
            if check.id == "ignored-inventory" {
                if check.command.program != Program::Cargo || check.command.args != discovery.args {
                    return Err(
                        "ignored census must execute its exact discovery contract".to_owned()
                    );
                }
            } else {
                if check.command.args.contains(&"--list") {
                    return Err(format!("list-only execution cannot prove {}", check.id));
                }
                let mut expected_command = discovery.args.clone();
                expected_command.retain(|argument| *argument != "--list");
                if let Some(exact) = check.command.args.iter().position(|a| *a == "--exact") {
                    if exact + 2 != check.command.args.len()
                        || !discovery.expected.contains(&check.command.args[exact + 1])
                    {
                        return Err(format!(
                            "exact filter lacks protected identity for {}",
                            check.id
                        ));
                    }
                    expected_command.extend(["--exact", check.command.args[exact + 1]]);
                }
                if expected_command != check.command.args || check.command.program != Program::Cargo
                {
                    return Err(format!(
                        "discovery and execution argv differ for {}",
                        check.id
                    ));
                }
            }
        }
    }
    for profile in PROFILES {
        if !checks.iter().any(|c| c.profiles.contains(profile)) {
            return Err(format!("empty profile: {profile}"));
        }
    }
    Ok(())
}
