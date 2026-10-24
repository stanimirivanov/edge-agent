//! One ordered inventory for local and CI acceptance; selectors cannot edit argv.

use super::{Check, CommandSpec, Discovery, Program, Requirement, proofs};

pub(super) const PROFILES: &[&str] = &[
    "verify",
    "offline-ingestion",
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
            "option-analytics-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-simulation-domain",
                "--test",
                "analytics",
                "--",
            ],
            "Independent numerical references, finite differences, point-in-time boundaries and bounded IV convergence; not provider or policy qualification.",
        ),
        check(
            "account-audit-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-audit-domain",
                "--test",
                "replay",
                "--",
            ],
            "Bounded owner-export replay, checkpoint/backfill equivalence, duplicates/conflicts, financial reconciliation and scope; not public deployment or risk authority.",
        ),
        check(
            "frozen-evaluation-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-audit-domain",
                "--test",
                "evaluation",
                "--",
            ],
            "Synthetic frozen-run replay and one-factor fee/latency/liquidity sensitivity; no empirical market or statistical performance claim.",
        ),
        check(
            "offline-worker-contracts",
            "T2",
            &["verify", "test", "offline-ingestion"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data",
                "--bin",
                "edgeagent-market-data",
                "--",
            ],
            "Offline-only composition configuration, pinned replay and non-inferred cleanup authority; process recovery is separate.",
        ),
        check(
            "offline-source-isolation",
            "T2",
            &["verify", "test", "offline-ingestion"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-replay",
                "--test",
                "offline",
                "--",
            ],
            "Network-free invented daily source, ambient credential isolation and P05 prefix replay; OS network denial is a separate Linux CI proof.",
        ),
        check(
            "instrument-selection-routing",
            "T2",
            &["verify", "test", "offline-ingestion"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-replay",
                "--test",
                "selection",
                "--",
            ],
            "Invented header-first routing, immutable selection/directory history, narrowed requests and fail-closed control/sequence/framing proofs; not native feed qualification.",
        ),
        check(
            "offline-source-network-denial",
            "T3",
            &["offline-ingestion"],
            Python,
            &["scripts/verify_offline_source.py"],
            "Linux fresh network namespace executes the complete synthetic source suite without external routes; requires sudo/unshare and a populated Cargo cache.",
        ),
        check(
            "market-data-quality",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-domain",
                "--test",
                "daily",
                "--",
            ],
            "Bounded point-in-time daily-series quality against explicit calendars, buckets and action coverage; no acquisition or license authority.",
        ),
        check(
            "market-data-conformance",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-application",
                "--test",
                "contracts",
                "--",
            ],
            "Synthetic acquisition admission, reviewed rights/capabilities and response conformance; no live qualification or legal authority.",
        ),
        check(
            "acquisition-terminal-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-application",
                "--test",
                "terminal",
                "--features",
                "serde_json/preserve_order",
                "--",
            ],
            "Receipt-first recovery, uncertain confirmation and authority/storage prerequisites; model proofs, not SQL atomicity.",
        ),
        check(
            "acquisition-budget-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-application",
                "--test",
                "admission",
                "--features",
                "serde_json/preserve_order",
                "--",
            ],
            "Canonical admission and accounting: original deadlines, conservative uncertainty, replay and account rate fences. Synthetic model, not SQL atomicity.",
        ),
        check(
            "evidence-retention-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-application",
                "--test",
                "retention",
                "--",
            ],
            "Durable denial surrounds I/O; cleanup requires explicit write quiescence and preserves inventory after uncertain deletion. Synthetic model, not physical media erasure.",
        ),
        check(
            "evidence-storage-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-object-store",
                "--lib",
                "--",
            ],
            "Synthetic create-only storage, uncertain-write recovery, scoped authority, bounded integrity reads, local reopen and scoped idempotent deletion; not cloud or media erasure.",
        ),
        check(
            "provider-http-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-http",
                "--lib",
                "--features",
                "serde_json/preserve_order,compression-unification-test",
                "--",
                "--skip",
                "tests::metered::",
            ],
            "Loopback-only exact JSON and bounded HTTP authority, streaming, cancellation, deadline, status and TLS-failure proofs; not vendor qualification.",
        ),
        check(
            "metered-provider-dispatch",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-http",
                "--lib",
                "tests::metered::",
                "--features",
                "serde_json/preserve_order,compression-unification-test",
                "--",
            ],
            "Loopback HTTP and committed-state model prove permit-before-I/O, conservative uncertainty, idempotent page confirmation and safe usage; not process/PostgreSQL qualification.",
        ),
        check(
            "evidence-contracts",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-event-spine-conformance",
                "--test",
                "evidence_contracts",
                "--features",
                "serde_json/preserve_order",
                "--",
            ],
            "Immutable evidence schemas, canonical bytes and scoped terminal contracts, including preserved JSON object order.",
        ),
        check(
            "evidence-freezing",
            "T2",
            &["verify", "test"],
            Cargo,
            &[
                "test",
                "--locked",
                "-p",
                "edgeagent-market-data-application",
                "--test",
                "evidence",
                "--",
            ],
            "Checked authority and whole-series quality bridge to immutable scoped evidence; no I/O or publication.",
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
            "offline-source-network-denial" => {
                entry.prerequisites = &[
                    Requirement::Executable("unshare"),
                    Requirement::Executable("sudo"),
                ];
            }
            "account-audit-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "complete_rebuild_checkpoint_resume_and_reordered_backfill_agree",
                        "duplicates_conflicts_and_foreign_scope_never_mutate_a_view",
                        "financial_drift_and_revision_regression_fail_atomically",
                        "capacity_corruption_truncation_and_unknown_versions_fail_closed",
                        "invalid_wire_state_ordering_reservations_and_nullable_presence_are_rejected",
                        "immutable_export_identity_and_payload_safe_diagnostics_are_stable",
                    ],
                    ignored: false,
                });
            }
            "offline-source-isolation" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "offline_mode_is_default_and_live_or_malformed_flags_fail_closed",
                        "daily_source_returns_meaningful_repeatable_canonical_batches",
                        "replay_restores_every_prefix_without_repeating_ids_or_random_draws",
                        "fixture_bytes_scope_bounds_and_provenance_are_checked_before_replay",
                        "cancellation_and_foreign_admission_leave_the_checkpoint_unchanged",
                        "ambient_credentials_cannot_activate_live_access",
                    ],
                    ignored: false,
                });
            }
            "instrument-selection-routing" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "latest_known_revisions_never_resurrect_obsolete_selection_or_directories",
                        "narrowed_requests_fail_before_provider_invocation_and_keep_exact_aliases",
                        "framing_and_header_selection_run_before_full_payload_decoding",
                        "controls_gaps_unknown_keys_and_stale_epochs_cannot_hide_behind_filtering",
                        "canonical_binding_tracks_content_revisions_and_order_without_diagnostic_leaks",
                        "bounds_ambiguous_mappings_and_malformed_frames_fail_closed",
                    ],
                    ignored: false,
                });
            }
            "acquisition-terminal-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "committed_recovery_skips_preparation_and_preserves_original_event_bytes",
                        "failed_or_cancelled_confirmation_recovers_without_compensation",
                        "ready_preparation_requires_matching_admission_storage_and_current_authority",
                        "ready_guards_pending_cancelled_and_expired_work_while_rejection_retains_charges",
                        "receipts_bind_exact_route_command_identity_and_payload_safe_diagnostics",
                    ],
                    ignored: false,
                });
            }
            "offline-worker-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "offline::tests::offline_mode_rejects_vendor_and_live_flags_before_infrastructure",
                        "offline::tests::compiled_profile_is_pinned_and_cancellation_cannot_activate_a_source",
                        "offline::tests::cleanup_never_infers_account_or_write_quiescence",
                        "offline::tests::profile_lock_excludes_a_second_owner_and_refuses_reinitialization",
                        "offline::tests::compiled_profile_passes_quality_and_creates_a_scoped_artifact",
                    ],
                    ignored: false,
                });
            }
            "acquisition-budget-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "admission_canonical_digest_binds_complete_inputs_and_current_authority",
                        "configuration_v2_roundtrips_without_reinterpreting_legacy_admissions",
                        "admission_decode_rejects_noncanonical_unknown_duplicate_and_tampered_records",
                        "attempt_call_page_and_byte_budgets_survive_state_recovery",
                        "account_limits_spacing_window_and_pending_permits_span_commands",
                        "cancellation_deadlines_and_uncertain_calls_do_not_refund_or_release",
                        "completion_replay_is_immutable_and_old_fences_cannot_release_new_work",
                        "daily_policy_bootstrap_and_legacy_accounts_fail_closed_without_reset",
                        "daily_credits_roll_at_utc_midnight_without_releasing_pending_or_refunding",
                        "daily_unknown_prices_expiry_clock_regression_and_overflow_deny_without_mutation",
                    ],
                    ignored: false,
                });
            }
            "evidence-retention-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "durable_denial_checks_surround_storage_and_never_release_revoked_bytes",
                        "cleanup_requires_quiescence_and_keeps_inventory_after_uncertain_deletion",
                    ],
                    ignored: false,
                });
            }
            "evidence-storage-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "store::tests::concurrent_creates_and_corrections_preserve_exact_immutable_bytes",
                        "store::tests::lost_write_completion_and_dropped_operations_recover_without_overwrite",
                        "store::tests::corrupt_collisions_descriptors_and_stream_bounds_fail_closed",
                        "store::tests::current_authority_and_exclusive_expiry_are_rechecked_after_io",
                        "store::tests::durable_local_store_reopens_and_licensed_namespaces_do_not_alias",
                        "store::tests::storage_errors_and_adapter_debug_redact_paths_hashes_and_payloads",
                        "store::tests::quiescent_deletion_is_scoped_idempotent_and_recovers_after_timeout",
                    ],
                    ignored: false,
                });
            }
            "provider-http-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "tests::exact_json_tokens_retain_precision_and_reject_ambiguous_or_excess_values",
                        "tests::endpoint_authority_credentials_and_redirects_cannot_expand_egress",
                        "tests::response_streams_and_local_attempt_budgets_fail_closed_at_boundaries",
                        "tests::cancellation_original_deadlines_and_dropped_attempts_do_not_restart_work",
                        "tests::statuses_retry_after_encodings_and_public_diagnostics_are_explicit",
                        "tests::tls_handshake_failure_never_downgrades_or_exposes_request_secrets",
                    ],
                    ignored: false,
                });
            }
            "metered-provider-dispatch" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "tests::metered::each_page_requires_confirmed_permit_and_usage_is_ledger_backed",
                        "tests::metered::lost_reservation_confirmation_and_inflight_drop_never_redispatch",
                        "tests::metered::completed_page_reconfirmation_survives_lost_or_dropped_confirmation",
                        "tests::metered::authority_cancellation_and_original_deadlines_are_rechecked_after_waits",
                        "tests::metered::partial_overflow_and_retry_after_accounting_stays_conservative",
                        "tests::metered::dispatch_diagnostics_and_usage_exclude_identity_and_payloads",
                    ],
                    ignored: false,
                });
            }
            "evidence-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "generated_evidence_artifacts_have_exact_inventory_and_immutable_baselines",
                        "canonical_evidence_bytes_preserve_exact_values_times_and_scoped_identity",
                        "malformed_units_knowledge_coverage_and_lineage_cannot_form_artifacts",
                        "bounded_canonical_decode_rejects_tampering_duplicates_and_payload_diagnostics",
                        "evidence_routes_bind_schema_owner_partition_and_reference_only_outcomes",
                        "local_schemas_reject_version_drift_unknown_fields_and_missing_explicit_nulls",
                    ],
                    ignored: false,
                });
            }
            "evidence-freezing" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "freezing_preserves_exact_quality_lineage_and_closed_calendar_coverage",
                        "corrections_link_immutable_predecessors_without_cross_scope_or_identity_reuse",
                    ],
                    ignored: false,
                });
            }
            "market-data-conformance" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "rights_permissions_scope_and_conflicts_fail_closed",
                        "latest_authority_revisions_never_resurrect_withdrawn_or_future_records",
                        "provider_capabilities_and_historical_knowledge_require_review",
                        "bounded_requests_cancellation_deadlines_and_retention_are_explicit",
                        "provider_results_cannot_expand_authority_or_bypass_provenance",
                        "synthetic_provider_port_requires_admission_and_post_call_authority",
                    ],
                    ignored: false,
                });
            }
            "market-data-quality" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "complete_explicit_calendars_preserve_dst_early_closes_and_raw_lineage",
                        "missing_closed_conflicting_and_unbounded_series_fail_without_partial_acceptance",
                        "bucket_end_grace_and_reference_freshness_have_exact_inclusive_boundaries",
                        "revisions_duplicates_and_cancellations_select_before_effective_filtering",
                        "action_coverage_and_point_in_time_reference_corrections_fail_closed",
                        "exact_outlier_and_independent_source_thresholds_do_not_adjust_or_choose_prices",
                    ],
                    ignored: false,
                });
            }
            "frozen-evaluation-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "actual_execution_and_settlement_expose_cost_latency_and_liquidity_sensitivity",
                        "repeated_and_checkpoint_resumed_paths_have_identical_canonical_reports",
                        "frozen_case_census_rejects_omitted_duplicate_and_multifactor_experiments",
                        "overlapping_holdouts_future_marks_and_invalid_continuations_fail_closed",
                        "partial_foreign_conflicting_and_open_owner_histories_produce_no_report",
                        "diagnostics_redact_financial_payloads_and_reports_preserve_declared_order",
                    ],
                    ignored: false,
                });
            }
            "option-analytics-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "independent_reference_premiums_and_greeks_cover_carry_and_intraday_expiry",
                        "analytic_greeks_satisfy_parity_and_independent_finite_differences",
                        "solver_requires_price_and_width_convergence_and_never_returns_an_exhausted_iterate",
                        "near_expiry_and_flat_time_value_cannot_fabricate_identified_volatility",
                        "nonfinite_degenerate_unsupported_and_unknown_inputs_fail_closed",
                        "retained_identities_reproduce_results_and_diagnostics_redact_inputs",
                    ],
                    ignored: false,
                });
            }
            "simulation-schema-contracts" => {
                let mut args = entry.command.args.clone();
                args.push("--list");
                entry.discovery = Some(Discovery {
                    args,
                    expected: vec![
                        "admission_wire_schema_and_constructor_reject_unsupported_modes_and_invalid_facts",
                        "published_admission_schemas_retain_immutable_content_and_identity",
                        "published_execution_schemas_retain_immutable_content_and_identity",
                        "execution_wire_schema_and_constructor_reject_mutable_financial_input",
                        "settlement_wire_contracts_validate_balanced_and_bounded_committed_facts",
                        "published_settlement_schemas_retain_immutable_content_and_identity",
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
