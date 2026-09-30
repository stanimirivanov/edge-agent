# Transactional PostgreSQL outbox

## TL;DR

- `edgeagent-outbox-postgres` stores exact validated CloudEvents bytes inside a
  caller-owned PostgreSQL transaction.
- CloudEvents identity is `(source, id)`; an identical retry is idempotent and
  different content under the same identity is rejected.
- Relay workers claim bounded batches with `FOR UPDATE SKIP LOCKED` and expiring
  leases, so concurrent workers receive disjoint records and crashed work recovers.
- Publication and retry release require the current, unexpired lease owner.
- An authorized replay request atomically appends operator audit evidence and
  releases immutable quarantine content with a fresh attempt budget.
- Published and quarantined records remain as operational evidence; the adapter
  does not delete, archive, or silently rewrite them.

## Transaction boundary

Every service applies `PostgresOutbox::MIGRATIONS` in order inside its
service-owned database schema. The migrations create and evolve
`edgeagent_message_outbox` in the connection's current schema; they do not
create a shared cross-service table.

The crate root is the stable public façade. `enqueue` owns exact-byte insertion
and identity checks; `leasing` owns claim, publish, retry, and quarantine
transitions; `replay` owns authorization audit and release; `validation` and
`error` centralize bounded adapter inputs and failure categories. The existing
`relay` module composes these operations behind the portable storage port.
This organization does not change SQL or transaction semantics.

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
- publication time and the last bounded failure code;
- terminal quarantine time and bounded operator reason; and
- append-only replay request, actor, reason, prior quarantine, and attempt evidence.

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

## Outbound replay authorization and recovery

`replay_quarantined` is the persistence boundary used after a control plane has
authenticated an operator, authorized the target and reason, and applied any
required approval policy. `ReplayRequest` bounds the replay request ID, operator
identity, and reason before database work; it is audit evidence, not an
authentication mechanism.

Migration 0003 adds `edgeagent_message_outbox_replay_audit`. One transaction:

1. locks the requested quarantined record;
2. appends its replay request ID, operator identity, reason, prior quarantine
   reason, prior attempt count, and database timestamps;
3. clears quarantine and prior failure state; and
4. resets the attempt count so the next relay claim begins at attempt one.

The envelope, CloudEvents identity, routing metadata, and creation time never
change. Preserving identity makes an uncertain earlier publication safe: broker
deduplication may suppress it, and a consumer inbox suppresses it after the
broker window. The replay request ID is independently idempotent. An exact
duplicate returns `AlreadyRequested`; reuse with changed target, actor, or reason
returns `ReplayRequestConflict`. If the target is published, missing, or already
released, a new request returns `NotQuarantined`.

Concurrent requests cannot release one quarantine generation twice. The first
committed request wins; another request must observe a later quarantine and use
a new authorization ID. Runtime roles should receive only the minimum table
privileges needed by their component. Operators and browsers must not receive
direct table access, and audit rows must remain append-only under deployment policy.

## Failure and security behavior

| Category | Meaning | Required response |
| --- | --- | --- |
| `Contract` | Envelope or message definition is invalid | Do not enqueue; correct the producer defect |
| `MessageIdentityConflict` | Existing immutable content differs | Fail closed and investigate identity reuse |
| `InvalidArgument` | Batch, lease, worker, delay, or reason code violates a bound | Correct relay configuration or policy |
| `Storage` | PostgreSQL operation failed | Roll back and apply bounded transient-failure policy |
| `StorageInvariant` | Stored data contradicts adapter assumptions | Quarantine and investigate corruption or unsupported mutation |
| `LeaseLost` | Lease expired, disappeared, or belongs to another worker | Stop processing that record without marking it |
| `ReplayRequestConflict` | Replay request identity already names different evidence | Fail closed and investigate request-ID reuse |
| `NotQuarantined` | Target is missing, published, or already released | Refresh operator state; do not infer success |

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

It creates a process-scoped schema and verifies transactional rollback,
idempotent enqueue, conflicting content, leasing, foreign-owner rejection,
quarantine, audited replay, duplicate and conflicting replay requests, attempt
budget reset, unchanged envelope bytes, publication, and queue exhaustion. It
then removes the schema. Run it only against an isolated development or CI database.

## Current limitations

This increment provides storage and leasing, not a continuously running worker.
Bounded retry and outbound quarantine policy are implemented by the relay
crate. Control-plane authentication, authorization and dual approval, operator
inspection UI/API, inbound quarantine replay, archival, telemetry export,
active lease extension, and quarantine retention remain separate work. Consumer
inbox deduplication is implemented separately.
