use super::*;

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
