# Transactional inbox handler

## TL;DR

- `edgeagent-inbox-handler` processes and settles one delivery at a time.
- It decodes and routes untrusted bytes before domain work, records the inbox
  identity, and invokes a service-owned handler in one PostgreSQL transaction.
- A successful first delivery commits before acknowledgement; a duplicate skips
  domain work and acknowledges after its no-op transaction commits.
- Transient handler failures roll back and request deterministic delayed
  redelivery. Permanent or exhausted handler failures commit exact quarantine
  evidence before terminal settlement.
- Database and settlement ambiguity never become terminal success. Redelivery
  resolves committed work through inbox or quarantine idempotency.

## Boundary and control flow

`TransactionalMessageHandler` is the service-owned extension point. It receives
only a validated `MessageEnvelope` and the caller-owned PostgreSQL transaction.
Implementations may update service-owned tables and enqueue outbox messages in
that transaction. They must not perform network calls or other external effects
because those operations cannot roll back atomically.

`handle_once` owns the complete one-delivery control flow:

1. Decode the raw structured CloudEvents bytes and validate them against the
   process `MessageRegistry`.
2. Quarantine malformed, unsupported, or misrouted messages before terminal settlement.
3. Start a transaction and record the consumer-scoped inbox identity.
4. On `FirstDelivery`, invoke the domain handler and commit its state, inbox
   record, and any outbox messages atomically.
5. On `Duplicate`, skip the handler and commit the no-op transaction.
6. Confirm broker acknowledgement only after a successful commit.
7. Roll back transient handler failures and request deterministic delayed redelivery.
8. Roll back permanent or delivery-attempt-exhausted handler failures, persist quarantine
   evidence in a new short transaction, commit it, and then confirm terminal settlement.

The coordinator does not receive messages or run a loop. A composition root
owns `MessageConsumer`, connection lifecycle, graceful drain, concurrency, and
recreation of unavailable consumer streams. This keeps intake and service
lifecycle independently reviewable from transactional correctness.

## Retry and terminal policy

`HandlerPolicy` requires a stable logical consumer name, 1–100 delivery attempts,
and base/maximum delays from one millisecond through 24 hours. Retry delay uses
capped exponential backoff with deterministic message-key jitter. Every replica
therefore makes the same decision for the same delivery without synchronized randomness.
The limit uses the broker's delivery-attempt counter, so transport and storage
redeliveries also consume the bounded delivery budget.

| Failure | Resolution |
| --- | --- |
| Malformed envelope or invalid route | Persist `envelope_invalid` or `routing_invalid`, then terminally settle |
| Inbox message-identity conflict | Persist `message_identity_conflict`, then terminally settle |
| Transient handler below delivery-attempt limit | Roll back and request delayed redelivery |
| Permanent handler failure | Roll back, persist the handler's bounded reason code, then terminally settle |
| Transient handler at delivery-attempt limit | Treat as terminal and retain its bounded reason code |
| PostgreSQL availability or commit ambiguity | Request redelivery; never terminally settle without quarantine evidence |
| Storage invariant or quarantine identity conflict | Return an error, leave delivery unsettled, and fail closed for operator investigation |
| Settlement confirmation failure | Return `Settlement`; rely on inbox or quarantine idempotency when redelivered |

Handler reason codes are static lowercase ASCII tokens of at most 64 bytes.
Public errors and outcomes do not copy payload content or database diagnostics;
internal causes remain available through the Rust error chain for redacted logs.

## Crash and acknowledgement safety

A crash before domain commit leaves no inbox record or domain effect. A crash
after commit but before acknowledgement causes redelivery; the inbox returns
`Duplicate`, so the handler does not run twice. A crash after quarantine commit
but before terminal-settlement confirmation causes redelivery; quarantine returns
`AlreadyPresent` before terminal settlement is retried.

Commit failures are treated as ambiguous because PostgreSQL may have committed
without returning confirmation. The coordinator requests redelivery rather than
assuming rollback. This is safe for the same reason: committed work is discovered
through the retained identity, while uncommitted work executes again.

## Verification

Default tests validate policy bounds, reason codes, and deterministic backoff
without external services. The isolated local-platform conformance test runs with:

```text
EDGEAGENT_POSTGRES_URL=postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent cargo test --locked -p edgeagent-inbox-handler --test postgres_handler -- --ignored --exact coordinator_preserves_commit_retry_and_quarantine_ordering
```

It proves first-delivery commit, duplicate suppression, transient rollback and
retry, poison-message quarantine, permanent handler quarantine, lost terminal
confirmation recovery, and acknowledgement-loss recovery. Run it only against
an isolated development or CI database.

The [event-spine recovery conformance](event-spine-conformance.md) additionally
composes this coordinator with the real outbox relay and JetStream adapters,
then abandons a delivery and reconnects before handling its redelivery.

## Current limitations

This increment does not provide a long-running consumer loop, connection pool,
parallelism, acknowledgement-progress extension, graceful shutdown, payload
schema generation, authorization policy, operator replay, or telemetry exporter
installation. It emits bounded durable-outcome signals through the
[event-spine telemetry contract](event-spine-telemetry.md). Domain
handlers and their tables remain service-specific. Those capabilities stay in
separate M02 increments because they change lifecycle, recovery, or domain behavior.
