# Research evidence acquisition

## TL;DR

- New Gateway submissions enter the MarketData-owned research evidence route.
- An offline operator selects one preseeded owner acquisition and trusted caller scope.
- Pending correlation and terminal acquisition receipts survive process/ACK loss.
- Ready carries owner reference metadata; Research must separately authorize object reads.

## Owner selection and commands

Use the existing [offline worker profile](offline-worker.md) and its creation-only
`offline-init` plus `offline-seed <acquisition-id> history-v1` commands. The approved
history recipe has six explicit invented sessions for instrument
`00000000000000000000000000000001`, supporting up to five sessions of momentum
lookback. Missing-history and action-mismatch recipes retain their quality refusal.
Single-v1 is insufficient for a positive lookback. No real-calendar inference or
provider activation is added; existing recipe configuration bytes stay unchanged.

After admission, set `EDGEAGENT_OFFLINE_RESEARCH_SCOPE` to the trusted caller scope.
`offline-research-worker <acquisition-id>` uses the existing loopback database/NATS,
profile root/lock, stream/consumer and 1–16 maximum-message configuration. Provision
its explicit durable consumer with the new command subject (from
[`REQUEST.subject()`](../../crates/contracts/src/research_acquisition.rs)), explicit
ACK, at least 30-second ACK wait, 1–3 maximum deliveries and 1–32 pending ACKs.
The existing profile lock permits one worker at a time. No automatic topology
provisioning, Gateway relay activation or deployment is introduced here.

`offline-research-once <acquisition-id>` instead reads one bounded canonical
CloudEvents envelope from stdin and writes the exact committed result envelope to
stdout. Input ends at EOF and is capped at 262144 bytes; run this finite diagnostic
with a closed pipe. It performs the same owner validation and durable processing
without a broker. Both modes reject foreign scope and malformed correlation before
reserving work. Neither accepts an acquisition ID, path, provider or rights from
request data. The cutoff must include the installed admission's knowledge time;
unsupported instruments return `UnsupportedUniverse`, lookback beyond the retained
session count returns `InsufficientHistory`, and future evidence returns its typed
abstention. Retained prior receipts keep their original bytes/reason.

`offline-research-read <acquisition-id>` accepts the exact retained correlated
publication on stdin and returns canonical bytes only after current owner authority,
retention and descriptor validation. It requires the same independent owner scope/
profile. Read exits are 77 (denied/absent), 65 (invalid) and 75 (storage unavailable).
See the [Research worker](research-worker.md) for the bounded consumer adapter.

## Contracts and durable recovery

The command reuses the Research v1 Request record. The new versioned outcome carries
the entire original request and either an exact ready `EvidenceDecisionData` or an
abstention reason without object data. Subject/partition are
`research-request/<scope>/<id>`; correlation and idempotency equal the canonical
request digest. Owner events authenticate MarketData via the configured transport;
source URI/digest validation alone is insufficient on an untrusted broker.

The [MarketData adapter](../../crates/market-data-postgres/src/research.rs) first
retains canonical request, original ingress context and owner admission. It then
uses existing acquisition storage/authority/terminal publication. A second owner
transaction atomically commits the correlated event, inbox and outbox. No object
or provider I/O occurs inside these transactions. Missing final publication is
pending work, including after a worker dies following acquisition completion.
Original terminal receipts avoid repeating capture; duplicates preserve exact result
bytes and causation, even when delivery order changes. Changed scoped intent and
foreign correlation fail closed. Ready requires an existing exact acquisition
receipt and matching outbox routing/bytes; fake ready descriptors cannot publish.

Current authority is checked before new ready publication. A completed correlated
result is historical metadata on retry, never a current object read grant. Research
must revalidate rights, retention, digest and point-in-time eligibility before using
it. Storage/cancellation/conflict failures remain errors. Invalid selection, denied
rights and failed required quality produce an abstention. When permitted by owner
retention, failed preparation also records the ordinary acquisition rejection.
Uncertain object writes retain the existing explicit owner cleanup obligation.

## Migration and compatibility

Apply [migration 0011](../../crates/market-data-postgres/migrations/0011_research_evidence.sql)
to the stopped MarketData-owned schema. Fresh `offline-init` includes it; startup
never upgrades a pre-existing profile. The runtime role needs SELECT/INSERT and the
single NULL-to-publication UPDATE on correlation, plus existing acquisition/inbox/
outbox permissions; grant no DDL, TRUNCATE or trigger-disable privileges. Correlation
bindings and completed events are immutable while retained. Existing owner retirement
atomically erases correlated admission/receipt data and its private inbox/outbox
envelopes through the acquisition cascade, leaving only an opaque scope/request
denial. That denial fences intake/completion and prevents remapping the retired
request to another acquisition. Grant SELECT/INSERT, but no DELETE, on denial
markers. Broker redelivery of a verified retained denial is acknowledged without
recreating publication. Retain both outbox rows while the correlated receipt is
active; retirement erases them together. Physical object cleanup still requires
owner-confirmed quiescence. No destructive schema migration is introduced.

New Gateway requests use the new route. Retained PR7 direct-evaluation commands
remain unchanged and retryable. Their original route must be provisioned/drained
separately with the later Research worker; this change does not rewrite historical
outbox state. Stop new intake before rollback and keep all authoritative state.
The original Gateway and Research record schemas remain valid. The [Research worker](research-worker.md) consumes these outcomes; projector queries and public access remain later slices.

## Verification

- `cargo xtask verify --check research-acquisition-contracts --python python3`
  protects strict routing/correlation and frozen structural schema checks.
- With loopback `EDGEAGENT_POSTGRES_URL`, run
  `cargo xtask verify --check research-evidence-processes --check research-correlation-postgres --check gateway-admission --check research-persistence --python python3`.
  Separate stdin worker processes prove restart after pending/attempt checkpoints,
  duplicate/reordered delivery, conflict, rights denial and incomplete history;
  PostgreSQL faults prove pending-state recovery and atomic inbox/outbox rollback.
- `cargo xtask verify --check research-evidence-broker --python python3` adds actual
  JetStream confirmed ACK, duplicate recovery, exact outbox relay and poison quarantine.
  It and the existing `offline-worker-processes` and event-spine checks need an owned local
  NATS endpoint as well. PostgreSQL/stdin proofs do not claim broker ACK/restart
  coverage. Default verification compiles/discovers every protected ignored proof.
- Regenerate the new structural schema with
  `cargo run --locked -p edgeagent-contracts --example research_acquisition_schema --features schema`.
  The frozen fixture and runtime validation remain authoritative together.
