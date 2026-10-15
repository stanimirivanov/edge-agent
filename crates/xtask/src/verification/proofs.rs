//! Opt-in adapter proofs and their protected ignored-test identities.

use super::{Check, CommandSpec, Discovery, Program, Requirement};

const NATS: &[Requirement] = &[Requirement::Environment("EDGEAGENT_NATS_URL")];
const POSTGRES: &[Requirement] = &[Requirement::Environment("EDGEAGENT_POSTGRES_URL")];
const EVENT_SPINE: &[Requirement] = &[
    Requirement::Environment("EDGEAGENT_NATS_URL"),
    Requirement::Environment("EDGEAGENT_POSTGRES_URL"),
];
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
    identities: &["relay_bounds_retry_and_quarantines_terminal_failures"],
};
const SPINE: Target = Target {
    package: "edgeagent-inbox-handler",
    name: "event_spine",
    identities: &[
        "durable_command_survives_client_restart_with_one_domain_transition",
        "durable_command_survives_broker_restart_with_one_domain_transition",
    ],
};

pub(super) fn checks() -> Vec<Check> {
    vec![
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
    let args = vec![
        "test",
        "--locked",
        "-p",
        target.package,
        "--test",
        target.name,
        "--",
        "--ignored",
    ];
    let mut discovery_args = args.clone();
    discovery_args.push("--list");
    Check {
        id,
        tier: "T3",
        profiles: &["local-platform"],
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
        PUBLISH,
        CONSUME,
        NATS_BOUNDARIES,
        NATS_LIFECYCLE,
        OUTBOX,
        OUTBOX_RELAY_BOUNDARIES,
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
    use super::*;
    use std::{collections::BTreeSet, error::Error};

    #[test]
    fn proofs_cover_each_external_ignored_identity_without_duplicate_execution()
    -> Result<(), Box<dyn Error>> {
        let mut executed = Vec::new();
        let checks = checks();
        assert_eq!(checks.len(), 23);
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
        assert_eq!(inventory.len(), 31);
        assert_eq!(inventory.iter().collect::<BTreeSet<_>>().len(), 31);
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
        let check = checks()
            .into_iter()
            .find(|check| check.id == "outbox-relay-boundaries")
            .ok_or("relay boundary proof missing")?;
        assert_eq!(check.profiles, ["local-platform"]);
        assert!(matches!(
            check.prerequisites,
            [Requirement::Environment("EDGEAGENT_POSTGRES_URL")]
        ));
        assert!(check.command.args.contains(&"postgres_relay_boundaries"));
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
