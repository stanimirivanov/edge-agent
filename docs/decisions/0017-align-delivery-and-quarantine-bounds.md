# ADR-0017: Align delivery and quarantine bounds

- Status: Accepted
- Date: 2026-10-08
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Reject raw transport payloads above the portable 256 KiB envelope limit before
constructing a delivery. Leave an oversized JetStream message unsettled and
halt that consumer instance for investigation; do not claim it was durably
quarantined. Widen PostgreSQL quarantine and replay-audit attempt columns to
`BIGINT` so every positive portable `u32` attempt remains representable.

## Context

[ADR-0011](0011-bound-untrusted-envelopes-before-decoding.md) bounds direct
envelope decoding, but an adapter could still hand a larger raw payload to the
handler. The handler's quarantine path must retain exact bytes, while
`QuarantineEvidence` rejects payloads above 256 KiB. That combination made an
oversized delivery impossible to settle through the documented durable
quarantine path. Separately, portable `DeliveryAttempt` accepts positive `u32`
values, but quarantine and replay audit used PostgreSQL `INTEGER` columns and
rejected attempts above `i32::MAX`.

## Decision

`MessageDelivery::new` validates the raw payload length and returns `Result`.
It accepts exactly 256 KiB and returns `ConsumeErrorKind::Protocol` above that
limit. The JetStream adapter performs this validation before returning a
delivery. On a protocol failure it leaves the raw broker message unacknowledged
and stops further pulls on that instance. An operator investigates the producer
and broker configuration before replacing the consumer and permitting
redelivery. Broker retention is not a committed quarantine record.

For accepted deliveries, the handler may persist exact bounded bytes and
settle terminally only after the quarantine transaction commits. The
PostgreSQL adapter accepts every positive portable `u32` delivery attempt and
stores first and last observed attempts, including replay-audit snapshots, as
`BIGINT`. Migration 0004 widens the four existing columns without discarding
prior observations. It does not change delivery identity or evidence bytes.

## Alternatives considered

### Let the handler reject oversized input

- Benefits: no constructor API change.
- Costs and risks: exact-byte quarantine cannot retain the oversized input;
  repeated handling cannot reach a durable terminal disposition.
- Reason not selected: the adapter must reject before promising a delivery that
  the handler cannot settle according to the existing contract.

### Truncate or hash oversized quarantine evidence

- Benefits: could create a compact database record.
- Costs and risks: changes the meaning of retained exact-byte evidence and
  cannot support byte-exact authorized replay.
- Reason not selected: it requires a distinct evidence schema and operational
  policy, not an implicit fallback in this refactor.

### Keep PostgreSQL attempts within signed 32-bit range

- Benefits: no migration.
- Costs and risks: a valid portable attempt can still fail before durable
  quarantine and replay authorization.
- Reason not selected: storage must cover the public portable domain.

## Consequences

### Positive

- No constructed delivery exceeds the bound of the exact-byte quarantine path.
- Quarantine and replay audit preserve the full positive `u32` attempt range.
- Oversized input cannot silently become a broker acknowledgement or a false
  claim of durable quarantine.

### Negative

- Direct Rust callers must handle `MessageDelivery::new` returning `Result`.
- Oversized raw broker messages require operator investigation; repeated
  restarts can consume a broker's finite delivery allowance.
- Widening populated PostgreSQL columns requires a planned migration window.

### Neutral or follow-up

- The limit does not replace broker-level frame limits or upstream admission
  controls. A future oversized-evidence path would require its own decision.
- This decision does not change the CloudEvents wire format, inbox identity,
  terminal settlement order, or dry-run-only execution boundary.

## Compatibility and migration

Update every in-workspace `MessageDelivery::new` caller to handle the fallible
constructor. Quiesce old inbox consumers before applying migration 0004, then
deploy consumers compiled for the widened columns. Do not run new binaries
against the old schema or assume old binaries can decode widened columns. A
database rollback to `INTEGER` is unsafe once any stored attempt exceeds
`i32::MAX`; no automatic down-migration is provided.

## Security and operations

Protocol errors expose a bounded reason, not payload bytes. The JetStream
adapter neither acknowledges nor terminally settles an oversized message and
halts further pulls on that consumer instance. Operators should inspect
producer output, stream size policy, acknowledgement wait, and `max_deliver`
before restart. Only accepted, bounded poison deliveries can generate
committed exact-byte quarantine evidence. Quarantine and replay audit remain
under the service-owned PostgreSQL schema and existing access controls.

## Validation

- Unit tests accept the exact byte limit, reject the adjacent byte, and prove
  that a payload-size protocol fault halts the pre-pull gate.
- An isolated JetStream test checks that the raw message remains unacknowledged
  and redelivers to a replacement consumer after the halted instance is gone.
- PostgreSQL tests cover `u32::MAX` at first observation, redelivery, and
  replay snapshot, plus migration of pre-existing rows.
- Database-backed CI regenerates SQLx metadata from ordered migrations and
  checks that committed offline metadata is fresh.
