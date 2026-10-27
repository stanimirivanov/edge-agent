use super::*;

#[test]
fn stream_recovery_routes_durable_and_pure_proofs() -> Result<(), String> {
    for path in [
        "crates/market-data-application/src/streaming/recovery.rs",
        "crates/market-data-postgres/src/streaming/recovery.rs",
        "crates/market-data-postgres/migrations/0009_stream_recovery.sql",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(
            advice
                .guides
                .contains("docs/development/stream-supervision.md")
        );
        assert!(advice.checks.contains("streaming-publication-postgres"));
    }
    Ok(())
}

#[test]
fn supervision_routes_bounded_offline_proofs() -> Result<(), String> {
    for path in [
        "crates/market-data-application/src/supervision.rs",
        "crates/market-data-application/tests/supervision.rs",
        "docs/development/stream-supervision.md",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(
            advice
                .guides
                .contains("docs/development/stream-supervision.md")
        );
        assert!(advice.checks.contains("stream-supervision-contracts"));
    }
    Ok(())
}

#[test]
fn iex_routes_offline_protocol_proofs() -> Result<(), String> {
    for path in [
        "crates/market-data-http/src/alpaca_iex.rs",
        "crates/market-data-http/src/alpaca_iex/protocol.rs",
        "crates/market-data-http/src/alpaca_iex/tests/peer.rs",
        "docs/development/alpaca-iex.md",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(advice.guides.contains("docs/development/alpaca-iex.md"));
        assert!(advice.checks.contains("alpaca-iex-conformance"));
    }
    Ok(())
}

#[test]
fn wss_transport_routes_bounded_local_protocol_proofs() -> Result<(), String> {
    for path in [
        "crates/market-data-http/src/wss.rs",
        "crates/market-data-http/src/wss/connection.rs",
        "crates/market-data-http/src/wss/tests.rs",
        "docs/development/streaming-transport.md",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(
            advice
                .guides
                .contains("docs/development/streaming-transport.md")
        );
        assert!(advice.checks.contains("wss-transport-contracts"));
    }
    Ok(())
}

#[test]
fn streaming_routes_authority_and_durable_account_proofs() -> Result<(), String> {
    for path in [
        "crates/market-data-application/src/streaming/authority.rs",
        "crates/market-data-application/tests/streaming.rs",
        "crates/market-data-application/tests/support/streaming.rs",
        "crates/market-data-postgres/src/streaming/worker.rs",
        "crates/market-data-postgres/tests/streaming.rs",
        "crates/market-data-postgres/migrations/0007_streaming_sessions.sql",
        "docs/development/streaming-sessions.md",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(
            advice
                .guides
                .contains("docs/development/streaming-sessions.md")
        );
        assert!(advice.checks.contains("streaming-session-contracts"));
        assert!(advice.checks.contains("streaming-session-postgres"));
        if path.contains("market-data-postgres") {
            assert!(advice.checks.contains("sqlx-metadata"));
        }
    }
    Ok(())
}

#[test]
fn qualification_harness_routes_offline_supervision_tests() -> Result<(), String> {
    for path in [
        "scripts/qualify_daily.py",
        "tests/test_qualify_daily.py",
        "docs/development/qualification-harness.md",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(
            advice
                .guides
                .contains("docs/development/qualification-harness.md")
        );
        assert!(advice.checks.contains("harness-tests"));
    }
    Ok(())
}

#[test]
fn qualification_runtime_routes_both_protocols_and_owner_proofs() -> Result<(), String> {
    let advice = advise(&["services/market-data/src/qualification/worker.rs".into()])?;
    assert!(
        advice
            .guides
            .contains("docs/development/qualification-runtime.md")
    );
    for id in [
        "qualification-owner-postgres",
        "twelve-data-conformance",
        "alpaca-conformance",
        "native-publication-postgres",
    ] {
        assert!(advice.checks.contains(id));
    }
    Ok(())
}

#[test]
fn owner_control_paths_route_real_role_proofs_and_metadata() -> Result<(), String> {
    for path in [
        "crates/market-data-postgres/src/qualification.rs",
        "crates/market-data-postgres/src/ledger.rs",
        "crates/market-data-postgres/src/terminal.rs",
        "crates/market-data-postgres/src/retention.rs",
        "crates/market-data-postgres/migrations/0005_qualification_owner.sql",
        "crates/market-data-postgres/tests/qualification.rs",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(advice.checks.contains("qualification-owner-postgres"));
        assert!(advice.checks.contains("sqlx-metadata"));
    }
    Ok(())
}

#[test]
fn native_storage_and_publication_route_their_protected_proofs() -> Result<(), String> {
    for path in [
        "crates/market-data-application/src/terminal/native.rs",
        "crates/market-data-application/src/storage.rs",
        "crates/market-data-application/src/retention.rs",
        "crates/market-data-postgres/tests/native.rs",
        "crates/market-data-object-store/src/store.rs",
    ] {
        let advice = advise(&[path.to_owned()])?;
        assert!(advice.checks.contains("native-publication-postgres"));
        if !path.contains("market-data-postgres") {
            assert!(advice.checks.contains("native-publication-contracts"));
        }
    }
    Ok(())
}

#[test]
fn native_quality_and_contract_paths_select_protected_acceptance() -> Result<(), String> {
    for path in [
        "crates/market-data-domain/src/daily/native.rs",
        "crates/market-data-application/src/native_evidence.rs",
        "crates/market-data-application/tests/native_acceptance/fixture.rs",
        "crates/contracts/src/evidence/native_daily.rs",
        "docs/development/native-daily-acceptance.md",
    ] {
        let advice = advise(&[path.to_owned()])?;
        assert!(
            advice.checks.contains("native-daily-acceptance"),
            "missing native proof for {path}"
        );
        assert!(
            advice
                .guides
                .contains("docs/development/native-daily-acceptance.md")
        );
    }
    Ok(())
}
#[test]
fn alpaca_sources_route_protected_precise_protocol_proofs() -> Result<(), String> {
    for path in [
        "crates/market-data-http/src/alpaca.rs",
        "crates/market-data-application/src/precise_daily.rs",
        "crates/market-data-application/src/precise_daily/encoding.rs",
        "docs/development/alpaca.md",
    ] {
        assert!(
            advise(&[path.into()])?
                .checks
                .contains("alpaca-conformance")
        );
    }
    Ok(())
}
#[test]
fn native_daily_adapter_routes_protected_protocol_evidence() -> Result<(), String> {
    for path in [
        "crates/market-data-http/src/twelve_data.rs",
        "crates/market-data-application/src/date_only_source.rs",
        "docs/development/twelve-data.md",
    ] {
        assert!(
            advise(&[path.into()])?
                .checks
                .contains("twelve-data-conformance")
        );
    }
    Ok(())
}

#[test]
fn retention_routes_require_model_storage_and_database_evidence() -> Result<(), String> {
    for path in [
        "crates/market-data-application/src/retention.rs",
        "docs/development/evidence-retention.md",
    ] {
        let advice = advise(&[path.into()])?;
        for check in [
            "evidence-retention-contracts",
            "evidence-retention-postgres",
            "evidence-storage-contracts",
        ] {
            assert!(advice.checks.contains(check));
        }
        assert!(
            advice
                .guides
                .contains("docs/development/evidence-retention.md")
        );
    }
    Ok(())
}

#[test]
fn selection_and_early_routing_sources_route_protected_proofs() -> Result<(), String> {
    for path in [
        "crates/market-data-application/src/selection.rs",
        "crates/market-data-application/src/selection/router.rs",
        "crates/market-data-replay/src/routing.rs",
        "crates/market-data-replay/tests/selection.rs",
        "docs/development/instrument-selection.md",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(advice.checks.contains("instrument-selection-routing"));
        assert!(advice.checks.contains("offline-source-isolation"));
        assert!(
            advice
                .guides
                .contains("docs/development/instrument-selection.md")
        );
    }
    Ok(())
}

#[test]
fn terminal_publication_routes_model_and_atomic_postgres_proofs() -> Result<(), String> {
    for path in [
        "crates/market-data-postgres/src/terminal.rs",
        "crates/market-data-application/src/terminal/receipt.rs",
        "crates/market-data-application/tests/terminal.rs",
        "docs/development/acquisition-publication.md",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(
            advice
                .guides
                .contains("docs/development/acquisition-publication.md")
        );
        assert!(advice.checks.contains("acquisition-terminal-contracts"));
        assert!(advice.checks.contains("acquisition-terminal-postgres"));
    }
    Ok(())
}

#[test]
fn acquisition_ledger_routes_model_and_postgres_proofs() -> Result<(), String> {
    for path in [
        "crates/market-data-postgres/src/ledger.rs",
        "crates/market-data-application/src/admission/codec.rs",
        "crates/market-data-application/src/budget/port.rs",
        "crates/market-data-application/tests/admission.rs",
        "docs/development/acquisition-ledger.md",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(advice.checks.contains("acquisition-budget-contracts"));
        assert!(advice.checks.contains("acquisition-ledger-postgres"));
        assert!(
            advice
                .guides
                .contains("docs/development/acquisition-ledger.md")
        );
    }
    Ok(())
}

#[test]
fn evidence_storage_routes_protected_integrity_and_recovery_proofs() -> Result<(), String> {
    for path in [
        "crates/market-data-object-store/src/store.rs",
        "crates/market-data-application/src/storage.rs",
        "docs/development/evidence-storage.md",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(advice.checks.contains("evidence-storage-contracts"));
        assert!(
            advice
                .guides
                .contains("docs/development/evidence-storage.md")
        );
    }
    Ok(())
}

#[test]
fn provider_transport_routes_exact_http_contracts() -> Result<(), String> {
    for path in [
        "crates/market-data-http/src/transport.rs",
        "crates/market-data-http/src/json.rs",
        "crates/market-data-application/src/dispatch.rs",
        "docs/development/provider-http.md",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(advice.checks.contains("metered-provider-dispatch"));
        assert!(advice.guides.contains("docs/development/provider-http.md"));
    }
    Ok(())
}

#[test]
fn evidence_sources_schemas_and_fixtures_route_protected_contracts() -> Result<(), String> {
    for path in [
        "crates/contracts/src/evidence/date_only.rs",
        "crates/market-data-domain/src/date_only.rs",
        "crates/market-data-application/src/date_only.rs",
        "crates/market-data-application/tests/date_only.rs",
        "crates/contracts/fixtures/evidence-date-only-v1/time.json",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(advice.checks.contains("date-only-evidence"));
    }
    for path in [
        "crates/contracts/src/evidence/artifact.rs",
        "crates/contracts/schemas/evidence/daily-v1.json",
        "crates/contracts/fixtures/evidence-v1/daily.json",
        "crates/event-spine-conformance/tests/evidence_contracts.rs",
        "docs/development/evidence-artifacts.md",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(advice.checks.contains("evidence-contracts"));
        assert!(
            advice
                .guides
                .contains("docs/development/evidence-artifacts.md")
        );
    }
    let advice = advise(&["crates/market-data-application/src/evidence.rs".into()])?;
    assert!(advice.checks.contains("evidence-freezing"));
    Ok(())
}

#[test]
fn acquisition_contracts_route_reviewed_authority_conformance() -> Result<(), String> {
    for path in [
        "crates/market-data-application/src/authority.rs",
        "crates/market-data-application/tests/contracts.rs",
        "docs/development/market-data-acquisition.md",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(advice.checks.contains("market-data-conformance"));
        assert!(
            advice
                .guides
                .contains("docs/development/market-data-acquisition.md")
        );
    }
    Ok(())
}

#[test]
fn daily_evidence_changes_route_the_protected_quality_gate() -> Result<(), String> {
    for path in [
        "crates/market-data-domain/src/daily.rs",
        "crates/market-data-domain/tests/daily.rs",
        "docs/development/market-data.md",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(advice.guides.contains("docs/development/market-data.md"));
        assert!(advice.checks.contains("market-data-quality"));
    }
    Ok(())
}

#[test]
fn audit_sources_require_replay_and_owner_process_proofs() -> Result<(), String> {
    for path in [
        "crates/audit-domain/src/lib.rs",
        "crates/contracts/src/audit.rs",
        "crates/simulation-postgres/src/audit/orders.rs",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(advice.guides.contains("docs/development/account-audit.md"));
        assert!(advice.checks.contains("account-audit-contracts"));
        if path.starts_with("crates/audit-domain/") {
            assert!(advice.checks.contains("frozen-evaluation-contracts"));
            assert!(
                advice
                    .guides
                    .contains("docs/development/frozen-evaluation.md")
            );
        }
        assert!(advice.checks.contains("account-admission-processes"));
    }
    Ok(())
}

#[test]
fn analytics_sources_and_references_require_independent_numerical_qualification()
-> Result<(), String> {
    for path in [
        "crates/simulation-domain/src/analytics/solver.rs",
        "crates/simulation-domain/tests/analytics/references.rs",
        "scripts/generate_option_analytics_references.py",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(
            advice
                .guides
                .contains("docs/development/option-analytics.md")
        );
        assert!(advice.checks.contains("option-analytics-contracts"));
    }
    Ok(())
}

#[test]
fn settlement_boundaries_route_the_financial_process_and_schema_gates() -> Result<(), String> {
    for path in [
        "crates/simulation-postgres/src/settlement/handling.rs",
        "crates/simulation-domain/src/settlement/allocation.rs",
        "crates/contracts/src/settlement.rs",
        "crates/contracts/schemas/simulation/settle-account-v1.json",
        "crates/contracts/schemas/simulation/settlement-decided-v1.json",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(
            advice
                .guides
                .contains("docs/development/option-settlement.md")
        );
        assert!(advice.checks.contains("account-admission-processes"));
        assert!(advice.checks.contains("simulation-schema-contracts"));
        if path.starts_with("crates/simulation-postgres/") {
            assert!(advice.checks.contains("sqlx-metadata"));
        }
    }
    Ok(())
}

#[test]
fn account_owner_changes_require_the_independent_process_gate() -> Result<(), String> {
    for path in [
        "crates/simulation-postgres/src/processing.rs",
        "crates/event-spine-conformance/src/accounts.rs",
        "crates/event-spine-conformance/src/bin/account-worker.rs",
        "crates/contracts/src/simulation.rs",
        "crates/contracts/schemas/simulation/submit-order-v1.json",
    ] {
        let advice = advise(&[path.into()])?;
        assert!(advice.checks.contains("account-admission-processes"));
        assert!(
            advice
                .guides
                .contains("docs/development/account-admission.md")
        );
    }
    Ok(())
}

#[test]
fn registry_and_adapter_changes_route_the_isolated_topology_proof() -> Result<(), String> {
    for path in [
        "crates/contracts/src/routing.rs",
        "crates/messaging-nats/src/topology.rs",
        "crates/event-spine-conformance/tests/jetstream_topology.rs",
    ] {
        assert!(advise(&[path.into()])?.checks.contains("nats-topology"));
    }
    Ok(())
}

#[test]
fn acceptance_schema_and_shared_envelope_changes_route_schema_contracts() -> Result<(), String> {
    for path in [
        "crates/event-spine-conformance/src/contracts.rs",
        "crates/event-spine-conformance/schemas/v1/apply-probe.json",
        "crates/event-spine-conformance/fixtures/baseline/v1/apply-probe.schema.json",
        "crates/event-spine-conformance/tests/schema_contracts.rs",
        "crates/contracts/src/messaging.rs",
    ] {
        assert!(
            advise(&[path.to_owned()])?
                .checks
                .contains("message-schema-contracts"),
            "{path}"
        );
    }
    Ok(())
}

#[test]
fn relay_configuration_and_runtime_changes_route_protected_policy_contracts() -> Result<(), String>
{
    for path in [
        "crates/outbox-relay/src/policy.rs",
        "crates/outbox-relay/src/error.rs",
        "crates/outbox-relay/src/retry.rs",
        "crates/outbox-relay/src/resolution.rs",
        "crates/outbox-relay/tests/policy_contracts.rs",
        "crates/outbox-postgres/src/relay.rs",
        "crates/messaging/src/outbox.rs",
    ] {
        let advice = advise(&[path.to_owned()])?;
        assert!(advice.checks.contains("relay-policy-contracts"), "{path}");
    }
    Ok(())
}

#[test]
fn relay_recovery_changes_route_the_budget_and_real_producer_proofs() -> Result<(), String> {
    for path in [
        "crates/outbox-relay/src/coordinator.rs",
        "crates/outbox-relay/tests/postgres_relay/recovery.rs",
        "crates/outbox-postgres/src/relay.rs",
        "deploy/local/compose.yaml",
    ] {
        let advice = advise(&[path.to_owned()])?;
        assert!(advice.checks.contains("relay-recovery-budget"), "{path}");
        assert!(advice.checks.contains("event-spine-producer"), "{path}");
    }
    for path in [
        "crates/messaging-nats/src/publisher.rs",
        "crates/inbox-handler/tests/event_spine/publication_recovery.rs",
        "crates/contracts/src/lib.rs",
    ] {
        assert!(
            advise(&[path.to_owned()])?
                .checks
                .contains("event-spine-producer"),
            "{path}"
        );
    }
    Ok(())
}

#[test]
fn relay_and_telemetry_changes_route_the_protected_observation_contract() -> Result<(), String> {
    for path in [
        "crates/outbox-relay/src/coordinator.rs",
        "crates/outbox-relay/src/observation.rs",
        "crates/outbox-relay/tests/telemetry.rs",
        "crates/telemetry/src/lib.rs",
    ] {
        let advice = advise(&[path.to_owned()])?;
        assert!(
            advice.checks.contains("relay-observation-contracts"),
            "{path}"
        );
        assert!(
            advice
                .guides
                .contains("docs/development/event-spine-telemetry.md")
        );
    }
    Ok(())
}

#[test]
fn query_policy_sensor_routes_its_canonical_sqlx_guide() -> Result<(), String> {
    for path in [
        "scripts/verify_sqlx_query_policy.py",
        "tests/test_verify_sqlx_query_policy.py",
    ] {
        let advice = advise(&[path.to_owned()])?;
        assert!(advice.guides.contains("docs/development/sqlx-metadata.md"));
        assert_eq!(advice.mandatory, DEFAULT_COMMAND);
    }
    Ok(())
}

#[test]
fn default_acceptance_is_mandatory_for_every_route_and_empty_input() -> Result<(), String> {
    for paths in [
        vec![],
        vec!["deleted/unknown.rs".to_owned()],
        vec!["crates/inbox-postgres/src/lib.rs".to_owned()],
    ] {
        assert_eq!(advise(&paths)?.mandatory, "cargo xtask verify");
    }
    Ok(())
}

#[test]
fn unknown_and_deleted_paths_keep_the_canonical_fallback() -> Result<(), String> {
    let advice = advise(&["deleted/unknown.rs".to_owned()])?;
    assert_eq!(
        advice.guides,
        BTreeSet::from(["docs/README.md", "docs/development/harness.md"])
    );
    assert!(advice.checks.is_empty());
    Ok(())
}

#[test]
fn separators_order_and_duplicates_do_not_change_the_union() -> Result<(), String> {
    let first = advise(&[
        "crates\\inbox-postgres\\migrations\\deleted.sql".to_owned(),
        "Dockerfile".to_owned(),
        "Dockerfile".to_owned(),
    ])?;
    let second = advise(&[
        "Dockerfile".to_owned(),
        "crates/inbox-postgres/migrations/deleted.sql".to_owned(),
    ])?;
    assert_eq!(first, second);
    assert!(first.checks.contains("sqlx-metadata"));
    assert!(first.checks.contains("inbox-invariants"));
    assert!(first.checks.contains("image-smoke"));
    assert!(first.checks.contains("dependency-policy"));
    assert!(
        first
            .guides
            .contains("docs/development/engineering-standards.md")
    );
    assert!(first.guides.contains("docs/development/postgres-inbox.md"));
    Ok(())
}

#[test]
fn prefixes_do_not_capture_similarly_named_crates_or_files() -> Result<(), String> {
    let advice = advise(&[
        "crates/inbox-postgres-extra/src/lib.rs".to_owned(),
        "Dockerfile.backup".to_owned(),
        "my-Cargo.toml".to_owned(),
    ])?;
    assert!(advice.checks.is_empty());
    assert!(!advice.guides.contains("docs/development/postgres-inbox.md"));
    Ok(())
}

#[test]
fn service_and_portable_contract_changes_route_related_proofs() -> Result<(), String> {
    let advice = advise(&[
        "services/gateway/src/main.rs".to_owned(),
        "crates/messaging/src/consumer.rs".to_owned(),
    ])?;
    for check in [
        "image-smoke",
        "nats-publish",
        "nats-consume",
        "nats-no-stream",
        "nats-size-rejection",
        "nats-header-only",
        "nats-publish-oversize",
        "nats-settlement-response",
        "nats-halted-worker",
        "nats-closed-intake",
        "nats-intake-bound",
        "nats-fixture-cleanup",
        "outbox-fencing",
        "outbox-relay-boundaries",
        "handler-ordering",
        "event-spine-broker",
    ] {
        assert!(advice.checks.contains(check), "missing {check}");
    }
    Ok(())
}

#[test]
fn each_experiment_routes_its_own_advisory_and_proof_checks() -> Result<(), String> {
    for (package, label) in [
        ("event-spine-proof", "event-spine"),
        ("durable-execution", "duroxide"),
        ("dbos-proof", "dbos"),
        ("temporal-proof", "temporal"),
        ("flawless-proof", "flawless"),
    ] {
        let advice = advise(&[format!("experiments/{package}/Cargo.lock")])?;
        for kind in ["dependency-policy", "format", "clippy", "test"] {
            assert!(
                advice
                    .checks
                    .contains(format!("m08-{label}-{kind}").as_str())
            );
        }
        assert_eq!(advice.checks.len(), 4);
        assert!(!advice.checks.contains("dependency-policy"));
    }
    Ok(())
}

#[test]
fn maintenance_and_scheduled_audit_changes_route_every_resolution_root() -> Result<(), String> {
    for path in [
        ".github/dependabot.yml",
        ".github/workflows/dependency-audit.yml",
        "experiments/deny.toml",
    ] {
        let advice = advise(&[path.to_owned()])?;
        for check in ALL_ADVISORY_CHECKS {
            assert!(advice.checks.contains(check), "missing {check} for {path}");
        }
        assert!(advice.guides.contains("docs/development/supply-chain.md"));
    }
    Ok(())
}

#[test]
fn event_spine_path_dependencies_route_the_separate_proof_lock() -> Result<(), String> {
    for path in [
        "Cargo.toml",
        "crates/contracts/src/envelope.rs",
        "crates/inbox-handler/Cargo.toml",
        "crates/inbox-postgres/src/lib.rs",
        "crates/messaging/src/lib.rs",
        "crates/messaging-nats/src/lib.rs",
        "crates/telemetry/src/lib.rs",
    ] {
        let advice = advise(&[path.to_owned()])?;
        for check in EVENT_SPINE_PROOF_CHECKS {
            assert!(advice.checks.contains(check), "missing {check} for {path}");
        }
    }
    Ok(())
}

#[test]
fn workspace_cargo_routes_root_supply_chain() -> Result<(), String> {
    for path in [
        "Cargo.toml",
        "Cargo.lock",
        "crates/contracts/Cargo.toml",
        "services/gateway/Cargo.toml",
    ] {
        let advice = advise(&[path.to_owned()])?;
        assert!(advice.checks.contains("dependency-policy"));
        assert!(
            advice
                .guides
                .contains("docs/architecture/dependency-rules.md")
        );
    }
    Ok(())
}

#[test]
fn sqlx_metadata_and_local_profile_route_their_specialized_checks() -> Result<(), String> {
    let sqlx = advise(&[".sqlx/query-deleted.json".to_owned()])?;
    assert_eq!(sqlx.checks, BTreeSet::from(["sqlx-metadata"]));
    let local = advise(&["deploy/local/compose.yaml".to_owned()])?;
    for check in [
        "nats-oversize",
        "outbox-conformance",
        "outbox-fencing",
        "outbox-relay-boundaries",
        "inbox-invariants",
        "atomic-enqueue",
        "relay-conformance",
        "relay-recovery-budget",
        "event-spine-producer",
        "event-spine-client",
        "event-spine-broker",
    ] {
        assert!(local.checks.contains(check));
    }
    Ok(())
}

#[test]
fn absolute_drive_dot_traversal_and_control_paths_are_rejected() {
    for path in [
        "",
        "/tmp/file.rs",
        "\\\\server\\share\\file.rs",
        "C:\\Source\\file.rs",
        "C:file.rs",
        "../file.rs",
        "crates/../file.rs",
        "./Cargo.toml",
        "crates/./lib.rs",
        "crates//lib.rs",
        "crates/",
        "crates/lib.rs\n",
        "crates/\0.rs",
    ] {
        assert!(advise(&[path.to_owned()]).is_err(), "accepted {path:?}");
    }
}

#[test]
fn renamed_guide_and_missing_check_fail_closed() {
    let ids = ROUTES
        .iter()
        .flat_map(|route| route.checks.iter().copied())
        .collect();
    let missing_guide =
        validate_references(&ids, |guide| guide != "docs/development/postgres-inbox.md");
    assert!(
        matches!(missing_guide, Err(error) if error.contains("routing guide missing: docs/development/postgres-inbox.md"))
    );

    let mut missing_ids = ids.clone();
    missing_ids.remove("image-smoke");
    let missing_check = validate_references(&missing_ids, |_| true);
    assert!(
        matches!(missing_check, Err(error) if error.contains("routing check missing from canonical inventory: image-smoke"))
    );
}

#[test]
fn relay_boundaries_follow_all_outbox_paths_and_fail_if_unregistered() -> Result<(), String> {
    for path in [
        "crates/outbox-postgres/src/relay.rs",
        "crates/outbox-postgres/tests/postgres_relay_boundaries.rs",
        "crates/outbox-postgres/tests/support/schema.rs",
        "crates/outbox-postgres/tests/recovery/cancellation.rs",
        "crates/outbox-relay/tests/postgres_relay.rs",
        "crates/outbox-relay/src/lib.rs",
        "crates/messaging/src/outbox.rs",
        "deploy/local/compose.yaml",
    ] {
        let advice = advise(&[path.to_owned()])?;
        assert!(
            advice.checks.contains("outbox-relay-boundaries"),
            "missing relay boundary route for {path}"
        );
        assert!(
            advice.checks.contains("outbox-recovery"),
            "missing recovery route for {path}"
        );
    }
    let mut ids = inventory::checks()
        .into_iter()
        .map(|check| check.id)
        .collect::<BTreeSet<_>>();
    ids.remove("outbox-relay-boundaries");
    assert!(matches!(
        validate_references(&ids, |_| true),
        Err(error) if error.contains("routing check missing from canonical inventory: outbox-relay-boundaries")
    ));
    ids.insert("outbox-relay-boundaries");
    ids.remove("outbox-recovery");
    assert!(matches!(
        validate_references(&ids, |_| true),
        Err(error) if error.contains("routing check missing from canonical inventory: outbox-recovery")
    ));
    Ok(())
}

#[test]
fn security_paths_route_actual_scanner_proofs_without_waiving_default_acceptance()
-> Result<(), String> {
    for path in [
        ".github/workflows/repository-quality.yml",
        ".gitleaks.toml",
        "supply-chain/security-tools.toml",
        "scripts/verify_secrets.py",
        "scripts/install_gitleaks.py",
    ] {
        let advice = advise(&[path.to_owned()])?;
        assert_eq!(advice.mandatory, DEFAULT_COMMAND);
        assert!(advice.guides.contains("docs/development/ci-security.md"));
        assert!(advice.checks.contains("secret-scanner-contracts"));
        assert!(advice.checks.contains("secret-scan"));
    }
    Ok(())
}

#[test]
fn every_declared_guide_and_check_exists_in_the_repository() -> Result<(), String> {
    validate()
}

#[test]
fn stream_segment_changes_route_codec_producer_and_storage_proofs() -> Result<(), String> {
    for path in [
        "crates/market-data-application/src/evidence.rs",
        "crates/market-data-object-store/src/store.rs",
        "crates/contracts/src/evidence/stream.rs",
        "crates/contracts/src/evidence/stream/validate.rs",
        "crates/market-data-application/src/streaming/segment.rs",
        "crates/market-data-application/tests/stream_segments.rs",
        "crates/market-data-application/tests/support/segment.rs",
        "crates/market-data-application/src/storage.rs",
        "crates/market-data-object-store/src/store/segment.rs",
        "crates/market-data-object-store/tests/stream_segments.rs",
        "docs/development/streaming-segments.md",
    ] {
        let advice = advise(&[path.to_owned()])?;
        assert_eq!(advice.mandatory, DEFAULT_COMMAND);
        assert!(
            advice.checks.contains("streaming-segment-contracts"),
            "missing segment proof for {path}"
        );
        assert!(
            advice
                .guides
                .contains("docs/development/streaming-segments.md")
        );
    }
    Ok(())
}

#[test]
fn market_data_changes_require_owned_recovery_proofs() -> Result<(), String> {
    for path in [
        "services/market-data/src/offline/handling.rs",
        "services/market-data/tests/recovery/postgres.rs",
    ] {
        let advice = advise(&[path.to_owned()])?;
        assert_eq!(advice.mandatory, DEFAULT_COMMAND);
        assert!(advice.checks.contains("offline-worker-recovery"));
    }
    Ok(())
}

#[test]
fn process_composition_and_adapters_route_the_protected_process_proof() -> Result<(), String> {
    for path in [
        "crates/event-spine-conformance/src/worker/persistence.rs",
        "crates/inbox-postgres/src/handler.rs",
        "crates/messaging-nats/src/lib.rs",
        "crates/outbox-relay/src/coordinator.rs",
        "docs/development/process-acceptance.md",
        "docs/development/quarantine-replay.md",
        "docs/development/event-spine-telemetry.md",
        "crates/telemetry/src/lib.rs",
    ] {
        let advice = advise(&[path.to_owned()])?;
        assert_eq!(advice.mandatory, DEFAULT_COMMAND);
        assert!(
            advice.checks.contains("event-spine-processes"),
            "missing process proof for {path}"
        );
    }
    let advice = advise(&["crates/event-spine-conformance/src/worker/persistence.rs".to_owned()])?;
    assert!(
        advice
            .guides
            .contains("docs/development/process-acceptance.md")
    );
    assert!(advice.checks.contains("sqlx-metadata"));
    Ok(())
}
#[test]
fn qualification_routes_include_protected_preflight_and_shared_canonical_consumers()
-> Result<(), String> {
    for path in [
        "crates/market-data-application/src/activation.rs",
        "crates/market-data-application/src/activation/profile.rs",
        "crates/market-data-application/tests/activation/fixture.rs",
        "crates/market-data-application/src/budget/daily.rs",
        "docs/development/qualification-preflight.md",
        "crates/market-data-application/src/canonical.rs",
    ] {
        let advice = advise(&[path.to_owned()])?;
        assert!(advice.checks.contains("qualification-preflight"));
        if path.ends_with("canonical.rs") {
            assert!(advice.checks.contains("native-publication-contracts"));
            assert!(advice.checks.contains("acquisition-terminal-contracts"));
        }
    }
    Ok(())
}
#[test]
fn stream_publication_changes_route_atomic_and_storage_proofs() -> Result<(), String> {
    for path in [
        "crates/market-data-postgres/migrations/0008_stream_publication.sql",
        "crates/market-data-postgres/src/streaming/publication.rs",
        "crates/market-data-postgres/src/streaming/lifecycle.rs",
        "crates/market-data-postgres/tests/stream_publication.rs",
        "crates/market-data-application/src/streaming/publication.rs",
        "crates/market-data-application/src/streaming/publication/validation.rs",
    ] {
        let advice = super::advise(&[path.to_owned()])?;
        assert!(advice.checks.contains("streaming-publication-postgres"));
    }
    Ok(())
}
