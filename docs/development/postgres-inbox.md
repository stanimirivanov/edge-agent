# Transactional PostgreSQL inbox

## TL;DR

- `edgeagent-inbox-postgres` implements the atomic inbound-store port and owns
  the transaction that combines a validated CloudEvent with service-owned SQL.
- Inbox identity is `(consumer_name, source, id)`, so redelivery to one logical
  consumer is idempotent while independent consumers can process the same event.
- An identical committed delivery returns `Duplicate`; different immutable
  bytes under the same identity fail closed as `MessageIdentityConflict`.
- A rollback removes the inbox record and domain writes together, allowing a
  later delivery to retry safely.
- Malformed or permanently rejected deliveries retain exact bytes, an opaque
  transport key, bounded reason code, and observed attempts before termination.
- An authorized replay request appends immutable operator and prior-failure
  evidence before returning the original subject and bytes to a publisher.
- The transport acknowledges only after the transaction commits. This provides
  one committed local transition under redelivery, not exactly-once delivery.

## Transaction boundary

Every consuming service applies `PostgresInbox::MIGRATIONS` in order inside its
own PostgreSQL schema. The migrations create inbox and quarantine tables in the
connection's current schema; they do not create shared cross-service storage.

The crate root is the stable public façade. `delivery`, `quarantine`, and
`replay` own their respective SQL and transaction-scoped operations;
`validation` bounds inputs before storage work, `error` preserves redacted
failure categories, and `handler` composes the adapter with the portable
inbound port. This organization changes no transaction or replay contract.

Application coordination uses `PostgresInboundMessageStore`, which implements
the portable `InboundMessageStore` capability. Across the persistence-neutral
coordinator and this adapter, the required control flow is below; the adapter
owns every PostgreSQL transaction step:

1. Decode the untrusted transport bytes and validate the envelope against the
   consumer's `MessageRegistry`.
2. The adapter begins a PostgreSQL transaction and calls `record_delivery` with
   a stable logical consumer name.
3. On `FirstDelivery`, invoke the adapter-specific
   `PostgresTransactionalMessageHandler` so it can apply service-owned SQL and
   enqueue any resulting message in the same transaction, then commit.
4. On `Duplicate`, skip service-owned work and commit the no-op transaction.
5. Return the portable committed disposition to the coordinator, which then
   acknowledges the transport delivery.
6. On a transient storage or service failure, roll back and request bounded redelivery.
7. On a permanent validation or policy failure, open a short transaction,
   call `quarantine_delivery` with exact bytes and bounded reason, commit it,
   and only then request terminal transport settlement.

Only `PostgresTransactionalMessageHandler` receives the concrete
`tokio_postgres::Transaction`. This callback is an infrastructure composition
seam for service-owned SQL, not a domain or persistence-neutral application
port. It must not perform network calls or other effects that cannot roll back
with the transaction.

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

The table is retained evidence and a replay source, not an automatic retry
queue. Runtime identities may insert and inspect only their service-owned
schema. Replay requires a separately authenticated and authorized control-plane
workflow; direct table updates are not supported.

## Inbound replay authorization

`authorize_quarantine_replay` is the persistence boundary used after a control
plane authenticates an operator, authorizes the target and reason, and applies
any required approval policy. `ReplayRequest` bounds the request identity,
operator identity, and reason before database work.

Migration 0003 adds `edgeagent_message_quarantine_replay_audit`. One transaction:

1. Locks the selected quarantine record.
2. Appends the request identity, operator, reason, target, prior failure code,
   attempt range, and original observation timestamps.
3. Returns the exact retained transport subject and payload bytes.

The caller must commit this transaction before publishing the returned bytes.
The adapter never publishes, deletes, repairs, or interprets the untrusted
payload. Repeating an identical request returns `AlreadyAuthorized` and the
same bytes. Reusing a request identity with a different target, operator, or
reason fails closed as `ReplayRequestConflict`. An intentional additional
replay requires a new authorization identity and produces a new audit row.

Authentication, authorization, dual control, rate limits, publication, and
publication-outcome recording belong to a separate operator control plane.
Runtime consumer roles should not receive replay-audit write privileges;
control-plane roles should receive only the minimum target-read and audit-insert
permissions needed for this operation.

## Failure and security behavior

| Category | Meaning | Required response |
| --- | --- | --- |
| `Contract` | Envelope is invalid or unsupported by this consumer registry | Do not handle; quarantine as a permanent contract failure |
| `MessageIdentityConflict` | This consumer previously committed different immutable content under the same identity | Fail closed and investigate producer identity reuse or storage mutation |
| `InvalidConsumerName` | Consumer name is empty, oversized, or not a lowercase ASCII token | Correct deployment or application configuration |
| `InvalidQuarantineEvidence` | Transport identity, subject, attempt, payload size, or reason is outside portable bounds | Leave unsettled and correct the adapter or policy defect |
| `QuarantineIdentityConflict` | One delivery key refers to different immutable evidence | Fail closed; do not terminate the new delivery until operators investigate |
| `InvalidReplayRequest` | Replay request identity, operator, reason, or target is outside portable bounds | Reject before database work and correct the control-plane request |
| `ReplayRequestConflict` | A replay request identity already names different authorization evidence | Fail closed and investigate request-ID reuse |
| `NotQuarantined` | The requested consumer and delivery key do not identify retained quarantine evidence | Reject without creating audit evidence |
| `Storage` | Connection or resource failure, retryable transaction rejection, or ambiguous outcome | Roll back when possible and request redelivery of the same immutable message |
| `StorageInvariant` | Schema, permission, constraint, parameter-type, or stored-column mismatch; unexpected database rejection | Roll back, leave unsettled, and investigate the deployment or data invariant |

`InboxError` `Display` and `Debug` expose stable categories and bounded
validation text, not PostgreSQL causes. Causes remain in the Rust error chain
for controlled diagnostics. Envelope payload content is never copied into
public error text.

The adapter retries PostgreSQL connection exceptions, serialization failures,
deadlocks, unknown statement completion,
insufficient-resource responses, lock unavailability, query cancellation, and
server shutdown/startup responses. Other server SQLSTATEs, including missing
tables or columns, are storage invariants. A driver-reported parameter type
mismatch or row-decoding failure is also an invariant. Connection loss during
commit remains an ambiguous outcome and requires redelivery; the durable inbox
identity resolves whether the first attempt committed. If explicit rollback
fails, the adapter reports unavailability because it cannot confirm cleanup.

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
and `last_observed_at`; replay audit records expose `authorized_at`. Target and
time indexes support bounded inspection. This increment deliberately does not
delete records or publish replay bytes. Operators should monitor table growth
until retention, archive, legal-hold, and replay policies are implemented together.

## Verification

Default workspace tests validate consumer-name and quarantine bounds, migration
identity, exact-byte storage, and dependency direction without requiring PostgreSQL.

The isolated local-platform conformance test can be run with:

```text
EDGEAGENT_POSTGRES_URL=postgresql://edgeagent:edgeagent-local-postgres@127.0.0.1:5432/edgeagent cargo test --locked -p edgeagent-inbox-postgres --tests -- --ignored
```

It creates a process-scoped schema and proves rollback recovery, one committed
domain transition under duplicate delivery, changed-content rejection,
independent consumer scope, unsupported-version rejection, idempotent quarantine,
attempt observation, quarantine identity-conflict rejection, exact replay bytes,
idempotent authorization, conflicting request rejection, and audit snapshots. It
also verifies that missing tables, incompatible stored types, and replay
row-decoding faults fail closed while connection loss remains retryable. Each
schema-mutating test removes its isolated schema. Run the suite only against an isolated
development or CI database.

## Current limitations

This crate provides transactional storage semantics and a PostgreSQL-specific
service callback, not a running transport consumer. Portable failure
classification, retry, settlement, and event-spine telemetry are implemented by
the coordinator described in the [handler guide](inbox-handler.md). Neither
crate chooses acknowledgement-progress deadlines, deletes evidence,
authenticates or authorizes operators, publishes replay bytes, or records replay
publication outcomes. Those capabilities remain separate increments because
they affect message loss, recovery time, evidence retention, and operator control.
