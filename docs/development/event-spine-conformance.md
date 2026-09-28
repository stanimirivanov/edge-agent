# Event-spine recovery conformance

## TL;DR

- One opt-in test composes the real PostgreSQL outbox, bounded relay, JetStream
  publisher and consumer, transactional inbox handler, and PostgreSQL inbox.
- The test abandons the first delivery without acknowledgement, discards every
  client-side consumer handle, reconnects, and processes the redelivery.
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
6. Wait beyond the durable consumer's acknowledgement deadline.
7. Connect a new NATS client, recover the named stream and durable consumer, and
   receive the same stream message as a later delivery attempt.
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

The test creates and removes its own memory-backed JetStream stream and
process-scoped PostgreSQL schema. Run it only against isolated development or
CI services. A failure before cleanup can leave those resources behind; the CI
job always destroys the complete local Compose profile afterward.

## Current limitations

This test proves application-client restart recovery, not restart of the NATS
server or PostgreSQL server. It uses one sequential synthetic command and does
not claim load, ordering across multiple partitions, schema compatibility,
telemetry export, or production service lifecycle coverage. Broker restart,
disk-backed stream recovery, concurrent backpressure, and deployable service
composition remain separate increments.
