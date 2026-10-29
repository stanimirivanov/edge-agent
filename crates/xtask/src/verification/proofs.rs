//! Opt-in adapter proofs and their protected ignored-test identities.

use super::{Check, CommandSpec, Discovery, Program, Requirement};

const NATS: &[Requirement] = &[Requirement::Environment("EDGEAGENT_NATS_URL")];
const TOPOLOGY_BROKER: &[Requirement] = &[Requirement::ExecutableOrEnvironment(
    "docker",
    "EDGEAGENT_NATS_SERVER",
)];
const PROCESS_DEPENDENCIES: &[Requirement] = &[
    Requirement::Environment("EDGEAGENT_POSTGRES_URL"),
    Requirement::ExecutableOrEnvironment("docker", "EDGEAGENT_NATS_SERVER"),
];
const POSTGRES: &[Requirement] = &[Requirement::Environment("EDGEAGENT_POSTGRES_URL")];
const OFFLINE_STREAM_RUNTIME: Target = Target {
    package: "edgeagent-market-data",
    name: "offline_stream_runtime",
    identities: &[
        "offline_runtime_requires_approval_and_publishes_exact_ready_evidence",
        "offline_runtime_isolates_slow_sources_with_bounded_fair_dispatch",
        "offline_runtime_restart_recovers_exact_native_intent_and_fences_old_workers",
        "failures::offline_runtime_sticky_failures_and_revocation_never_grant_readiness",
    ],
};
const STREAM_PUBLICATION: Target = Target {
    package: "edgeagent-market-data-postgres",
    name: "stream_publication",
    identities: &[
        "stream_preparation_and_atomic_publication_survive_result_loss_and_rollback",
        "stream_publication_fences_generations_charged_ranges_and_chain_forks",
        "stream_stop_preserves_reads_withdrawal_denies_and_cleanup_requires_writer_quiescence",
        "stream_restore_requires_independent_approval_and_preserves_sticky_incidents",
        "stream_restore_recovers_exact_pending_intents_and_approval_rolls_back",
        "inputs::stream_owner_inputs_require_independent_roles_and_exact_current_loading",
        "inputs::stream_namespace_collisions_and_changed_retries_roll_back_every_claim",
        "inputs::stream_input_retirement_erases_private_scope_without_reopening_namespaces",
        "companion::native_pair_publication_recovers_exact_receipt_and_retains_partial_cleanup_inventory",
    ],
};
const STREAMING: Target = Target {
    package: "edgeagent-market-data-postgres",
    name: "streaming",
    identities: &[
        "stream_replicas_serialize_capacity_and_fence_unresolved_connections",
        "stream_usage_survives_confirmation_loss_reconnect_and_transaction_failure",
        "stream_owner_authority_expiry_and_failed_start_never_reset_allocations",
    ],
};
const QUALIFICATION_OWNER: Target = Target {
    package: "edgeagent-market-data-postgres",
    name: "qualification",
    identities: &[
        "qualification_roles_require_independent_owner_and_exact_current_profile",
        "qualification_withdrawal_and_terminal_failure_roll_back_all_effects",
        "qualification_retirement_erases_grants_without_owner_reconciliation_bypass",
        "qualification_withdrawal_races_and_policy_mismatches_leave_atomic_account_state",
    ],
};
const NATIVE_PUBLICATION: Target = Target {
    package: "edgeagent-market-data-postgres",
    name: "native",
    identities: &[
        "native_preparation_survives_restart_and_terminal_ack_loss_without_new_identity",
        "native_terminal_rollback_and_concurrent_corrections_preserve_atomic_fences",
        "native_revocation_denies_recovery_and_retirement_erases_preparations_not_cleanup_keys",
    ],
};
const EVIDENCE_RETENTION: Target = Target {
    package: "edgeagent-market-data-postgres",
    name: "retention",
    identities: &[
        "retiring_uncertain_calls_erases_fingerprints_but_preserves_fenced_account_blocks",
        "ready_evidence_survives_orphan_sweep_then_revocation_erases_private_publication_state",
        "failed_retirement_rolls_back_denial_and_account_redaction_atomically",
        "retiring_old_snapshots_preserves_new_heads_and_shared_version_ownership",
    ],
};
const ACQUISITION_TERMINAL: Target = Target {
    package: "edgeagent-market-data-postgres",
    name: "terminal",
    identities: &[
        "ready_and_rejection_commit_once_and_terminal_recovery_skips_work",
        "receipt_write_failure_rolls_back_inbox_outbox_heads_and_observations",
        "competing_terminal_outcomes_and_changed_inbox_identity_fail_closed",
        "concurrent_corrections_fence_heads_and_changed_observation_versions",
    ],
};
const ACQUISITION_LEDGER: Target = Target {
    package: "edgeagent-market-data-postgres",
    name: "ledger",
    identities: &[
        "duplicate_and_conflicting_admission_preserve_original_deadline_and_bytes",
        "competing_replicas_restart_and_lost_completion_preserve_permits_and_charges",
        "expired_cancelled_and_corrupt_state_fail_closed_without_partial_mutation",
        "daily_credits_commit_with_permits_and_survive_contention_rollback_and_restart",
    ],
};
const ACCOUNT_ADMISSION: Target = Target {
    package: "edgeagent-event-spine-conformance",
    name: "account_admission",
    identities: &[
        "audit_owner_export_rebuild_checkpoint_and_reordered_backfill_reconcile",
        "audit_history_rolls_back_with_worker_death_and_lost_result_replays_once",
        "competing_processes_cannot_overspend_cash_inventory_or_option_contracts",
        "rollback_process_crash_restart_and_lost_ack_preserve_one_effect",
        "aggregate_strategy_risk_and_breach_latches_survive_new_workers",
        "unauthorized_gapped_stale_and_tampered_quotes_leave_no_financial_state",
        "execution_fill_accounting_and_shared_liquidity_hold_across_products",
        "execution_crash_rollback_and_lost_ack_retain_one_fill",
        "execution_competing_workers_serialize_without_reusing_liquidity",
        "settlement_payoffs_defer_without_releasing_holds_and_close_once_across_strategies",
        "settlement_worker_death_and_lost_result_preserve_atomic_journals_and_cash",
        "settlement_competitors_authorization_conflicts_and_tampering_cannot_repay_a_position",
    ],
};
const EVENT_SPINE: &[Requirement] = &[
    Requirement::Environment("EDGEAGENT_NATS_URL"),
    Requirement::Environment("EDGEAGENT_POSTGRES_URL"),
];
const OFFLINE_WORKER: Target = Target {
    package: "edgeagent-market-data",
    name: "offline_worker",
    identities: &["offline_processes_commit_recover_relay_and_retire_without_provider_calls"],
};
const OFFLINE_RECOVERY: Target = Target {
    package: "edgeagent-market-data",
    name: "worker_recovery",
    identities: &[
        "killed_worker_before_terminal_commit_recovers_one_terminal_transition",
        "ack_loss_and_same_store_broker_restart_preserve_completed_receipts",
        "quota_process_death_preserves_shared_account_fences_and_offline_isolation",
    ],
};
const BROKER_RESTART: &[Requirement] = &[
    Requirement::Environment("EDGEAGENT_NATS_URL"),
    Requirement::Environment("EDGEAGENT_POSTGRES_URL"),
    Requirement::EnvironmentEquals("EDGEAGENT_NATS_COMPOSE_RESTART", "1"),
    Requirement::Executable("docker"),
];

struct Target {
    package: &'static str,
    name: &'static str,
    identities: &'static [&'static str],
}

const RESEARCH_EVIDENCE_BROKER: Target = Target {
    package: "edgeagent-market-data",
    name: "research_broker",
    identities: &[
        "broker_worker_commits_before_ack_relays_exact_outcome_and_quarantines_bad_correlation",
    ],
};
const RESEARCH_EVIDENCE_PROCESSES: Target = Target {
    package: "edgeagent-market-data",
    name: "research_evidence",
    identities: &[
        "pending_research_restarts_and_reordered_duplicates_reuse_one_acquisition",
        "unsupported_selection_and_revoked_owner_rights_abstain_without_objects",
        "incomplete_owner_history_abstains_without_a_ready_reference",
    ],
};
const RESEARCH_CORRELATION: Target = Target {
    package: "edgeagent-market-data-postgres",
    name: "research",
    identities: &["correlation_commit_failures_preserve_pending_work_without_inbox_or_outbox"],
};
const GATEWAY_ADMISSION: Target = Target {
    package: "edgeagent-gateway",
    name: "admission",
    identities: &[
        "http_retries_restart_and_conflicting_reuse_preserve_one_command",
        "concurrent_admission_and_separate_scopes_are_idempotent",
        "statement_and_deferred_commit_failures_never_acknowledge_or_retain_intent",
        "immutable_admission_and_corrupt_retained_outbox_fail_closed",
    ],
};
const RESEARCH_PERSISTENCE: Target = Target {
    package: "edgeagent-research-postgres",
    name: "persistence",
    identities: &[
        "duplicate_requests_return_first_outcome_and_conflicting_intent_rolls_back",
        "concurrent_completion_and_client_restart_recover_exact_original_receipt",
        "statement_and_deferred_commit_failures_rollback_completion_inbox_and_outbox",
        "cancelled_transaction_leaves_no_completion_or_publication_intent",
        "immutable_completion_and_corrupt_retained_outbox_fail_closed",
    ],
};
const PUBLISH: Target = Target {
    package: "edgeagent-messaging-nats",
    name: "jetstream_publish",
    identities: &["persisted_message_identity_deduplicates_on_retry"],
};
const CONSUME: Target = Target {
    package: "edgeagent-messaging-nats",
    name: "jetstream_consume",
    identities: &[
        "delivery_settlement_controls_redelivery_and_acknowledgement",
        "abandoned_settlement_future_leaves_delivery_eligible_for_redelivery",
        "oversized_raw_delivery_halts_intake_without_acknowledgement",
    ],
};
const NATS_BOUNDARIES: Target = Target {
    package: "edgeagent-messaging-nats",
    name: "jetstream_boundaries",
    identities: &[
        "publication_without_an_owning_stream_is_rejected",
        "stream_message_limit_is_classified_as_rejected",
        "header_only_consumer_is_rejected_without_starting_intake",
        "oversized_publication_never_reaches_transport",
        "non_success_settlement_response_is_not_confirmation",
        "owned_stream_cleanup_runs_after_failure_panic_and_timeout",
    ],
};
const NATS_LIFECYCLE: Target = Target {
    package: "edgeagent-messaging-nats",
    name: "jetstream_lifecycle",
    identities: &[
        "protocol_halt_stops_expiry_and_reconnect_pulls",
        "deleted_consumer_closes_intake_while_adapter_remains_alive",
        "completed_receive_does_not_prefetch_while_delivery_is_held",
    ],
};
const REGISTRY_TOPOLOGY: Target = Target {
    package: "edgeagent-event-spine-conformance",
    name: "jetstream_topology",
    identities: &[
        "provisioning_preserves_evidence_and_rejects_drift_before_writes",
        "runtime_permissions_enforce_delivery_retention_and_admission_bounds",
    ],
};
const PROCESS_ACCEPTANCE: Target = Target {
    package: "edgeagent-event-spine-conformance",
    name: "process_acceptance",
    identities: &[
        "independent_processes_commit_acceptance_transition_and_fact_once",
        "process_rollbacks_leave_no_partial_acceptance_or_inbound_fact",
        "semantics::competing_semantic_commands_commit_one_transition_and_retain_conflicts",
        "semantics::missing_predecessors_recover_in_order_or_exhaust_into_quarantine",
        "replicas::competing_replicas_respect_shared_inflight_high_water",
        "replicas::locked_partition_and_connection_loss_preserve_other_progress_and_recovery",
        "replay::authorization::authenticated_replay_authorization_is_scoped_atomic_and_immutable",
        "replay::recovery::inbound_replay_recovers_persisted_result_loss_with_one_domain_transition",
        "replay::routes::original_route_replay_reaches_all_subscribers_without_exclusive_targeting",
        "replay::outbound::outbound_replay_preserves_audit_budget_and_stale_claim_fences",
        "telemetry::exported_process_signals_preserve_context_privacy_and_bounded_dimensions",
        "telemetry::rejected_or_stalled_exporters_preserve_commits_duplicates_and_settlement",
        "failures::killed_producers_recover_before_commit_and_after_durable_enqueue",
        "failures::killed_relay_recovers_persisted_publication_without_second_effect",
        "failures::killed_consumer_after_commit_recovers_as_duplicate_before_ack",
        "failures::killed_file_broker_retains_message_and_durable_across_fresh_workers",
        "failures::wire_ack_request_and_confirmation_loss_have_distinct_recovery",
        "failures::inputs::duplicate_after_broker_window_preserves_one_committed_transition",
        "failures::inputs::invalid_inputs_quarantine_or_halt_without_false_acknowledgement",
    ],
};
const OUTBOX: Target = Target {
    package: "edgeagent-outbox-postgres",
    name: "postgres_outbox",
    identities: &[
        "transaction_identity_and_lease_invariants_hold",
        "same_owner_reclaim_rejects_stale_transitions_and_preserves_replay_fence",
    ],
};
const OUTBOX_RELAY_BOUNDARIES: Target = Target {
    package: "edgeagent-outbox-postgres",
    name: "postgres_relay_boundaries",
    identities: &[
        "nested_relay_operations_preserve_the_callers_transaction",
        "successful_relay_operations_are_visible_after_return",
        "deferred_commit_rejection_is_an_invariant_and_preserves_evidence",
        "terminated_connection_is_unavailable_through_relay_port",
        "invalid_stored_claim_rolls_back_before_connection_reuse",
    ],
};
const OUTBOX_RECOVERY: Target = Target {
    package: "edgeagent-outbox-postgres",
    name: "postgres_recovery",
    identities: &[
        "concurrency::competing_claims_skip_locked_and_bound_batches",
        "concurrency::waiting_enqueues_observe_committed_identity_decisions",
        "concurrency::waiting_enqueue_inserts_after_competing_rollback",
        "concurrency::competing_replays_release_and_audit_only_once",
        "concurrency::nonzero_retry_delay_defers_claim_until_persisted_eligibility",
        "cancellation::precommit_cancellation_rolls_back_each_transition_before_reuse",
        "cancellation::cancelled_blocked_outcome_recovers_with_a_new_fence",
        "fixture::owned_schema_cleanup_runs_after_error_panic_and_timeout",
    ],
};
const INBOX: Target = Target {
    package: "edgeagent-inbox-postgres",
    name: "postgres_inbox",
    identities: &[
        "inbox_and_quarantine_preserve_consumer_invariants",
        "attempt_range_migration_preserves_existing_evidence",
    ],
};
const INBOX_ERRORS: Target = Target {
    package: "edgeagent-inbox-postgres",
    name: "postgres_errors",
    identities: &[
        "permanent_schema_faults_are_invariants_through_inbound_port",
        "closed_postgres_connection_remains_unavailable_through_inbound_port",
        "replay_row_decode_fault_is_invariant_and_audit_rolls_back",
    ],
};
const HANDLER: Target = Target {
    package: "edgeagent-inbox-handler",
    name: "postgres_handler",
    identities: &["coordinator_preserves_commit_retry_and_quarantine_ordering"],
};
const ATOMIC_ENQUEUE: Target = Target {
    package: "edgeagent-inbox-handler",
    name: "sqlx_outbox_atomicity",
    identities: &["inbox_domain_and_sqlx_outbox_commit_or_roll_back_together"],
};
const RELAY: Target = Target {
    package: "edgeagent-outbox-relay",
    name: "postgres_relay",
    identities: &[
        "relay_bounds_retry_and_quarantines_terminal_failures",
        "recovery::cancelled_publication_recovers_over_budget_without_another_publish",
    ],
};
const SPINE: Target = Target {
    package: "edgeagent-inbox-handler",
    name: "event_spine",
    identities: &[
        "durable_command_survives_client_restart_with_one_domain_transition",
        "durable_command_survives_broker_restart_with_one_domain_transition",
        "publication_recovery::producer_result_loss_recovers_within_budget_and_quarantines_after_exhaustion",
    ],
};

pub(super) fn checks() -> Vec<Check> {
    vec![
        suite(
            "research-evidence-broker",
            &RESEARCH_EVIDENCE_BROKER,
            EVENT_SPINE,
            "Actual JetStream delivery/confirmed ACK, duplicate process recovery, exact correlated outbox relay and poison quarantine before termination.",
        ),
        suite(
            "research-evidence-processes",
            &RESEARCH_EVIDENCE_PROCESSES,
            POSTGRES,
            "Separate offline worker processes recover pending correlations and owner receipts; duplicates, denial and quality refusal retain one outcome without providers.",
        ),
        suite(
            "research-correlation-postgres",
            &RESEARCH_CORRELATION,
            POSTGRES,
            "Real scoped correlation atomicity: forged ready refusal, statement/deferred-commit rollback and retained outbox integrity.",
        ),
        suite(
            "gateway-admission",
            &GATEWAY_ADMISSION,
            POSTGRES,
            "Real Gateway HTTP acceptance after request/outbox commit, retries, scope isolation, contention, rollback and integrity refusal.",
        ),
        suite(
            "research-persistence",
            &RESEARCH_PERSISTENCE,
            POSTGRES,
            "Real research completion/inbox/outbox atomicity, scoped concurrency, original receipt recovery, cancellation and integrity refusal; no broker publication.",
        ),
        suite(
            "offline-stream-runtime",
            &OFFLINE_STREAM_RUNTIME,
            POSTGRES,
            "Finite synthetic runtime: actual independent roles, grouped native publication, slow-source isolation, worker-state loss, atomic restore fences, exact history recovery and domain readiness denial; no provider activation.",
        ),
        suite(
            "streaming-publication-postgres",
            &STREAM_PUBLICATION,
            POSTGRES,
            "Synthetic exact pre-PUT recovery, atomic capture/receipt/outbox rollback, generation fences, stop/withdrawal and independently quiescent cleanup; no vendor activation.",
        ),
        suite(
            "streaming-session-postgres",
            &STREAMING,
            POSTGRES,
            "Synthetic account contention, independent owner control, durable uncertainty, generations, frame confirmation loss and atomic failure; no provider or socket activation.",
        ),
        suite(
            "qualification-owner-postgres",
            &QUALIFICATION_OWNER,
            POSTGRES,
            "Authenticated owner/worker SQL roles, exact current profile, withdrawal/reservation races, terminal rollback/recovery and retirement erasure; no vendor I/O or runtime qualification.",
        ),
        suite(
            "native-publication-postgres",
            &NATIVE_PUBLICATION,
            POSTGRES,
            "Owned PostgreSQL native preparation restart, exact uncertain PUT recovery, terminal rollback/ACK loss, competing heads and revocation/retirement erasure; no provider calls or process-death qualification.",
        ),
        suite(
            "offline-worker-recovery",
            &OFFLINE_RECOVERY,
            PROCESS_DEPENDENCIES,
            "Owned process death before terminal COMMIT and after commit before ACK; request/confirmation loss, same-store broker restart, independent shared-account quota fencing and zero offline provider charges. No vendor calls or cloud/HA qualification.",
        ),
        suite(
            "offline-worker-processes",
            &OFFLINE_WORKER,
            EVENT_SPINE,
            "Real offline worker processes: immutable terminal recovery, outbox relay and quiescence-gated cleanup; no vendor calls, worker death or ACK-loss qualification.",
        ),
        suite(
            "evidence-retention-postgres",
            &EVIDENCE_RETENTION,
            POSTGRES,
            "Owned PostgreSQL proofs: orphan/live separation, immediate namespace denial, private metadata erasure, atomic rollback and opaque generation-fenced account recovery; no provider/cloud calls.",
        ),
        suite(
            "acquisition-terminal-postgres",
            &ACQUISITION_TERMINAL,
            POSTGRES,
            "Atomic receipt/inbox/outbox, rollback, terminal recovery, concurrent corrections and immutable versions; isolated PostgreSQL, no external provider/broker.",
        ),
        suite(
            "acquisition-ledger-postgres",
            &ACQUISITION_LEDGER,
            POSTGRES,
            "Owned PostgreSQL schemas prove immutable admission, competing clients, client restart and completion-loss replay. No provider calls or worker-death qualification.",
        ),
        suite(
            "account-admission-processes",
            &ACCOUNT_ADMISSION,
            POSTGRES,
            "Independent restricted workers prove synthetic cash/inventory/options admission and execution, shared liquidity, balanced accounting, retained virtual latency, sticky risk, rollback, process death and duplicate recovery; not provider/market-fidelity, HA or live qualification.",
        ),
        suite(
            "event-spine-processes",
            &PROCESS_ACCEPTANCE,
            PROCESS_DEPENDENCIES,
            "Independent processes prove acceptance, ordering/admission, fenced replay, OTLP, actual worker death, same-store broker restart, post-dedup duplicates, invalid-input bounds and wire ACK request/confirmation loss. Single-node synthetic qualification, not HA, power-loss, PostgreSQL restart or load proof.",
        ),
        suite(
            "nats-topology",
            &REGISTRY_TOPOLOGY,
            TOPOLOGY_BROKER,
            "Exclusive authenticated pinned broker proves registry-derived file topology, create-only drift rejection, exact runtime ACLs, retained replay, durable redelivery and bounded admission; no production deployment or crash-recovery claim.",
        ),
        exact(
            "nats-publish",
            &PUBLISH,
            "persisted_message_identity_deduplicates_on_retry",
            NATS,
            "JetStream persistence and immutable message-identity retry; not exactly-once business execution.",
        ),
        exact(
            "nats-consume",
            &CONSUME,
            "delivery_settlement_controls_redelivery_and_acknowledgement",
            NATS,
            "Confirmed acknowledgement and redelivery against an isolated JetStream server.",
        ),
        exact(
            "nats-abandoned-settlement",
            &CONSUME,
            "abandoned_settlement_future_leaves_delivery_eligible_for_redelivery",
            NATS,
            "Abandoned settlement remains recoverable through JetStream redelivery.",
        ),
        exact(
            "nats-oversize",
            &CONSUME,
            "oversized_raw_delivery_halts_intake_without_acknowledgement",
            NATS,
            "Oversized broker payload halts intake without an implicit acknowledgement.",
        ),
        exact(
            "nats-no-stream",
            &NATS_BOUNDARIES,
            "publication_without_an_owning_stream_is_rejected",
            NATS,
            "A real no-responder publication is a transport rejection.",
        ),
        exact(
            "nats-size-rejection",
            &NATS_BOUNDARIES,
            "stream_message_limit_is_classified_as_rejected",
            NATS,
            "Typed server message-size rejection is not an unknown confirmation.",
        ),
        exact(
            "nats-header-only",
            &NATS_BOUNDARIES,
            "header_only_consumer_is_rejected_without_starting_intake",
            NATS,
            "Header-only configuration is rejected before broker intake starts.",
        ),
        exact(
            "nats-publish-oversize",
            &NATS_BOUNDARIES,
            "oversized_publication_never_reaches_transport",
            NATS,
            "Oversized publication fails contract preparation before any transport request.",
        ),
        exact(
            "nats-settlement-response",
            &NATS_BOUNDARIES,
            "non_success_settlement_response_is_not_confirmation",
            NATS,
            "Deleting real consumers proves 503/no-responder ACK/NAK/TERM replies are not confirmation.",
        ),
        exact(
            "nats-halted-worker",
            &NATS_LIFECYCLE,
            "protocol_halt_stops_expiry_and_reconnect_pulls",
            NATS,
            "A live halted adapter issues no expiry/reconnect pulls; an active same-connection control responds to reconnect.",
        ),
        exact(
            "nats-closed-intake",
            &NATS_LIFECYCLE,
            "deleted_consumer_closes_intake_while_adapter_remains_alive",
            NATS,
            "Broker consumer deletion closes intake; repeated calls resolve immediately without new pulls.",
        ),
        exact(
            "nats-intake-bound",
            &NATS_LIFECYCLE,
            "completed_receive_does_not_prefetch_while_delivery_is_held",
            NATS,
            "Holding a delivery past SDK renewal produces no new pulls despite permissive broker limits; later receives drain retained work.",
        ),
        exact(
            "nats-fixture-cleanup",
            &NATS_BOUNDARIES,
            "owned_stream_cleanup_runs_after_failure_panic_and_timeout",
            NATS,
            "Shared owned-stream cleanup deletes resources after error, synchronous/asynchronous panic, and timeout; typed lookup proves absence.",
        ),
        exact(
            "outbox-conformance",
            &OUTBOX,
            "transaction_identity_and_lease_invariants_hold",
            POSTGRES,
            "Real PostgreSQL outbox transaction, immutable retry, lease and replay invariants.",
        ),
        exact(
            "outbox-fencing",
            &OUTBOX,
            "same_owner_reclaim_rejects_stale_transitions_and_preserves_replay_fence",
            POSTGRES,
            "Same-owner lease reclamation rejects stale completion and preserves replay fences.",
        ),
        suite(
            "outbox-relay-boundaries",
            &OUTBOX_RELAY_BOUNDARIES,
            POSTGRES,
            "Top-level relay commits, nested-transaction rejection, permanent commit faults, connectivity classification and invalid-claim rollback against real PostgreSQL.",
        ),
        suite(
            "outbox-recovery",
            &OUTBOX_RECOVERY,
            POSTGRES,
            "Independent PostgreSQL contenders, persisted eligibility, precommit cancellation with rollback/reuse, recovered fencing, and owned-schema cleanup on error/panic/timeout; no FIFO or unknown-commit proof.",
        ),
        suite(
            "inbox-invariants",
            &INBOX,
            POSTGRES,
            "Real PostgreSQL inbox/quarantine atomicity and migration preservation of retained evidence.",
        ),
        suite(
            "inbox-errors",
            &INBOX_ERRORS,
            POSTGRES,
            "Real PostgreSQL schema, connectivity and row-decode errors retain port classification and rollback.",
        ),
        exact(
            "handler-ordering",
            &HANDLER,
            "coordinator_preserves_commit_retry_and_quarantine_ordering",
            POSTGRES,
            "Handler commit, retry and quarantine ordering against real PostgreSQL.",
        ),
        exact(
            "atomic-enqueue",
            &ATOMIC_ENQUEUE,
            "inbox_domain_and_sqlx_outbox_commit_or_roll_back_together",
            POSTGRES,
            "Inbox, service-owned domain data and outbox enqueue commit or roll back in one SQLx transaction.",
        ),
        exact(
            "relay-conformance",
            &RELAY,
            "relay_bounds_retry_and_quarantines_terminal_failures",
            POSTGRES,
            "Persisted relay retry bounds and terminal quarantine; publication uses a controlled fake.",
        ),
        exact(
            "relay-recovery-budget",
            &RELAY,
            "recovery::cancelled_publication_recovers_over_budget_without_another_publish",
            POSTGRES,
            "Cancelling the relay during a pending publisher leaves a durable PostgreSQL claim; same-owner recovery quarantines over budget without new publication, preserving bytes and rejecting stale completion.",
        ),
        exact(
            "event-spine-producer",
            &SPINE,
            "publication_recovery::producer_result_loss_recovers_within_budget_and_quarantines_after_exhaustion",
            EVENT_SPINE,
            "Real JetStream persistence with a held confirmed receipt proves application result loss (not wire ACK loss): recovery deduplicates within budget or quarantines without new I/O after exhaustion.",
        ),
        exact(
            "event-spine-client",
            &SPINE,
            "durable_command_survives_client_restart_with_one_domain_transition",
            EVENT_SPINE,
            "One synthetic command recovers client handles with one domain transition; no load or PostgreSQL restart proof.",
        ),
        exact(
            "event-spine-broker",
            &SPINE,
            "durable_command_survives_broker_restart_with_one_domain_transition",
            BROKER_RESTART,
            "One synthetic command survives the explicitly authorized isolated Compose NATS restart.",
        ),
    ]
}

fn exact(
    id: &'static str,
    target: &Target,
    identity: &'static str,
    prerequisites: &'static [Requirement],
    evidence: &'static str,
) -> Check {
    let mut check = suite(id, target, prerequisites, evidence);
    check.command.args.extend(["--exact", identity]);
    check
}

fn suite(
    id: &'static str,
    target: &Target,
    prerequisites: &'static [Requirement],
    evidence: &'static str,
) -> Check {
    let mut args = vec![
        "test",
        "--locked",
        "-p",
        target.package,
        "--test",
        target.name,
        "--",
        "--ignored",
    ];
    if id == "event-spine-processes" {
        // Each scenario already owns concurrent processes, a broker and SQL
        // schemas. Bound fixture admission independently of host CPU count;
        // retain every scenario's internal contention and original deadlines.
        args.push("--test-threads=2");
    }
    if id == "offline-worker-recovery" {
        // Each case owns child processes, a file broker and a private schema.
        args.push("--test-threads=1");
    }
    let mut discovery_args = args.clone();
    discovery_args.push("--list");
    Check {
        id,
        tier: "T3",
        profiles: if matches!(id, "event-spine-processes" | "nats-topology") {
            &["m02-qualification"]
        } else {
            &["local-platform"]
        },
        command: CommandSpec {
            program: Program::Cargo,
            args,
        },
        prerequisites,
        // List the whole target, not the filtered test: otherwise a newly
        // ignored test could silently disappear from acceptance coverage.
        discovery: Some(Discovery {
            args: discovery_args,
            expected: target.identities.to_vec(),
            ignored: true,
        }),
        evidence,
    }
}

pub(super) fn ignored_inventory() -> Vec<&'static str> {
    let mut identities = vec!["messaging::tests::payload_decode_throughput_probe"];
    for target in [
        RESEARCH_EVIDENCE_BROKER,
        RESEARCH_EVIDENCE_PROCESSES,
        RESEARCH_CORRELATION,
        GATEWAY_ADMISSION,
        RESEARCH_PERSISTENCE,
        PUBLISH,
        CONSUME,
        NATS_BOUNDARIES,
        NATS_LIFECYCLE,
        REGISTRY_TOPOLOGY,
        PROCESS_ACCEPTANCE,
        ACCOUNT_ADMISSION,
        ACQUISITION_LEDGER,
        ACQUISITION_TERMINAL,
        QUALIFICATION_OWNER,
        STREAM_PUBLICATION,
        OFFLINE_STREAM_RUNTIME,
        STREAMING,
        NATIVE_PUBLICATION,
        EVIDENCE_RETENTION,
        OFFLINE_WORKER,
        OFFLINE_RECOVERY,
        OUTBOX,
        OUTBOX_RELAY_BOUNDARIES,
        OUTBOX_RECOVERY,
        INBOX,
        INBOX_ERRORS,
        HANDLER,
        ATOMIC_ENQUEUE,
        RELAY,
        SPINE,
    ] {
        identities.extend(target.identities);
    }
    identities
}

#[cfg(test)]
mod tests {
    #[test]
    fn qualification_owner_census_rejects_removed_renamed_duplicate_and_extra_proofs()
    -> Result<(), String> {
        let check = super::checks()
            .into_iter()
            .find(|c| c.id == "qualification-owner-postgres")
            .ok_or("qualification proof")?;
        let census = check.discovery.ok_or("qualification census")?;
        let expected = [
            "qualification_roles_require_independent_owner_and_exact_current_profile",
            "qualification_withdrawal_and_terminal_failure_roll_back_all_effects",
            "qualification_retirement_erases_grants_without_owner_reconciliation_bypass",
            "qualification_withdrawal_races_and_policy_mismatches_leave_atomic_account_state",
        ];
        assert_eq!(census.expected, expected);
        let complete = expected
            .iter()
            .map(|n| format!("{n}: test\n"))
            .collect::<String>();
        super::super::runner::verify_discovery(&census, complete.as_bytes())?;
        for n in expected {
            for changed in [
                complete.replace(&format!("{n}: test\n"), ""),
                complete.replace(n, "renamed_qualification"),
                format!("{complete}{n}: test\n"),
                format!("{complete}extra: test\n"),
            ] {
                assert!(
                    super::super::runner::verify_discovery(&census, changed.as_bytes()).is_err()
                );
            }
        }
        Ok(())
    }
    #[test]
    fn native_publication_census_cannot_silently_shrink() -> Result<(), String> {
        let check = super::checks()
            .into_iter()
            .find(|c| c.id == "native-publication-postgres")
            .ok_or("native gate")?;
        let census = check.discovery.ok_or("native census")?;
        let expected = [
            "native_preparation_survives_restart_and_terminal_ack_loss_without_new_identity",
            "native_terminal_rollback_and_concurrent_corrections_preserve_atomic_fences",
            "native_revocation_denies_recovery_and_retirement_erases_preparations_not_cleanup_keys",
        ];
        assert_eq!(census.expected, expected);
        let complete = expected
            .iter()
            .map(|n| format!("{n}: test\n"))
            .collect::<String>();
        super::super::runner::verify_discovery(&census, complete.as_bytes())?;
        for n in expected {
            for changed in [
                complete.replace(&format!("{n}: test\n"), ""),
                complete.replace(n, "renamed_native"),
                format!("{complete}{n}: test\n"),
                format!("{complete}extra: test\n"),
            ] {
                assert!(
                    super::super::runner::verify_discovery(&census, changed.as_bytes()).is_err()
                );
            }
        }
        Ok(())
    }
    #[test]
    fn offline_recovery_census_and_resource_bounds_cannot_silently_shrink() -> Result<(), String> {
        let check = super::checks()
            .into_iter()
            .find(|c| c.id == "offline-worker-recovery")
            .ok_or("recovery gate")?;
        assert_eq!(check.profiles, ["local-platform"]);
        assert!(matches!(
            check.prerequisites,
            [
                super::Requirement::Environment("EDGEAGENT_POSTGRES_URL"),
                super::Requirement::ExecutableOrEnvironment("docker", "EDGEAGENT_NATS_SERVER")
            ]
        ));
        assert!(check.command.args.contains(&"--test-threads=1"));
        let census = check.discovery.ok_or("recovery census")?;
        assert_eq!(census.expected, super::OFFLINE_RECOVERY.identities);
        assert_eq!(census.expected.len(), 3);
        let complete = census
            .expected
            .iter()
            .map(|n| format!("{n}: test\n"))
            .collect::<String>();
        super::super::runner::verify_discovery(&census, complete.as_bytes())?;
        for n in &census.expected {
            for changed in [
                complete.replace(&format!("{n}: test\n"), ""),
                complete.replace(n, "renamed_recovery"),
                format!("{complete}{n}: test\n"),
                format!("{complete}extra: test\n"),
            ] {
                assert!(
                    super::super::runner::verify_discovery(&census, changed.as_bytes()).is_err()
                );
            }
        }
        Ok(())
    }
    #[test]
    fn retention_census_rejects_missing_renamed_duplicate_and_extra_proofs() -> Result<(), String> {
        let check = super::checks()
            .into_iter()
            .find(|c| c.id == "evidence-retention-postgres")
            .ok_or("retention gate")?;
        let census = check.discovery.ok_or("retention census")?;
        let expected = [
            "retiring_uncertain_calls_erases_fingerprints_but_preserves_fenced_account_blocks",
            "ready_evidence_survives_orphan_sweep_then_revocation_erases_private_publication_state",
            "failed_retirement_rolls_back_denial_and_account_redaction_atomically",
            "retiring_old_snapshots_preserves_new_heads_and_shared_version_ownership",
        ];
        assert_eq!(census.expected, expected);
        assert_eq!(check.profiles, ["local-platform"]);
        let complete = expected
            .iter()
            .map(|n| format!("{n}: test\n"))
            .collect::<String>();
        super::super::runner::verify_discovery(&census, complete.as_bytes())?;
        for n in expected {
            for changed in [
                complete.replace(&format!("{n}: test\n"), ""),
                complete.replace(n, "renamed_retention"),
                format!("{complete}{n}: test\n"),
                format!("{complete}extra: test\n"),
            ] {
                assert!(
                    super::super::runner::verify_discovery(&census, changed.as_bytes()).is_err()
                );
            }
        }
        Ok(())
    }
    #[test]
    fn terminal_census_rejects_missing_renamed_duplicate_and_extra_proofs() -> Result<(), String> {
        let check = super::checks()
            .into_iter()
            .find(|c| c.id == "acquisition-terminal-postgres")
            .ok_or("terminal suite")?;
        assert_eq!(check.profiles, ["local-platform"]);
        assert!(matches!(
            check.prerequisites,
            [super::Requirement::Environment("EDGEAGENT_POSTGRES_URL")]
        ));
        let census = check.discovery.ok_or("terminal census")?;
        let expected = [
            "ready_and_rejection_commit_once_and_terminal_recovery_skips_work",
            "receipt_write_failure_rolls_back_inbox_outbox_heads_and_observations",
            "competing_terminal_outcomes_and_changed_inbox_identity_fail_closed",
            "concurrent_corrections_fence_heads_and_changed_observation_versions",
        ];
        assert_eq!(census.expected, expected);
        let complete = expected
            .iter()
            .map(|n| format!("{n}: test\n"))
            .collect::<String>();
        super::super::runner::verify_discovery(&census, complete.as_bytes())?;
        for name in expected {
            for changed in [
                complete.replace(&format!("{name}: test\n"), ""),
                complete.replace(name, "renamed_terminal"),
                format!("{complete}{name}: test\n"),
                format!("{complete}unknown_terminal: test\n"),
            ] {
                assert!(
                    super::super::runner::verify_discovery(&census, changed.as_bytes()).is_err()
                );
            }
        }
        Ok(())
    }
    #[test]
    fn acquisition_census_rejects_missing_renamed_duplicate_and_extra_proofs() -> Result<(), String>
    {
        let check = super::checks()
            .into_iter()
            .find(|c| c.id == "acquisition-ledger-postgres")
            .ok_or("acquisition proof missing")?;
        assert_eq!(check.profiles, ["local-platform"]);
        assert!(matches!(
            check.prerequisites,
            [super::Requirement::Environment("EDGEAGENT_POSTGRES_URL")]
        ));
        let discovery = check.discovery.ok_or("acquisition census absent")?;
        let mut expected = vec![
            "duplicate_and_conflicting_admission_preserve_original_deadline_and_bytes",
            "competing_replicas_restart_and_lost_completion_preserve_permits_and_charges",
            "expired_cancelled_and_corrupt_state_fail_closed_without_partial_mutation",
            "daily_credits_commit_with_permits_and_survive_contention_rollback_and_restart",
        ];
        expected.sort_unstable();
        let mut actual = discovery.expected.clone();
        actual.sort_unstable();
        assert_eq!(actual, expected);
        let complete = expected
            .iter()
            .map(|name| format!("{name}: test\n"))
            .collect::<String>();
        super::super::runner::verify_discovery(&discovery, complete.as_bytes())?;
        for name in expected {
            for mutation in [
                complete.replace(&format!("{name}: test\n"), ""),
                complete.replace(name, "renamed_acquisition"),
                format!("{complete}{name}: test\n"),
                format!("{complete}unregistered_acquisition: test\n"),
            ] {
                assert!(
                    super::super::runner::verify_discovery(&discovery, mutation.as_bytes())
                        .is_err()
                );
            }
        }
        Ok(())
    }
    use super::*;
    use std::{collections::BTreeSet, error::Error};

    #[test]
    fn account_admission_gate_requires_postgres_and_each_named_process_proof() -> Result<(), String>
    {
        let check = checks()
            .into_iter()
            .find(|check| check.id == "account-admission-processes")
            .ok_or("account gate missing")?;
        assert_eq!(check.profiles, ["local-platform"]);
        assert!(matches!(
            check.prerequisites,
            [Requirement::Environment("EDGEAGENT_POSTGRES_URL")]
        ));
        assert!(check.command.args.contains(&"account_admission"));
        let discovery = check.discovery.ok_or("account discovery missing")?;
        assert_eq!(discovery.expected.len(), 12);
        let complete = discovery
            .expected
            .iter()
            .map(|name| format!("{name}: test\n"))
            .collect::<String>();
        super::super::runner::verify_discovery(&discovery, complete.as_bytes())?;
        for name in &discovery.expected {
            let removed = complete.replace(&format!("{name}: test\n"), "");
            assert!(
                super::super::runner::verify_discovery(&discovery, removed.as_bytes()).is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn process_fixture_admission_is_bounded_without_filtering_the_suite() -> Result<(), String> {
        let check = checks()
            .into_iter()
            .find(|c| c.id == "event-spine-processes")
            .ok_or("process proof missing")?;
        assert!(check.command.args.contains(&"--test-threads=2"));
        assert_eq!(
            check.command.args,
            [
                "test",
                "--locked",
                "-p",
                "edgeagent-event-spine-conformance",
                "--test",
                "process_acceptance",
                "--",
                "--ignored",
                "--test-threads=2"
            ]
        );
        assert!(!check.command.args.contains(&"--exact"));
        let discovery = check.discovery.ok_or("process discovery missing")?;
        assert_eq!(discovery.expected.len(), 19);
        assert!(discovery.args.contains(&"--test-threads=2"));
        Ok(())
    }

    #[test]
    fn proofs_cover_each_external_ignored_identity_without_duplicate_execution()
    -> Result<(), Box<dyn Error>> {
        let mut executed = Vec::new();
        let checks = checks();
        assert_eq!(checks.len(), 44);
        for check in checks {
            let discovery = check.discovery.ok_or("every proof requires discovery")?;
            assert!(discovery.ignored);
            assert_eq!(discovery.args.last(), Some(&"--list"));
            assert!(!discovery.args.contains(&"--exact"));
            if check.command.args.contains(&"--exact") {
                let identity = *check.command.args.last().ok_or("exact test identity")?;
                assert!(discovery.expected.contains(&identity));
                executed.push(identity);
            } else {
                executed.extend(discovery.expected);
            }
        }
        let inventory = ignored_inventory();
        // Includes owner research evidence process/correlation and Gateway admission proofs.
        assert_eq!(inventory.len(), 127);
        assert_eq!(inventory.iter().collect::<BTreeSet<_>>().len(), 127);
        let expected = inventory
            .into_iter()
            .filter(|identity| *identity != "messaging::tests::payload_decode_throughput_probe")
            .collect::<BTreeSet<_>>();
        assert_eq!(executed.len(), expected.len());
        assert_eq!(executed.into_iter().collect::<BTreeSet<_>>(), expected);
        Ok(())
    }

    #[test]
    fn relay_boundary_suite_protects_each_identity_before_execution() -> Result<(), String> {
        for (id, target) in [
            ("outbox-relay-boundaries", "postgres_relay_boundaries"),
            ("outbox-recovery", "postgres_recovery"),
        ] {
            let check = checks()
                .into_iter()
                .find(|check| check.id == id)
                .ok_or("relay boundary proof missing")?;
            assert_eq!(check.profiles, ["local-platform"]);
            assert!(matches!(
                check.prerequisites,
                [Requirement::Environment("EDGEAGENT_POSTGRES_URL")]
            ));
            assert!(check.command.args.contains(&target));
            assert!(!check.command.args.contains(&"--exact"));
            let discovery = check.discovery.ok_or("relay boundary discovery missing")?;
            let complete = discovery
                .expected
                .iter()
                .map(|identity| format!("{identity}: test\n"))
                .collect::<String>();
            super::super::runner::verify_discovery(&discovery, complete.as_bytes())?;
            for identity in &discovery.expected {
                let removed = complete.replace(&format!("{identity}: test\n"), "");
                assert!(
                    super::super::runner::verify_discovery(&discovery, removed.as_bytes()).is_err(),
                    "missing relay boundary proof passed discovery: {identity}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn recovery_budget_proofs_reject_missing_renamed_duplicate_and_extra_identities()
    -> Result<(), String> {
        for id in ["relay-recovery-budget", "event-spine-producer"] {
            let check = checks()
                .into_iter()
                .find(|check| check.id == id)
                .ok_or("recovery proof missing")?;
            assert_eq!(check.profiles, ["local-platform"]);
            assert!(check.command.args.contains(&"--exact"));
            assert!(check.prerequisites.iter().any(|requirement| matches!(
                requirement,
                Requirement::Environment("EDGEAGENT_POSTGRES_URL")
            )));
            assert_eq!(
                check.prerequisites.iter().any(|requirement| matches!(
                    requirement,
                    Requirement::Environment("EDGEAGENT_NATS_URL")
                )),
                id == "event-spine-producer"
            );
            let discovery = check.discovery.ok_or("recovery discovery missing")?;
            let complete = discovery
                .expected
                .iter()
                .map(|identity| format!("{identity}: test\n"))
                .collect::<String>();
            super::super::runner::verify_discovery(&discovery, complete.as_bytes())?;
            for identity in &discovery.expected {
                for mutation in [
                    complete.replace(&format!("{identity}: test\n"), ""),
                    complete.replace(identity, "renamed_recovery"),
                    format!("{complete}{identity}: test\n"),
                    format!("{complete}unregistered_recovery: test\n"),
                ] {
                    assert!(
                        super::super::runner::verify_discovery(&discovery, mutation.as_bytes())
                            .is_err()
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn topology_proofs_require_an_owned_broker_and_reject_census_drift() -> Result<(), String> {
        let check = checks()
            .into_iter()
            .find(|check| check.id == "nats-topology")
            .ok_or("topology proof missing")?;
        assert_eq!(check.profiles, ["m02-qualification"]);
        assert!(matches!(
            check.prerequisites,
            [Requirement::ExecutableOrEnvironment(
                "docker",
                "EDGEAGENT_NATS_SERVER"
            )]
        ));
        assert!(check.command.args.contains(&"jetstream_topology"));
        assert!(!check.command.args.contains(&"--exact"));
        let discovery = check.discovery.ok_or("topology discovery absent")?;
        assert_eq!(discovery.expected.len(), 2);
        let complete = discovery
            .expected
            .iter()
            .map(|name| format!("{name}: test\n"))
            .collect::<String>();
        super::super::runner::verify_discovery(&discovery, complete.as_bytes())?;
        for name in &discovery.expected {
            for mutation in [
                complete.replace(&format!("{name}: test\n"), ""),
                complete.replace(name, "renamed_topology"),
                format!("{complete}{name}: test\n"),
                format!("{complete}unregistered_topology: test\n"),
            ] {
                assert!(
                    super::super::runner::verify_discovery(&discovery, mutation.as_bytes())
                        .is_err()
                );
            }
        }
        Ok(())
    }

    #[test]
    fn process_proofs_require_database_and_owned_broker_and_reject_census_drift()
    -> Result<(), String> {
        let check = checks()
            .into_iter()
            .find(|check| check.id == "event-spine-processes")
            .ok_or("process proof missing")?;
        assert_eq!(check.profiles, ["m02-qualification"]);
        assert!(matches!(
            check.prerequisites,
            [
                Requirement::Environment("EDGEAGENT_POSTGRES_URL"),
                Requirement::ExecutableOrEnvironment("docker", "EDGEAGENT_NATS_SERVER")
            ]
        ));
        assert!(check.command.args.contains(&"process_acceptance"));
        let discovery = check.discovery.ok_or("process discovery absent")?;
        assert_eq!(discovery.expected.len(), 19);
        let complete = discovery
            .expected
            .iter()
            .map(|name| format!("{name}: test\n"))
            .collect::<String>();
        super::super::runner::verify_discovery(&discovery, complete.as_bytes())?;
        for name in &discovery.expected {
            for mutation in [
                complete.replace(&format!("{name}: test\n"), ""),
                complete.replace(name, "renamed_process"),
                format!("{complete}{name}: test\n"),
                format!("{complete}unregistered_process: test\n"),
            ] {
                assert!(
                    super::super::runner::verify_discovery(&discovery, mutation.as_bytes())
                        .is_err()
                );
            }
        }
        Ok(())
    }

    #[test]
    fn broker_restart_requires_exact_authorization_and_docker() -> Result<(), Box<dyn Error>> {
        let check = checks()
            .into_iter()
            .find(|check| check.id == "event-spine-broker")
            .ok_or("broker restart proof")?;
        assert!(check.prerequisites.iter().any(|requirement| matches!(
            requirement,
            Requirement::EnvironmentEquals("EDGEAGENT_NATS_COMPOSE_RESTART", "1")
        )));
        assert!(
            check
                .prerequisites
                .iter()
                .any(|requirement| matches!(requirement, Requirement::Executable("docker")))
        );
        Ok(())
    }
}
