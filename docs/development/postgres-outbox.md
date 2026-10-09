# Transactional PostgreSQL outbox

## TL;DR

- `edgeagent-outbox-postgres` stores exact validated CloudEvents bytes inside a
  caller-owned PostgreSQL transaction.
- Inbox callbacks and standalone producers use one SQLx `enqueue` API, while
  the relay uses the same driver for short claim and outcome transactions.
- CloudEvents identity is `(source, id)`; an identical retry is idempotent and
  different content under the same identity is rejected.
- Relay workers claim bounded batches with `FOR UPDATE SKIP LOCKED` and expiring
  leases, so concurrent workers receive disjoint records and crashed work recovers.
- Publication, retry release, and quarantine require the current, unexpired
  lease owner **and** the generation returned by that specific claim. Reusing a
  worker token cannot authorize an older claim.
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

Application persistence writes domain state and calls `PostgresOutbox::enqueue`
with the caller's mutable SQLx transaction before committing. An inbound
callback passes its existing transaction, so a rollback removes the inbox
marker, service state, and outbox row together; a second database connection
cannot provide this guarantee. The relay uses SQLx for separate, short lease
and outcome transactions. External publication never occurs inside a database
transaction. [ADR-0016](../decisions/0016-use-sqlx-throughout-the-postgresql-outbox.md)
records the single-driver boundary and migration from the temporary dual path.

The outbox stores:

- CloudEvents source and ID as a composite primary key;
- versioned message type and definition-derived transport subject;
- exact structured JSON bytes in `BYTEA`;
- creation and next-availability timestamps from the database clock;
- attempt count, monotonic lease generation, current lease owner, and lease expiry;
- publication time and the last bounded failure code;
- terminal quarantine time and bounded operator reason; and
- append-only replay request, actor, reason, prior quarantine, and attempt evidence.

Reusing an identity with identical type, subject, and bytes returns
`AlreadyPresent`. Reusing it with different immutable content returns
`MessageIdentityConflict`. This prevents a retry key from silently acquiring
new meaning. After an insert conflict, PostgreSQL compares the validated type,
subject, and exact envelope bytes and returns one Boolean; the adapter does not
download the stored envelope for the comparison. The comparison runs as a
separate statement so a conflicting transaction that commits while the insert
waits is visible under PostgreSQL's default `READ COMMITTED` isolation.

## Relay leasing and recovery

A relay performs each state change in a short transaction:

1. `claim_batch` validates the worker token, batch size, and lease duration.
2. PostgreSQL selects eligible records in deterministic order using
   `FOR UPDATE SKIP LOCKED`.
3. The claim sets a database-clock lease and increments both the attempt counter
   and a per-record lease generation. Its committed result returns an opaque
   `LeaseGeneration` with the message.
4. Outside the transaction, the relay revalidates stored bytes against its
   `MessageRegistry` and publishes through `MessagePublisher`.
5. A confirmed `Persisted` or `Duplicate` result calls `mark_published` with
   that claim's generation in a new transaction.
6. A retryable failure calls `release_for_retry` with the same generation,
   policy-selected delay, and a validated, non-sensitive `FailureCode`.
   Terminal quarantine also requires the claim's generation and a validated
   code. The PostgreSQL adapter rechecks the token at its storage boundary.

If a relay stops after claiming, the record becomes eligible when its lease
expires. If it stops after broker persistence but before `mark_published`, the
next worker republishes the same identity and relies on bounded broker
deduplication. The [transactional PostgreSQL inbox](postgres-inbox.md) provides
consumer-side deduplication after the broker window expires.

Every outcome predicate compares message identity, worker token, claim
generation, and unexpired lease. The adapter rejects a stale claim even when
the expired lease was reclaimed by a worker using the **same** token. The
attempt counter cannot serve as the fence: authorized replay resets the attempt
budget, but never resets lease generation. A stale worker may still have sent
the identical message to the broker; fencing prevents it from recording an
outcome for a newer claim, while broker and inbox deduplication contain duplicate
effects. Lease duration must exceed the configured publication timeout while
remaining short enough for the recovery objective; the adapter bounds it to
15 minutes.

`quarantine` retains a leased record while removing it from future claims. It
requires the current unexpired claim, pairs timestamp with a bounded reason,
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
4. resets the attempt count so the next relay claim begins at attempt one,
   without resetting the monotonic lease generation.

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
| `InvalidArgument` | Batch, lease, worker, delay, or replay request violates a bound | Correct relay configuration or request |
| `Storage` | PostgreSQL is unavailable or a transaction is retryable or ambiguous | Roll back and apply bounded transient-failure policy |
| `StorageInvariant` | Stored data, schema, codec, or deterministic database rejection contradicts adapter assumptions | Quarantine and investigate corruption or unsupported mutation |
| `LeaseLost` | Lease expired, disappeared, or its owner/generation no longer names this claim | Stop processing that record without marking it |
| `ReplayRequestConflict` | Replay request identity already names different evidence | Fail closed and investigate request-ID reuse |
| `NotQuarantined` | Target is missing, published, or already released | Refresh operator state; do not infer success |

Public errors contain stable categories and bounded validation text. PostgreSQL
causes remain in the Rust error chain for redacted diagnostics. Failure codes
accept only lowercase ASCII tokens and never raw exception or payload text.

Database roles should grant each service access only to its own schema. Relay
workers need select/update access to their service's outbox, while unrelated
services and browser identities receive none. Migration authority remains
separate from runtime identity in deployment profiles.

## Migration and rollout

The additive migration initializes `lease_generation BIGINT NOT NULL DEFAULT 0`
for existing records. Each successful claim increments it atomically under the
row lock; a generation overflow fails the claim rather than wrapping or
reusing an earlier generation. Publication, retry, quarantine, and audited
replay leave the generation unchanged. The next claim after replay receives a
new generation even though its attempt count restarts at one. This is the
persisted and public-port compatibility decision in
[ADR-0009](../decisions/0009-fence-outbox-transitions-by-claim-generation.md).

Quiesce older relay binaries and drain their in-flight work or let their leases
expire. Apply the migration, then start only the generation-aware version. An
older binary can still execute its owner-only outcome predicate, so mixed relay
versions do not provide fencing. A rollback
that restores an older relay must use the same quiesce-and-drain procedure and
explicitly accepts loss of the fencing guarantee; retaining the corrected relay
is the safe operational default. The column is additive and does not change
message identity, envelope bytes, or retained audit evidence.

## Verification

Default workspace tests validate identity construction, argument bounds,
migration invariants, stored-envelope revalidation, and dependency direction
without requiring PostgreSQL.

The isolated local-platform conformance test can be run with:

```text
EDGEAGENT_POSTGRES_URL=postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent cargo test --locked -p edgeagent-outbox-postgres --test postgres_outbox -- --ignored --exact transaction_identity_and_lease_invariants_hold
EDGEAGENT_POSTGRES_URL=postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent cargo test --locked -p edgeagent-inbox-handler --test sqlx_outbox_atomicity -- --ignored --exact inbox_domain_and_sqlx_outbox_commit_or_roll_back_together
```

It creates a process-scoped schema and verifies transactional rollback,
idempotent enqueue, conflicting content, leasing, foreign-owner rejection,
quarantine, audited replay, duplicate and conflicting replay requests, attempt
budget reset, unchanged envelope bytes, publication, and queue exhaustion.

The same-owner fencing regression is a separate isolated test:

```text
EDGEAGENT_POSTGRES_URL=postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent cargo test --locked -p edgeagent-outbox-postgres --test postgres_outbox -- --ignored --exact same_owner_reclaim_rejects_stale_transitions_and_preserves_replay_fence
```

It expires and reclaims a record with the same worker token, rejects the old
generation for publish, retry, and quarantine without altering the new claim,
and proves audited replay resets attempt budget without reusing the generation.
Both tests remove their process-scoped schemas. Run them only against an
isolated development or CI database. The local-platform CI job invokes these
ignored tests explicitly; the default credential-free test gate does not run
them.

## Compile-checked enqueue queries

The enqueue insert and identity-match statements use `sqlx::query!` and
checked-in offline metadata. The [SQLx metadata guide](sqlx-metadata.md)
describes the combined inbox/outbox schema, refresh commands, and CI freshness
gate. Other outbox statements still use bound runtime SQLx queries and remain
an [M02](../roadmap/milestones.md#m02---contracts-and-event-spine) follow-up.
PostgreSQL conformance tests continue to verify behavior and migration
compatibility, including exact-byte conflicts and atomic inbox/outbox writes.

## Current limitations

This increment provides storage and leasing, not a continuously running worker.
Bounded retry and outbound quarantine policy are implemented by the relay
crate. Control-plane authentication, authorization and dual approval, operator
inspection UI/API, inbound quarantine replay, archival, telemetry export,
active lease extension, and quarantine retention remain separate work. Consumer
inbox deduplication is implemented separately.
