use std::{collections::VecDeque, io};

use super::{
    Check, CommandSpec, Discovery, Program, Requirement, inventory,
    runner::{self, Execution, Executor, Options, Status},
};

#[test]
fn evidence_proof_census_rejects_removed_renamed_duplicated_and_extra_tests() -> Result<(), String>
{
    for (id, expected) in [
        (
            "acquisition-terminal-contracts",
            vec![
                "committed_recovery_skips_preparation_and_preserves_original_event_bytes",
                "failed_or_cancelled_confirmation_recovers_without_compensation",
                "ready_preparation_requires_matching_admission_storage_and_current_authority",
                "ready_guards_pending_cancelled_and_expired_work_while_rejection_retains_charges",
                "receipts_bind_exact_route_command_identity_and_payload_safe_diagnostics",
            ],
        ),
        (
            "acquisition-budget-contracts",
            vec![
                "admission_canonical_digest_binds_complete_inputs_and_current_authority",
                "admission_decode_rejects_noncanonical_unknown_duplicate_and_tampered_records",
                "attempt_call_page_and_byte_budgets_survive_state_recovery",
                "account_limits_spacing_window_and_pending_permits_span_commands",
                "cancellation_deadlines_and_uncertain_calls_do_not_refund_or_release",
                "completion_replay_is_immutable_and_old_fences_cannot_release_new_work",
            ],
        ),
        (
            "evidence-storage-contracts",
            vec![
                "store::tests::concurrent_creates_and_corrections_preserve_exact_immutable_bytes",
                "store::tests::lost_write_completion_and_dropped_operations_recover_without_overwrite",
                "store::tests::corrupt_collisions_descriptors_and_stream_bounds_fail_closed",
                "store::tests::current_authority_and_exclusive_expiry_are_rechecked_after_io",
                "store::tests::durable_local_store_reopens_and_licensed_namespaces_do_not_alias",
                "store::tests::storage_errors_and_adapter_debug_redact_paths_hashes_and_payloads",
            ],
        ),
        (
            "provider-http-contracts",
            vec![
                "tests::exact_json_tokens_retain_precision_and_reject_ambiguous_or_excess_values",
                "tests::endpoint_authority_credentials_and_redirects_cannot_expand_egress",
                "tests::response_streams_and_local_attempt_budgets_fail_closed_at_boundaries",
                "tests::cancellation_original_deadlines_and_dropped_attempts_do_not_restart_work",
                "tests::statuses_retry_after_encodings_and_public_diagnostics_are_explicit",
                "tests::tls_handshake_failure_never_downgrades_or_exposes_request_secrets",
            ],
        ),
        (
            "evidence-contracts",
            vec![
                "generated_evidence_artifacts_have_exact_inventory_and_immutable_baselines",
                "canonical_evidence_bytes_preserve_exact_values_times_and_scoped_identity",
                "malformed_units_knowledge_coverage_and_lineage_cannot_form_artifacts",
                "bounded_canonical_decode_rejects_tampering_duplicates_and_payload_diagnostics",
                "evidence_routes_bind_schema_owner_partition_and_reference_only_outcomes",
                "local_schemas_reject_version_drift_unknown_fields_and_missing_explicit_nulls",
            ],
        ),
        (
            "evidence-freezing",
            vec![
                "freezing_preserves_exact_quality_lineage_and_closed_calendar_coverage",
                "corrections_link_immutable_predecessors_without_cross_scope_or_identity_reuse",
            ],
        ),
    ] {
        let check = inventory::checks()
            .into_iter()
            .find(|c| c.id == id)
            .ok_or("evidence gate missing")?;
        assert!(check.profiles.contains(&"verify") && check.profiles.contains(&"test"));
        let census = check.discovery.ok_or("evidence census missing")?;
        assert_eq!(census.expected, expected);
        let complete = expected
            .iter()
            .map(|n| format!("{n}: test\n"))
            .collect::<String>();
        runner::verify_discovery(&census, complete.as_bytes())?;
        for name in expected {
            for changed in [
                complete.replace(&format!("{name}: test\n"), ""),
                complete.replace(name, "renamed_evidence_proof"),
                format!("{complete}{name}: test\n"),
                format!("{complete}extra_evidence_proof: test\n"),
            ] {
                assert!(runner::verify_discovery(&census, changed.as_bytes()).is_err());
            }
        }
    }
    Ok(())
}

#[test]
fn analytics_census_rejects_every_removed_renamed_duplicated_and_extra_proof() -> Result<(), String>
{
    let check = inventory::checks()
        .into_iter()
        .find(|c| c.id == "option-analytics-contracts")
        .ok_or("analytics gate missing")?;
    assert!(check.profiles.contains(&"verify") && check.profiles.contains(&"test"));
    let census = check.discovery.ok_or("analytics discovery missing")?;
    assert_eq!(census.expected.len(), 6);
    let complete = census
        .expected
        .iter()
        .map(|name| format!("{name}: test\n"))
        .collect::<String>();
    runner::verify_discovery(&census, complete.as_bytes())?;
    for name in &census.expected {
        for mutation in [
            complete.replace(&format!("{name}: test\n"), ""),
            complete.replace(name, "renamed_numerical_proof"),
            format!("{complete}{name}: test\n"),
            format!("{complete}unregistered_numerical_proof: test\n"),
        ] {
            assert!(runner::verify_discovery(&census, mutation.as_bytes()).is_err());
        }
    }
    Ok(())
}

#[test]
fn audit_and_evaluation_census_reject_removed_renamed_duplicated_and_extra_proofs()
-> Result<(), String> {
    for id in [
        "account-audit-contracts",
        "frozen-evaluation-contracts",
        "market-data-quality",
        "market-data-conformance",
    ] {
        let check = inventory::checks()
            .into_iter()
            .find(|c| c.id == id)
            .ok_or("audit gate missing")?;
        assert!(check.profiles.contains(&"verify") && check.profiles.contains(&"test"));
        let census = check.discovery.ok_or("audit discovery missing")?;
        assert_eq!(census.expected.len(), 6);
        if id == "market-data-conformance" {
            assert_eq!(
                census.expected,
                [
                    "rights_permissions_scope_and_conflicts_fail_closed",
                    "latest_authority_revisions_never_resurrect_withdrawn_or_future_records",
                    "provider_capabilities_and_historical_knowledge_require_review",
                    "bounded_requests_cancellation_deadlines_and_retention_are_explicit",
                    "provider_results_cannot_expand_authority_or_bypass_provenance",
                    "synthetic_provider_port_requires_admission_and_post_call_authority",
                ]
            );
        }
        if id == "market-data-quality" {
            assert_eq!(
                census.expected,
                [
                    "complete_explicit_calendars_preserve_dst_early_closes_and_raw_lineage",
                    "missing_closed_conflicting_and_unbounded_series_fail_without_partial_acceptance",
                    "bucket_end_grace_and_reference_freshness_have_exact_inclusive_boundaries",
                    "revisions_duplicates_and_cancellations_select_before_effective_filtering",
                    "action_coverage_and_point_in_time_reference_corrections_fail_closed",
                    "exact_outlier_and_independent_source_thresholds_do_not_adjust_or_choose_prices",
                ]
            );
        }
        let complete = census
            .expected
            .iter()
            .map(|name| format!("{name}: test\n"))
            .collect::<String>();
        runner::verify_discovery(&census, complete.as_bytes())?;
        for name in &census.expected {
            for changed in [
                complete.replace(&format!("{name}: test\n"), ""),
                complete.replace(name, "renamed_audit_proof"),
                format!("{complete}{name}: test\n"),
                format!("{complete}extra_audit_proof: test\n"),
            ] {
                assert!(runner::verify_discovery(&census, changed.as_bytes()).is_err());
            }
        }
    }
    Ok(())
}

#[test]
fn executable_choices_fail_closed_for_invalid_explicit_overrides()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        runner::resolve_executable_choice("docker", "EDGEAGENT_NATS_SERVER", None)?,
        std::ffi::OsString::from("docker")
    );
    for invalid in ["", "relative.exe", "private-sentinel-missing"] {
        let error = runner::resolve_executable_choice(
            "docker",
            "EDGEAGENT_NATS_SERVER",
            Some(invalid.into()),
        )
        .err()
        .ok_or("invalid override accepted")?;
        assert!(!error.contains("private-sentinel-missing"));
    }
    let current = std::env::current_exe()?.into_os_string();
    assert_eq!(
        runner::resolve_executable_choice(
            "docker",
            "EDGEAGENT_NATS_SERVER",
            Some(current.clone())
        )?,
        current
    );
    Ok(())
}

fn fixture() -> Check {
    Check {
        id: "fixture",
        tier: "T3",
        profiles: &["local-platform"],
        command: CommandSpec {
            program: Program::Cargo,
            args: vec![
                "test",
                "--test",
                "proof",
                "--",
                "--ignored",
                "--exact",
                "required_test",
            ],
        },
        prerequisites: &[],
        discovery: Some(Discovery {
            args: vec!["test", "--test", "proof", "--", "--ignored", "--list"],
            expected: vec!["required_test"],
            ignored: true,
        }),
        evidence: "fixture only",
    }
}

#[test]
fn relay_security_regression_discovery_rejects_missing_renamed_and_extra_tests()
-> Result<(), String> {
    let checks = inventory::checks();
    let check = checks
        .iter()
        .find(|check| check.id == "relay-observation-contracts")
        .ok_or("protected relay observation check missing")?;
    assert!(check.profiles.contains(&"verify"));
    let discovery = check.discovery.as_ref().ok_or("relay discovery missing")?;
    assert_eq!(
        discovery.expected,
        ["relay_observations_respect_validation_and_confirmed_outcomes"]
    );
    runner::verify_discovery(
        discovery,
        b"relay_observations_respect_validation_and_confirmed_outcomes: test\n",
    )?;
    for stdout in [
        "".as_bytes(),
        b"renamed_security_regression: test\n",
        b"relay_observations_respect_validation_and_confirmed_outcomes: test\nunregistered: test\n",
        b"relay_observations_respect_validation_and_confirmed_outcomes: test\nrelay_observations_respect_validation_and_confirmed_outcomes: test\n",
    ] {
        assert!(runner::verify_discovery(discovery, stdout).is_err());
    }
    Ok(())
}

#[test]
fn relay_policy_discovery_protects_each_public_contract_identity() -> Result<(), String> {
    let checks = inventory::checks();
    let check = checks
        .iter()
        .find(|check| check.id == "relay-policy-contracts")
        .ok_or("policy check missing")?;
    assert_eq!(check.profiles, ["verify", "test"]);
    assert!(check.prerequisites.is_empty());
    assert!(check.command.args.contains(&"policy_contracts"));
    let discovery = check.discovery.as_ref().ok_or("policy discovery missing")?;
    assert_eq!(discovery.expected.len(), 5);
    let complete = discovery
        .expected
        .iter()
        .map(|identity| format!("{identity}: test\n"))
        .collect::<String>();
    runner::verify_discovery(discovery, complete.as_bytes())?;
    for identity in &discovery.expected {
        for mutation in [
            complete.replace(&format!("{identity}: test\n"), ""),
            complete.replace(identity, "renamed_policy_contract"),
            format!("{complete}{identity}: test\n"),
            format!("{complete}unregistered_policy_contract: test\n"),
        ] {
            assert!(runner::verify_discovery(discovery, mutation.as_bytes()).is_err());
        }
    }
    Ok(())
}

#[test]
fn message_schema_discovery_protects_every_acceptance_contract() -> Result<(), String> {
    let checks = inventory::checks();
    let check = checks
        .iter()
        .find(|check| check.id == "message-schema-contracts")
        .ok_or("schema check missing")?;
    assert_eq!(check.profiles, ["verify", "test"]);
    assert!(check.prerequisites.is_empty());
    assert!(check.command.args.contains(&"schema_contracts"));
    assert!(check.command.args.contains(&"serde_json/preserve_order"));
    let discovery = check.discovery.as_ref().ok_or("schema discovery missing")?;
    assert_eq!(discovery.expected.len(), 10);
    let complete = discovery
        .expected
        .iter()
        .map(|name| format!("{name}: test\n"))
        .collect::<String>();
    runner::verify_discovery(discovery, complete.as_bytes())?;
    for name in &discovery.expected {
        for mutation in [
            complete.replace(&format!("{name}: test\n"), ""),
            complete.replace(name, "renamed_schema_contract"),
            format!("{complete}{name}: test\n"),
            format!("{complete}unregistered_schema_contract: test\n"),
        ] {
            assert!(runner::verify_discovery(discovery, mutation.as_bytes()).is_err());
        }
    }
    Ok(())
}

struct Fake {
    commands: Vec<(Vec<&'static str>, bool)>,
    outputs: VecDeque<io::Result<Execution>>,
    prerequisite: Result<(), String>,
}

impl Fake {
    fn new(outputs: Vec<io::Result<Execution>>) -> Self {
        Self {
            commands: Vec::new(),
            outputs: outputs.into(),
            prerequisite: Ok(()),
        }
    }
}

impl Executor for Fake {
    fn prerequisite(&mut self, _: Requirement) -> Result<(), String> {
        self.prerequisite.clone()
    }
    fn execute(&mut self, command: &CommandSpec, capture: bool) -> io::Result<Execution> {
        self.commands.push((command.args.clone(), capture));
        self.outputs
            .pop_front()
            .ok_or_else(|| io::Error::other("unexpected execution"))?
    }
}

fn output(success: bool, stdout: &str) -> io::Result<Execution> {
    Ok(Execution {
        success,
        stdout: stdout.as_bytes().to_vec(),
    })
}

#[test]
fn inventory_and_default_acceptance_are_complete_and_credential_free() -> Result<(), String> {
    let checks = inventory::checks();
    inventory::validate(&checks)?;
    let options = Options::parse(&[])?;
    let selected = options.select(&checks)?;
    assert_eq!(
        selected.iter().map(|c| c.id).collect::<Vec<_>>(),
        [
            "text-format",
            "rust-format",
            "repository-bytes",
            "repository-policy",
            "architecture-declarations",
            "workflow-security",
            "secret-policy",
            "architecture-graph",
            "image-contracts",
            "local-stack-contracts",
            "supply-chain-contracts",
            "release-contracts",
            "locked-graph",
            "rust-check",
            "rust-clippy",
            "sqlx-query-policy",
            "harness-tests",
            "ignored-inventory",
            "rust-tests",
            "rust-doctests",
            "option-analytics-contracts",
            "account-audit-contracts",
            "frozen-evaluation-contracts",
            "market-data-quality",
            "market-data-conformance",
            "acquisition-terminal-contracts",
            "acquisition-budget-contracts",
            "evidence-storage-contracts",
            "provider-http-contracts",
            "evidence-contracts",
            "evidence-freezing",
            "messaging-error-contracts",
            "relay-observation-contracts",
            "relay-policy-contracts",
            "contracts-preserve-order",
            "message-schema-contracts",
            "simulation-schema-contracts",
        ]
    );
    assert!(
        selected
            .iter()
            .all(|c| c.tier != "T3" && c.prerequisites.is_empty())
    );
    assert!(selected.iter().any(|c| c.command.args.contains(&"--doc")));
    assert!(
        selected
            .iter()
            .any(|c| c.command.args.contains(&"serde_json/preserve_order"))
    );
    Ok(())
}

#[test]
fn dependency_audit_covers_each_isolated_lock_without_runtime_or_sbom_work() -> Result<(), String> {
    let checks = inventory::checks();
    let options = Options::parse(&["dependency-audit".to_owned()])?;
    let selected = options.select(&checks)?;
    assert_eq!(
        selected.iter().map(|check| check.id).collect::<Vec<_>>(),
        [
            "workflow-security",
            "supply-chain-contracts",
            "dependency-policy",
            "m08-event-spine-dependency-policy",
            "m08-duroxide-dependency-policy",
            "m08-dbos-dependency-policy",
            "m08-temporal-dependency-policy",
            "m08-flawless-dependency-policy",
        ]
    );
    let audits: Vec<_> = selected
        .into_iter()
        .filter(|check| check.id.ends_with("dependency-policy"))
        .collect();
    assert_eq!(audits.len(), 6);
    for audit in audits {
        assert_eq!(audit.command.program, Program::Cargo);
        assert_eq!(audit.command.args.first(), Some(&"deny"));
        assert!(audit.command.args.contains(&"--locked"));
        assert!(matches!(
            audit.prerequisites,
            [Requirement::Executable("cargo-deny")]
        ));
        assert!(audit.discovery.is_none());
        if audit.id != "dependency-policy" {
            assert!(audit.command.args.contains(&"--workspace"));
            assert!(audit.command.args.contains(&"experiments/deny.toml"));
            assert!(audit.command.args.contains(&"--manifest-path"));
        }
    }
    Ok(())
}

#[test]
fn security_profile_requires_actual_scanner_protocol_and_default_does_not_install_it()
-> Result<(), String> {
    let checks = inventory::checks();
    let options = Options::parse(&["ci-security".to_owned()])?;
    let selected = options.select(&checks)?;
    assert_eq!(
        selected.iter().map(|check| check.id).collect::<Vec<_>>(),
        [
            "workflow-security",
            "secret-policy",
            "secret-scanner-contracts",
            "secret-scan"
        ]
    );
    for check in selected.iter().filter(|check| check.tier == "T3") {
        assert!(matches!(
            check.prerequisites,
            [Requirement::CommandProbe("gitleaks", ["version"])]
        ));
        assert_eq!(check.command.program, Program::Python);
        assert_eq!(
            check.command.args.first(),
            Some(&"scripts/verify_secrets.py")
        );
    }
    Ok(())
}

#[test]
fn unknown_empty_ambiguous_and_duplicate_selections_fail_closed() -> Result<(), String> {
    let checks = inventory::checks();
    for args in [
        vec!["unknown"],
        vec!["--python"],
        vec!["--check", ""],
        vec!["verify", "--check", "rust-tests"],
        vec!["--other"],
    ] {
        assert!(Options::parse(&args.into_iter().map(str::to_owned).collect::<Vec<_>>()).is_err());
    }
    for args in [
        vec!["--check", "renamed-test"],
        vec!["--check", "rust-tests", "--check", "rust-tests"],
    ] {
        assert!(
            Options::parse(&args.into_iter().map(str::to_owned).collect::<Vec<_>>())?
                .select(&checks)
                .is_err()
        );
    }
    Ok(())
}

#[test]
fn libtest_census_rejects_zero_renamed_added_duplicate_and_malformed_output() -> Result<(), String>
{
    let discovery = fixture().discovery.ok_or("fixture missing discovery")?;
    runner::verify_discovery(
        &discovery,
        b"required_test: test\r\n\r\n1 test, 0 benchmarks\r\n",
    )?;
    for stdout in [
        b"0 tests, 0 benchmarks".as_slice(),
        b"renamed_test: test",
        b"required_test: test\nnew_ignored: test",
        b"required_test: test\nrequired_test: test",
        b"required_test: benchmark",
        b"required_test: test\xff",
    ] {
        assert!(runner::verify_discovery(&discovery, stdout).is_err());
    }
    let not_ignored = Discovery {
        ignored: false,
        ..discovery
    };
    assert!(runner::verify_discovery(&not_ignored, b"required_test: test").is_err());
    Ok(())
}

#[test]
fn successful_discovery_precedes_actual_proof_execution() {
    let check = fixture();
    let mut executor = Fake::new(vec![
        output(true, "required_test: test\n"),
        output(true, ""),
    ]);
    let outcomes = runner::execute(&[&check], &mut executor);
    assert_eq!(outcomes[0].status, Status::Passed);
    assert_eq!(executor.commands.len(), 2);
    assert!(executor.commands[0].1);
    assert!(!executor.commands[1].1);
    assert!(executor.commands[0].0.contains(&"--list"));
    assert!(executor.commands[1].0.contains(&"--exact"));
}

#[test]
fn missing_prerequisite_is_not_run_and_blocks_later_effects() {
    let mut check = fixture();
    check.prerequisites = &[Requirement::Environment("SYNTHETIC_SERVICE_URL")];
    let next = fixture();
    let mut executor = Fake::new(Vec::new());
    executor.prerequisite = Err("requires SYNTHETIC_SERVICE_URL".to_owned());
    let outcomes = runner::execute(&[&check, &next], &mut executor);
    assert_eq!(
        outcomes[0].status,
        Status::NotRun("requires SYNTHETIC_SERVICE_URL".to_owned())
    );
    assert!(matches!(outcomes[1].status, Status::NotRun(_)));
    assert!(executor.commands.is_empty());
}

#[test]
fn discovery_failure_cannot_launch_proof_or_later_check() {
    for response in [
        output(true, "0 tests"),
        output(false, "required_test: test"),
        Err(io::Error::new(io::ErrorKind::NotFound, "no cargo")),
    ] {
        let check = fixture();
        let next = fixture();
        let mut executor = Fake::new(vec![response]);
        let outcomes = runner::execute(&[&check, &next], &mut executor);
        assert_ne!(outcomes[0].status, Status::Passed);
        assert!(matches!(outcomes[1].status, Status::NotRun(_)));
        assert_eq!(executor.commands.len(), 1);
    }
}

#[test]
fn actual_command_failure_and_spawn_failure_never_pass() {
    for response in [
        output(false, ""),
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "missing executable",
        )),
    ] {
        let mut check = fixture();
        check.discovery = None;
        let mut executor = Fake::new(vec![response]);
        let outcomes = runner::execute(&[&check], &mut executor);
        assert_ne!(outcomes[0].status, Status::Passed);
    }
}

#[test]
fn census_runs_once_and_claims_only_discovery() {
    let mut check = fixture();
    check.id = "ignored-inventory";
    let mut executor = Fake::new(vec![output(true, "required_test: test")]);
    let outcomes = runner::execute(&[&check], &mut executor);
    assert_eq!(outcomes[0].status, Status::Passed);
    assert_eq!(executor.commands.len(), 1);
}

#[test]
fn malformed_inventory_is_rejected() {
    let checks = inventory::checks();
    let mut duplicate = checks.clone();
    duplicate.push(checks[0].clone());
    assert!(inventory::validate(&duplicate).is_err());
    let mut empty_discovery = checks.clone();
    empty_discovery[0].discovery = Some(Discovery {
        args: vec!["test", "--list"],
        expected: Vec::new(),
        ignored: false,
    });
    assert!(inventory::validate(&empty_discovery).is_err());
}

#[test]
fn selector_target_and_list_only_mutations_cannot_borrow_a_valid_census() -> Result<(), String> {
    let checks = inventory::checks();
    let index = checks
        .iter()
        .position(|check| check.id == "nats-publish")
        .ok_or("proof missing")?;
    let mut typo = checks.clone();
    let argument = typo[index]
        .command
        .args
        .last_mut()
        .ok_or("filter missing")?;
    *argument = "renamed-test-selects-zero";
    assert!(inventory::validate(&typo).is_err());
    let mut target = checks.clone();
    let target_index = target[index]
        .command
        .args
        .iter()
        .position(|arg| *arg == "jetstream_publish")
        .ok_or("target missing")?;
    target[index].command.args[target_index] = "different_target";
    assert!(inventory::validate(&target).is_err());
    let mut list_only = checks.clone();
    list_only[index].command.args = list_only[index]
        .discovery
        .as_ref()
        .ok_or("discovery missing")?
        .args
        .clone();
    assert!(inventory::validate(&list_only).is_err());
    let mut duplicate_list = checks.clone();
    duplicate_list[index]
        .discovery
        .as_mut()
        .ok_or("discovery missing")?
        .args
        .push("--list");
    assert!(inventory::validate(&duplicate_list).is_err());
    Ok(())
}

#[test]
fn default_service_dependency_and_census_command_drift_fail_closed() -> Result<(), String> {
    let checks = inventory::checks();
    let mut service_default = checks.clone();
    service_default[0].tier = "T3";
    assert!(inventory::validate(&service_default).is_err());
    service_default[0].tier = "T2";
    service_default[0].prerequisites = &[Requirement::Environment("PRIVATE_DATABASE_URL")];
    assert!(inventory::validate(&service_default).is_err());
    let mut census = checks;
    let index = census
        .iter()
        .position(|c| c.id == "ignored-inventory")
        .ok_or("census missing")?;
    census[index].command.program = Program::Python;
    assert!(inventory::validate(&census).is_err());
    Ok(())
}

#[test]
fn overrides_are_single_argv_values_and_ordinary_children_force_offline_sqlx()
-> Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse(&[
        "--python".to_owned(),
        "C:/Python Tools/python.exe".to_owned(),
        "--sbom-output".to_owned(),
        "artifacts/custom path".to_owned(),
    ])?;
    let command = CommandSpec {
        program: Program::Python,
        args: vec![
            "scripts/generate_sboms.py",
            "rust",
            "--output",
            "artifacts/sbom",
        ],
    };
    let child = runner::subprocess(&command, &options)?;
    assert_eq!(child.get_program(), "C:/Python Tools/python.exe");
    assert_eq!(
        child.get_args().collect::<Vec<_>>(),
        [
            "scripts/generate_sboms.py",
            "rust",
            "--output",
            "artifacts/custom path"
        ]
        .map(std::ffi::OsStr::new)
    );
    assert!(
        child.get_envs().any(
            |(key, value)| key == "SQLX_OFFLINE" && value == Some(std::ffi::OsStr::new("true"))
        )
    );
    let metadata = CommandSpec {
        program: Program::Xtask,
        args: vec!["sqlx-metadata"],
    };
    let child = runner::subprocess(&metadata, &options)?;
    assert!(!child.get_envs().any(|(key, _)| key == "SQLX_OFFLINE"));
    Ok(())
}
