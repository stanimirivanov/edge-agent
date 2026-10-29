use std::{collections::VecDeque, io};

#[test]
fn shared_http_census_covers_specialized_library_proofs() -> Result<(), String> {
    let checks = inventory::checks();
    let shared = checks
        .iter()
        .find(|c| c.id == "provider-http-contracts")
        .and_then(|c| c.discovery.as_ref())
        .ok_or("shared HTTP census missing")?;
    let covers =
        |broad: &[&str], specialized: &[&str]| specialized.iter().all(|name| broad.contains(name));
    for id in [
        "wss-transport-contracts",
        "alpaca-iex-conformance",
        "iex-canonical-acceptance",
    ] {
        let proof = checks
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.discovery.as_ref())
            .ok_or_else(|| format!("specialized HTTP census missing: {id}"))?;
        assert!(
            covers(&shared.expected, &proof.expected),
            "shared HTTP census omits {id}"
        );
        for identity in &proof.expected {
            let missing = shared
                .expected
                .iter()
                .copied()
                .filter(|name| name != identity)
                .collect::<Vec<_>>();
            assert!(
                !covers(&missing, &proof.expected),
                "missing shared proof must fail coverage"
            );
        }
    }
    Ok(())
}

use super::{
    Check, CommandSpec, Discovery, Program, Requirement, inventory,
    runner::{self, Execution, Executor, Options, Status},
};

#[test]
fn flawless_census_requires_crash_and_download_proofs_without_filtering() -> Result<(), String> {
    let proof = inventory::checks()
        .into_iter()
        .find(|c| c.id == "m08-flawless-test")
        .ok_or("missing Flawless proof")?;
    assert_eq!(
        proof.command.args,
        [
            "test",
            "--locked",
            "--workspace",
            "--manifest-path",
            "experiments/flawless-proof/Cargo.toml",
            "--all-targets",
            "--",
        ]
    );
    let census = proof.discovery.ok_or("missing Flawless census")?;
    assert!(!census.ignored);
    let mut expected_args = proof.command.args;
    expected_args.push("--list");
    assert_eq!(census.args, expected_args);
    // Independent literals protect the original crash proof as well as every new
    // provisioning regression; the execution command must remain the whole suite.
    assert_eq!(
        census.expected,
        [
            "tests::recorded_effects_resume_after_server_process_crash",
            "tests::download::tests::exhausted_body_retries_remove_all_partial_bytes",
            "tests::download::tests::terminal_rejections_never_publish_an_executable_or_retry",
            "tests::download::tests::transient_status_retries_with_the_same_pinned_digest",
            "tests::download::tests::truncated_body_restarts_fresh_and_exposes_only_verified_binary",
        ]
    );
    let complete = census
        .expected
        .iter()
        .map(|n| format!("{n}: test\n"))
        .collect::<String>();
    runner::verify_discovery(&census, complete.as_bytes())?;
    for name in &census.expected {
        for changed in [
            complete.replace(&format!("{name}: test\n"), ""),
            complete.replace(name, &format!("{name}_renamed")),
            format!("{complete}{name}: test\n"),
            format!("{complete}unreviewed: test\n"),
        ] {
            assert!(runner::verify_discovery(&census, changed.as_bytes()).is_err());
        }
    }
    Ok(())
}

#[test]
fn offline_stream_runtime_census_cannot_silently_shrink() -> Result<(), String> {
    let proof = inventory::checks()
        .into_iter()
        .find(|c| c.id == "offline-stream-runtime")
        .ok_or("missing runtime gate")?;
    let census = proof.discovery.ok_or("missing runtime census")?;
    assert_eq!(
        census.expected,
        vec![
            "offline_runtime_requires_approval_and_publishes_exact_ready_evidence",
            "offline_runtime_isolates_slow_sources_with_bounded_fair_dispatch",
            "offline_runtime_restart_recovers_exact_native_intent_and_fences_old_workers",
            "failures::offline_runtime_sticky_failures_and_revocation_never_grant_readiness",
        ]
    );
    assert!(census.ignored);
    let complete = census
        .expected
        .iter()
        .map(|n| format!("{n}: test\n"))
        .collect::<String>();
    runner::verify_discovery(&census, complete.as_bytes())?;
    for name in &census.expected {
        for changed in [
            complete.replace(&format!("{name}: test\n"), ""),
            complete.replace(name, &format!("{name}_renamed")),
            format!("{complete}{name}: test\n"),
            format!("{complete}unreviewed: test\n"),
        ] {
            assert!(runner::verify_discovery(&census, changed.as_bytes()).is_err());
        }
    }
    Ok(())
}

#[test]
fn evidence_proof_census_rejects_removed_renamed_duplicated_and_extra_tests() -> Result<(), String>
{
    let publication = inventory::checks()
        .into_iter()
        .find(|c| c.id == "streaming-publication-postgres")
        .ok_or("publication proof missing")?;
    let census = publication.discovery.ok_or("publication census missing")?;
    assert_eq!(
        census.expected,
        [
            "stream_preparation_and_atomic_publication_survive_result_loss_and_rollback",
            "stream_publication_fences_generations_charged_ranges_and_chain_forks",
            "stream_stop_preserves_reads_withdrawal_denies_and_cleanup_requires_writer_quiescence",
            "stream_restore_requires_independent_approval_and_preserves_sticky_incidents",
            "stream_restore_recovers_exact_pending_intents_and_approval_rolls_back",
            "inputs::stream_owner_inputs_require_independent_roles_and_exact_current_loading",
            "inputs::stream_namespace_collisions_and_changed_retries_roll_back_every_claim",
            "inputs::stream_input_retirement_erases_private_scope_without_reopening_namespaces",
            "companion::native_pair_publication_recovers_exact_receipt_and_retains_partial_cleanup_inventory",
        ]
    );
    let complete = census
        .expected
        .iter()
        .map(|n| format!("{n}: test\n"))
        .collect::<String>();
    runner::verify_discovery(&census, complete.as_bytes())?;
    for name in &census.expected {
        for changed in [
            complete.replace(&format!("{name}: test\n"), ""),
            complete.replace(name, "renamed_publication"),
            format!("{complete}{name}: test\n"),
            format!("{complete}unregistered_publication: test\n"),
        ] {
            assert!(runner::verify_discovery(&census, changed.as_bytes()).is_err());
        }
    }
    for (id, expected) in [
        (
            "iex-canonical-acceptance",
            vec![
                "iex_acceptance::tests::iex_acceptance_trade_identity_replay_and_native_provenance_are_exact",
                "iex_acceptance::tests::iex_owner_inputs_derive_exact_namespace_claims_from_the_complete_profile",
                "iex_acceptance::tests::iex_acceptance_minute_bars_preserve_raw_coverage_count_vwap_and_bounds",
                "iex_acceptance::tests::iex_acceptance_rejects_unknown_semantics_stale_references_and_authority",
                "iex_acceptance::tests::iex_acceptance_profile_bytes_bind_complete_inputs_and_reject_ambiguity",
            ],
        ),
        (
            "stream-supervision-contracts",
            vec![
                "supervision_round_robin_preserves_fifo_and_bounds_every_eligible_wait",
                "supervision_busy_and_failed_sources_cannot_starve_other_lanes",
                "supervision_overload_retains_rejected_and_unresolved_records_without_eviction",
                "supervision_foreign_and_dropped_dispatches_never_confirm_or_retry_heads",
                "supervision_cancellation_and_source_failure_are_sticky_without_compensation",
                "supervision_configuration_binding_and_diagnostics_fail_closed",
            ],
        ),
        (
            "alpaca-iex-conformance",
            vec![
                "alpaca_iex::tests::iex_protocol_requires_exact_handshake_and_subscription",
                "alpaca_iex::tests::iex_exact_trades_and_minute_bars_preserve_scope_time_and_values",
                "alpaca_iex::tests::iex_batches_reject_ambiguity_corrections_and_partial_success",
                "alpaca_iex::tests::iex_known_unselected_payload_skips_values_but_not_control_or_mapping_checks",
                "alpaca_iex::tests::iex_loopback_tls_auth_subscription_fragments_and_controls_are_charged",
                "alpaca_iex::tests::iex_unknown_accounting_revocation_and_cancellation_stop_without_reconnect",
                "alpaca_iex::tests::iex_abandoned_intake_and_absolute_handshake_deadline_are_terminal",
                "alpaca_iex::tests::iex_configuration_and_diagnostics_do_not_expand_scope_or_expose_data",
            ],
        ),
        (
            "wss-transport-contracts",
            vec![
                "wss::tests::wss_charges_controls_fragments_and_exact_caps_before_exposure",
                "wss::tests::wss_rejects_untrusted_tls_bad_hostname_and_invalid_upgrades",
                "wss::tests::wss_protocol_overflow_and_lost_charge_are_terminal",
                "wss::tests::wss_cancellation_denial_and_abandoned_receive_never_reuse_socket",
                "wss::tests::wss_guard_bounds_idle_partial_work_and_total_lifetime",
                "wss::tests::wss_configuration_and_diagnostics_fail_closed",
                "wss::tests::wss_abandoned_committed_charge_never_exposes_or_replays_payload",
                "wss::tests::wss_fragment_and_outgoing_budgets_are_not_renewed_by_progress",
                "wss::tests::wss_frame_ceiling_is_inclusive_and_silent_upgrade_times_out",
            ],
        ),
        (
            "streaming-segment-contracts",
            vec![
                "stream_segment_freezing_binds_session_scope_epoch_generation_and_retention",
                "stream_segment_codec_rejects_noncanonical_corrupt_and_unbounded_input",
                "stream_segment_values_preserve_cancellations_and_source_semantics",
                "stream_publication_recovery_preserves_exact_bytes_and_opaque_event",
                "recovery::stream_restore_replays_original_intent_and_rechecks_after_effects",
                "recovery::stream_restore_empty_cancelled_or_stale_recovery_never_opens_barrier",
                "inputs::stream_owner_inputs_bind_complete_bytes_and_reject_ambiguous_or_stale_records",
                "companion::native_companion_binds_exact_records_owner_inputs_and_legacy_recovery",
                "stream_retained_authority_outlives_capture_not_original_rights",
                "stream_deletion_requires_quiescence_and_preserves_other_namespaces",
                "stream_segment_store_is_create_only_scoped_and_reopens",
                "stream_segment_store_recovers_uncertain_put_and_denies_revocation_expiry",
                "stream_segment_store_rejects_descriptor_and_content_attacks",
                "companion::native_companion_store_verifies_uncertain_creation_denial_and_paired_cleanup",
            ],
        ),
        (
            "streaming-session-contracts",
            vec![
                "stream_admission_pins_complete_selection_and_current_authority",
                "stream_windows_limits_and_delivery_never_inherit_daily_defaults",
                "stream_frame_receipts_charge_controls_and_stop_at_exact_limits",
                "stream_overages_and_invalid_state_cannot_refund_or_continue",
                "stream_connection_fences_bind_session_account_and_preserve_start_snapshot",
            ],
        ),
        (
            "qualification-preflight",
            vec![
                "qualification_profiles_are_bounded_canonical_strict_and_payload_safe",
                "qualification_preflight_covers_both_profiles_without_enabling_or_mutating_execution",
                "qualification_preflight_requires_independent_current_owner_opt_in",
                "qualification_preflight_rejects_changed_bindings_semantics_authority_and_cancellation",
                "qualification_preflight_requires_every_current_credential_and_lifecycle_control",
                "qualification_preflight_preserves_uncertainty_and_never_resets_or_guesses_credits",
            ],
        ),
        (
            "native-publication-contracts",
            vec![
                "native_preparation_recovers_original_acceptance_after_uncertain_put",
                "native_publication_requires_authorized_predecessor_and_current_durable_denial",
            ],
        ),
        (
            "native-daily-acceptance",
            vec![
                "native_acceptance_preserves_labels_receipts_values_and_complete_provenance",
                "native_acceptance_rejects_gaps_closed_days_and_changed_capture_bindings",
                "native_acceptance_enforces_reference_freshness_actions_and_exact_outlier_thresholds",
                "native_acceptance_rechecks_authority_cancellation_and_original_admission",
                "native_acceptance_requires_explicit_revision_chains_and_same_scoped_predecessor",
                "native_artifact_decode_rejects_noncanonical_tampered_and_unaccepted_formats",
            ],
        ),
        (
            "alpaca-conformance",
            vec![
                "tests::alpaca::alpaca_raw_sip_binding_preserves_exact_provider_instants_and_receipts",
                "tests::alpaca::alpaca_pages_charge_separately_and_confirmation_loss_never_refetches",
                "tests::alpaca::alpaca_manual_rate_rejection_never_sleeps_or_consumes_another_call",
                "tests::alpaca::alpaca_malformed_foreign_duplicate_and_truncated_pages_fail_closed",
                "tests::alpaca::alpaca_credentials_authority_historical_delay_and_spent_credits_deny_before_io",
                "tests::alpaca::alpaca_errors_cancellation_and_uncertain_start_never_trigger_feed_fallback",
            ],
        ),
        (
            "twelve-data-conformance",
            vec![
                "tests::twelve_data::twelve_data_selected_request_is_raw_header_only_and_exact_native_capture_round_trips",
                "tests::twelve_data::twelve_data_foreign_null_duplicate_incomplete_and_imprecise_responses_fail_closed",
                "tests::twelve_data::twelve_data_errors_cancellation_and_authority_denial_never_retry_or_bypass_quota",
                "tests::twelve_data::twelve_data_lost_confirmation_releases_original_receipt_without_refetch",
                "tests::twelve_data::twelve_data_configuration_and_native_format_cannot_weaken_authority",
            ],
        ),
        (
            "date-only-evidence",
            vec![
                "native_dates_preserve_reviewed_dst_buckets_and_exact_local_receipts",
                "date_only_construction_rejects_unreviewed_or_conflicting_calendar_and_chronology",
                "date_only_wire_decode_is_bounded_canonical_versioned_and_payload_safe",
            ],
        ),
        (
            "offline-worker-contracts",
            vec![
                "offline::tests::offline_mode_rejects_vendor_and_live_flags_before_infrastructure",
                "offline::tests::compiled_profile_is_pinned_and_cancellation_cannot_activate_a_source",
                "offline::tests::cleanup_never_infers_account_or_write_quiescence",
                "offline::tests::profile_lock_excludes_a_second_owner_and_refuses_reinitialization",
                "offline::tests::compiled_profile_passes_quality_and_creates_a_scoped_artifact",
                "qualification::tests::both_complete_owner_inputs_round_trip_and_reject_changed_or_unknown_values",
                "qualification::tests::owner_input_errors_redact_payload_and_profile_requires_explicit_arguments",
                "qualification::tests::precise_preparation_rehydrates_original_time_and_passes_quality_again",
            ],
        ),
        (
            "offline-source-isolation",
            vec![
                "offline_mode_is_default_and_live_or_malformed_flags_fail_closed",
                "daily_source_returns_meaningful_repeatable_canonical_batches",
                "replay_restores_every_prefix_without_repeating_ids_or_random_draws",
                "fixture_bytes_scope_bounds_and_provenance_are_checked_before_replay",
                "cancellation_and_foreign_admission_leave_the_checkpoint_unchanged",
                "ambient_credentials_cannot_activate_live_access",
            ],
        ),
        (
            "instrument-selection-routing",
            vec![
                "latest_known_revisions_never_resurrect_obsolete_selection_or_directories",
                "narrowed_requests_fail_before_provider_invocation_and_keep_exact_aliases",
                "framing_and_header_selection_run_before_full_payload_decoding",
                "controls_gaps_unknown_keys_and_stale_epochs_cannot_hide_behind_filtering",
                "canonical_binding_tracks_content_revisions_and_order_without_diagnostic_leaks",
                "bounds_ambiguous_mappings_and_malformed_frames_fail_closed",
            ],
        ),
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
        ),
        (
            "evidence-retention-contracts",
            vec![
                "durable_denial_checks_surround_storage_and_never_release_revoked_bytes",
                "cleanup_requires_quiescence_and_keeps_inventory_after_uncertain_deletion",
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
                "store::tests::quiescent_deletion_is_scoped_idempotent_and_recovers_after_timeout",
                "store::tests::native::native_uncertain_create_recovers_exact_bytes_and_rejects_other_formats",
                "store::tests::native::native_reads_recheck_authority_expiry_scope_and_quiescent_cleanup",
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
                "alpaca_iex::tests::iex_protocol_requires_exact_handshake_and_subscription",
                "alpaca_iex::tests::iex_exact_trades_and_minute_bars_preserve_scope_time_and_values",
                "alpaca_iex::tests::iex_batches_reject_ambiguity_corrections_and_partial_success",
                "alpaca_iex::tests::iex_known_unselected_payload_skips_values_but_not_control_or_mapping_checks",
                "alpaca_iex::tests::iex_loopback_tls_auth_subscription_fragments_and_controls_are_charged",
                "alpaca_iex::tests::iex_unknown_accounting_revocation_and_cancellation_stop_without_reconnect",
                "alpaca_iex::tests::iex_abandoned_intake_and_absolute_handshake_deadline_are_terminal",
                "alpaca_iex::tests::iex_configuration_and_diagnostics_do_not_expand_scope_or_expose_data",
                "iex_acceptance::tests::iex_owner_inputs_derive_exact_namespace_claims_from_the_complete_profile",
                "iex_acceptance::tests::iex_acceptance_trade_identity_replay_and_native_provenance_are_exact",
                "iex_acceptance::tests::iex_acceptance_minute_bars_preserve_raw_coverage_count_vwap_and_bounds",
                "iex_acceptance::tests::iex_acceptance_rejects_unknown_semantics_stale_references_and_authority",
                "iex_acceptance::tests::iex_acceptance_profile_bytes_bind_complete_inputs_and_reject_ambiguity",
                "wss::tests::wss_charges_controls_fragments_and_exact_caps_before_exposure",
                "wss::tests::wss_rejects_untrusted_tls_bad_hostname_and_invalid_upgrades",
                "wss::tests::wss_protocol_overflow_and_lost_charge_are_terminal",
                "wss::tests::wss_cancellation_denial_and_abandoned_receive_never_reuse_socket",
                "wss::tests::wss_guard_bounds_idle_partial_work_and_total_lifetime",
                "wss::tests::wss_configuration_and_diagnostics_fail_closed",
                "wss::tests::wss_abandoned_committed_charge_never_exposes_or_replays_payload",
                "wss::tests::wss_fragment_and_outgoing_budgets_are_not_renewed_by_progress",
                "wss::tests::wss_frame_ceiling_is_inclusive_and_silent_upgrade_times_out",
            ],
        ),
        (
            "metered-provider-dispatch",
            vec![
                "tests::metered::each_page_requires_confirmed_permit_and_usage_is_ledger_backed",
                "tests::metered::lost_reservation_confirmation_and_inflight_drop_never_redispatch",
                "tests::metered::completed_page_reconfirmation_survives_lost_or_dropped_confirmation",
                "tests::metered::authority_cancellation_and_original_deadlines_are_rechecked_after_waits",
                "tests::metered::partial_overflow_and_retry_after_accounting_stays_conservative",
                "tests::metered::dispatch_diagnostics_and_usage_exclude_identity_and_payloads",
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
fn streaming_postgres_census_rejects_missing_renamed_duplicate_and_extra_proofs()
-> Result<(), String> {
    let check = inventory::checks()
        .into_iter()
        .find(|c| c.id == "streaming-session-postgres")
        .ok_or("streaming proof missing")?;
    assert_eq!(check.tier, "T3");
    assert_eq!(check.profiles, ["local-platform"]);
    assert!(matches!(
        check.prerequisites,
        [Requirement::Environment("EDGEAGENT_POSTGRES_URL")]
    ));
    let census = check.discovery.ok_or("streaming census missing")?;
    assert!(census.ignored);
    let expected = [
        "stream_replicas_serialize_capacity_and_fence_unresolved_connections",
        "stream_usage_survives_confirmation_loss_reconnect_and_transaction_failure",
        "stream_owner_authority_expiry_and_failed_start_never_reset_allocations",
    ];
    assert_eq!(census.expected, expected);
    let complete = expected
        .iter()
        .map(|n| format!("{n}: test\n"))
        .collect::<String>();
    runner::verify_discovery(&census, complete.as_bytes())?;
    for name in expected {
        for changed in [
            complete.replace(&format!("{name}: test\n"), ""),
            complete.replace(name, "renamed_streaming_proof"),
            format!("{complete}{name}: test\n"),
            format!("{complete}extra_streaming_proof: test\n"),
        ] {
            assert!(runner::verify_discovery(&census, changed.as_bytes()).is_err());
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
    for invalid in ["", "relative-binary", "private-sentinel-missing"] {
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
            "streaming-segment-contracts",
            "wss-transport-contracts",
            "stream-supervision-contracts",
            "iex-canonical-acceptance",
            "alpaca-iex-conformance",
            "streaming-session-contracts",
            "offline-worker-contracts",
            "offline-source-isolation",
            "instrument-selection-routing",
            "market-data-quality",
            "market-data-conformance",
            "acquisition-terminal-contracts",
            "acquisition-budget-contracts",
            "evidence-retention-contracts",
            "evidence-storage-contracts",
            "provider-http-contracts",
            "metered-provider-dispatch",
            "evidence-contracts",
            "evidence-freezing",
            "native-daily-acceptance",
            "native-publication-contracts",
            "qualification-preflight",
            "date-only-evidence",
            "twelve-data-conformance",
            "messaging-error-contracts",
            "alpaca-conformance",
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
        "/opt/Python Tools/python3".to_owned(),
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
    assert_eq!(child.get_program(), "/opt/Python Tools/python3");
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
