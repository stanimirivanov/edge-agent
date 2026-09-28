# Event-spine recovery conformance

## TL;DR

- Two opt-in tests compose the real PostgreSQL outbox, bounded relay, JetStream
  publisher and consumer, transactional inbox handler, and PostgreSQL inbox.
- Both tests abandon the first delivery without acknowledgement. One discards
  every client-side handle; the other restarts the isolated NATS server and
  recovers a file-backed stream and durable consumer.
- Acceptance requires one published outbox record, one inbox identity, one
  domain effect, a stable transport delivery key, and no pending acknowledgement.
- The local-platform CI job runs this test against isolated NATS and PostgreSQL
  services; credential-free workspace tests compile it but keep it ignored.

## Recovery scenario

The conformance test exercises one synthetic `SubmitDryRunOrder` command through
the complete M02 data and control path:

1. Apply every outbox and inbox migration in a process-scoped PostgreSQL schema.
2. Enqueue the validated CloudEvents envelope inside a committed transaction.
3. Run `relay_once` with the real JetStream publisher and confirm broker persistence.
4. Pull delivery attempt one through the real JetStream consumer.
5. Drop the delivery and all first-client consumer handles without settlement.
6. Either wait beyond the durable consumer's acknowledgement deadline or restart
   the isolated NATS Compose service with its persistent volume intact.
7. Poll for a new NATS connection, recover the named stream and durable consumer,
   and receive the same stream message as a later delivery attempt.
8. Run `handle_once`, committing inbox identity and a synthetic domain effect
   before confirmed acknowledgement.
9. Verify the broker has no pending work and PostgreSQL contains exactly one
   outbox publication, inbox identity, and domain transition.

The abandoned attempt models a process stopping after receipt but before local
commit or broker acknowledgement. The stable JetStream stream/sequence key must
survive reconnection while the delivery-attempt count increases. No test helper
short-circuits publication, consumption, settlement, or PostgreSQL transactions.

## Verification

Run the isolated conformance test with both local dependencies available:

```text
EDGEAGENT_NATS_URL=nats://127.0.0.1:4222 EDGEAGENT_POSTGRES_URL=postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent cargo test --locked -p edgeagent-inbox-handler --test event_spine -- --ignored --exact durable_command_survives_client_restart_with_one_domain_transition
```

Run the broker-restart case only against the repository's isolated local Compose
profile. The explicit opt-in prevents an ignored test from restarting a shared
broker accidentally:

```text
EDGEAGENT_NATS_COMPOSE_RESTART=1 EDGEAGENT_NATS_URL=nats://127.0.0.1:4222 EDGEAGENT_POSTGRES_URL=postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent cargo test --locked -p edgeagent-inbox-handler --test event_spine -- --ignored --exact durable_command_survives_broker_restart_with_one_domain_transition
```

The client-restart case creates a memory-backed stream. The broker-restart case
creates a file-backed stream and invokes the fixed `docker compose ... restart
nats` command for this repository. Both create and remove a process-scoped
PostgreSQL schema. A failure before cleanup can leave resources behind; the CI
job always destroys the complete local Compose profile and its volumes afterward.

## Current limitations

These tests prove application-client and NATS server restart recovery, including
disk-backed stream and durable-consumer state. They do not restart PostgreSQL and
use one sequential synthetic command, so they do not claim load, ordering across
multiple partitions, schema compatibility, telemetry export, concurrent
backpressure, or production service lifecycle coverage.
