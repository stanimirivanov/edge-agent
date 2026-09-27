# Transactional PostgreSQL outbox

## TL;DR

- `edgeagent-outbox-postgres` stores exact validated CloudEvents bytes inside a
  caller-owned PostgreSQL transaction.
- CloudEvents identity is `(source, id)`; an identical retry is idempotent and
  different content under the same identity is rejected.
- Relay workers claim bounded batches with `FOR UPDATE SKIP LOCKED` and expiring
  leases, so concurrent workers receive disjoint records and crashed work recovers.
- Publication and retry release require the current, unexpired lease owner.
- Published and quarantined records remain as operational evidence; this
  increment does not delete, archive, or silently rewrite them.

## Transaction boundary

Every service applies `PostgresOutbox::MIGRATIONS` in order inside its
service-owned database schema. The migrations create and evolve
`edgeagent_message_outbox` in the connection's current schema; they do not
create a shared cross-service table.

Application persistence opens a PostgreSQL transaction, commits its domain
state, and calls `PostgresOutbox::enqueue` with the same transaction before
commit. A rollback removes both changes. External publication never occurs
inside that transaction.

The outbox stores:

- CloudEvents source and ID as a composite primary key;
- versioned message type and definition-derived transport subject;
- exact structured JSON bytes in `BYTEA`;
- creation and next-availability timestamps from the database clock;
- attempt count, current lease owner, and lease expiry;
- publication time and the last bounded failure code.
- terminal quarantine time and bounded operator reason.

Reusing an identity with identical type, subject, and bytes returns
`AlreadyPresent`. Reusing it with different immutable content returns
`MessageIdentityConflict`. This prevents a retry key from silently acquiring
new meaning.

## Relay leasing and recovery

A relay performs each state change in a short transaction:

1. `claim_batch` validates the worker token, batch size, and lease duration.
2. PostgreSQL selects eligible records in deterministic order using
   `FOR UPDATE SKIP LOCKED`.
3. The claim sets a database-clock lease and increments the attempt counter.
4. Outside the transaction, the relay revalidates stored bytes against its
   `MessageRegistry` and publishes through `MessagePublisher`.
5. A confirmed `Persisted` or `Duplicate` result calls `mark_published` in a
   new transaction.
6. A retryable failure calls `release_for_retry` with policy-selected delay and
   a bounded, non-sensitive reason code.

If a relay stops after claiming, the record becomes eligible when its lease
expires. If it stops after broker persistence but before `mark_published`, the
next worker republishes the same identity and relies on bounded broker
deduplication. The [transactional PostgreSQL inbox](postgres-inbox.md) provides
consumer-side deduplication after the broker window expires.

The adapter rejects completion by another owner or after lease expiry. This
prevents a slow worker from marking a record after ownership has transferred.
Lease duration must exceed the configured publication timeout while remaining
short enough for the recovery objective; the adapter bounds it to 15 minutes.

`quarantine` retains a leased record while removing it from future claims. It
requires the current unexpired lease, pairs timestamp with a bounded reason,
and cannot coexist with published state. The
[bounded relay](outbox-relay.md) owns publication failure classification and
attempt policy; storage does not infer those application decisions.

## Failure and security behavior

| Category | Meaning | Required response |
| --- | --- | --- |
| `Contract` | Envelope or message definition is invalid | Do not enqueue; correct the producer defect |
| `MessageIdentityConflict` | Existing immutable content differs | Fail closed and investigate identity reuse |
| `InvalidArgument` | Batch, lease, worker, delay, or reason code violates a bound | Correct relay configuration or policy |
| `Storage` | PostgreSQL operation failed | Roll back and apply bounded transient-failure policy |
| `StorageInvariant` | Stored data contradicts adapter assumptions | Quarantine and investigate corruption or unsupported mutation |
| `LeaseLost` | Lease expired, disappeared, or belongs to another worker | Stop processing that record without marking it |

Public errors contain stable categories and bounded validation text. PostgreSQL
causes remain in the Rust error chain for redacted diagnostics. Failure codes
accept only lowercase ASCII tokens and never raw exception or payload text.

Database roles should grant each service access only to its own schema. Relay
workers need select/update access to their service's outbox, while unrelated
services and browser identities receive none. Migration authority remains
separate from runtime identity in deployment profiles.

## Verification

Default workspace tests validate identity construction, argument bounds,
migration invariants, stored-envelope revalidation, and dependency direction
without requiring PostgreSQL.

The isolated local-platform conformance test can be run with:

```text
EDGEAGENT_POSTGRES_URL=postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent cargo test --locked -p edgeagent-outbox-postgres --test postgres_outbox -- --ignored --exact transaction_identity_and_lease_invariants_hold
```

It creates a process-scoped schema, verifies transactional rollback, idempotent
enqueue, conflicting content, source-scoped identity, leasing, foreign-owner
rejection, retry release, attempt counting, publication, and queue exhaustion,
then removes the schema. Run it only against an isolated development or CI database.

## Current limitations

This increment provides storage and leasing, not a continuously running worker.
Bounded retry and outbound quarantine policy are implemented by the relay
crate. Archival, telemetry export, active lease extension, operator replay, and
quarantine retention remain separate work. Consumer inbox deduplication is
implemented separately.
