# Transactional PostgreSQL inbox

## TL;DR

- `edgeagent-inbox-postgres` records a validated CloudEvent before a consumer
  applies its domain transition, using the same caller-owned transaction.
- Inbox identity is `(consumer_name, source, id)`, so redelivery to one logical
  consumer is idempotent while independent consumers can process the same event.
- An identical committed delivery returns `Duplicate`; different immutable
  bytes under the same identity fail closed as `MessageIdentityConflict`.
- A rollback removes the inbox record and domain writes together, allowing a
  later delivery to retry safely.
- Malformed or permanently rejected deliveries retain exact bytes, an opaque
  transport key, bounded reason code, and observed attempts before termination.
- The transport acknowledges only after the transaction commits. This provides
  one committed local transition under redelivery, not exactly-once delivery.

## Transaction boundary

Every consuming service applies `PostgresInbox::MIGRATIONS` in order inside its
own PostgreSQL schema. The migrations create inbox and quarantine tables in the
connection's current schema; they do not create shared cross-service storage.

A handler opens a transaction and calls `PostgresInbox::record_delivery` before
performing domain work. The required control flow is:

1. Decode the untrusted transport bytes and validate the envelope against the
   consumer's `MessageRegistry`.
2. Begin a PostgreSQL transaction.
3. Call `record_delivery` with a stable logical consumer name.
4. On `FirstDelivery`, apply the domain transition and enqueue any resulting
   message in the same transaction, then commit.
5. On `Duplicate`, skip domain work and commit the no-op transaction.
6. Acknowledge the transport delivery only after a successful commit.
7. On a transient storage or domain failure, roll back and request bounded redelivery.
8. On a permanent validation or policy failure, open a short transaction,
   call `quarantine_delivery` with exact bytes and bounded reason, commit it,
   and only then request terminal transport settlement.

This ordering closes both crash windows. A failure before commit leaves no
deduplication marker and can be retried. A failure after commit but before
transport acknowledgement causes redelivery, which returns `Duplicate` and
does not repeat the committed domain transition.

## Identity and concurrency

The composite primary key is `(consumer_name, message_source, message_id)`.
CloudEvents defines message identity as `(source, id)`; `consumer_name` scopes
that identity to one independently checkpointed projection or command handler.
Replica IDs, pod names, and deployment revisions must not be used as consumer
names because a rollout would bypass deduplication. A semantic name such as
`execution_simulator_v1` remains stable across replicas and restarts.

The inbox stores the exact canonical envelope bytes and versioned message type.
An insert that wins the unique key returns `FirstDelivery`. A conflicting insert
waits for the concurrent transaction to commit or roll back. It then either
inserts after rollback or reads the committed record. Identical content returns
`Duplicate`; changed content under the same identity returns
`MessageIdentityConflict`. The conflict prevents an ID from silently acquiring
new meaning.

## Inbound quarantine boundary

Malformed bytes may not contain a valid CloudEvents `(source, id)` pair, so the
quarantine ledger uses `(consumer_name, delivery_key)`. `delivery_key` is an
opaque transport-adapter value that remains stable across redelivery. For the
JetStream adapter it identifies one persisted stream sequence; other cloud
adapters must provide an equivalent stable value. One logical consumer must not
reuse its name across unrelated transport resources whose delivery-key spaces overlap.

`QuarantineEvidence` validates the boundary before database work:

- delivery key and transport subject contain 1–512 visible ASCII bytes;
- delivery attempt is between 1 and PostgreSQL's signed 32-bit maximum;
- failure code is a lowercase ASCII token of 1–64 bytes; and
- exact untrusted payload bytes do not exceed the portable 256 KiB limit.

The first committed observation returns `Inserted`. An identical redelivery
returns `AlreadyPresent`, preserves the original quarantine time, and advances
the last observed attempt. Different subject, payload bytes, or reason under the
same identity returns `QuarantineIdentityConflict`. This protects against key
collision or mutation and makes redelivery after lost terminal-settlement
confirmation safe.

The table is retained evidence and a future replay source, not an automatic
retry queue. Runtime identities may insert and inspect only their service-owned
schema; replay requires a separately authorized workflow that is not yet implemented.

## Failure and security behavior

| Category | Meaning | Required response |
| --- | --- | --- |
| `Contract` | Envelope is invalid or unsupported by this consumer registry | Do not handle; quarantine as a permanent contract failure |
| `MessageIdentityConflict` | This consumer previously committed different immutable content under the same identity | Fail closed and investigate producer identity reuse or storage mutation |
| `InvalidConsumerName` | Consumer name is empty, oversized, or not a lowercase ASCII token | Correct deployment or application configuration |
| `InvalidQuarantineEvidence` | Transport identity, subject, attempt, payload size, or reason is outside portable bounds | Leave unsettled and correct the adapter or policy defect |
| `QuarantineIdentityConflict` | One delivery key refers to different immutable evidence | Fail closed; do not terminate the new delivery until operators investigate |
| `Storage` | PostgreSQL rejected or could not complete an operation | Roll back and apply bounded transient-failure policy |
| `StorageInvariant` | A conflicting key disappeared or stored columns violate adapter assumptions | Roll back, quarantine, and investigate corruption or unsupported mutation |

Public errors expose stable categories and bounded validation text. PostgreSQL
causes remain in the Rust error chain for redacted diagnostics. Envelope payload
content is never copied into public error text.

Database roles should grant a consumer access only to its service-owned schema.
Migration authority remains separate from runtime identity. Browser identities,
peer services, and message publishers receive no inbox table access.

## Retention and operations

Inbox and quarantine records must remain available for at least the longest
interval in which a transport delivery or operator replay can reappear. Deleting
an inbox record sooner re-enables its domain effect; deleting quarantine evidence
sooner can make a terminal delivery unrecoverable. A cleanup policy must derive
its cutoff from broker retention, quarantine retention, replay policy, legal
hold, and the recovery objective; it must never use an arbitrary table-size threshold.

Inbox records expose `processed_at`; quarantine records expose `quarantined_at`
and `last_observed_at`. Both are indexed with `consumer_name` for future bounded
retention work. This increment deliberately does not delete or replay records.
Operators should monitor table growth until retention, archive, legal-hold, and
replay policies are implemented together.

## Verification

Default workspace tests validate consumer-name and quarantine bounds, migration
identity, exact-byte storage, and dependency direction without requiring PostgreSQL.

The isolated local-platform conformance test can be run with:

```text
EDGEAGENT_POSTGRES_URL=postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent cargo test --locked -p edgeagent-inbox-postgres --test postgres_inbox -- --ignored --exact inbox_and_quarantine_preserve_consumer_invariants
```

It creates a process-scoped schema and proves rollback recovery, one committed
domain transition under duplicate delivery, changed-content rejection,
independent consumer scope, unsupported-version rejection, idempotent quarantine,
attempt observation, and quarantine identity-conflict rejection. It then removes
the schema. Run it only against an isolated development or CI database.

## Current limitations

This crate provides transactional storage semantics, not a running transport
consumer. It does not classify handler failures, choose acknowledgement deadlines,
retry delays or attempt limits, compose terminal settlement, delete evidence,
authorize replay, or emit telemetry. Those policies remain separate M02 increments
because they affect message loss, recovery time, evidence retention, and operator control.
