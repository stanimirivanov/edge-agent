# ADR-0023: Type relay configuration and runtime failures

- Status: Accepted
- Date: 2026-10-16
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes: ADR-0019
- Superseded by:

## TL;DR

Construct relay policy from validated `LeaseDuration`, `MaxAttempts`, and
`RetryBackoff` values. Separate startup validation from runtime availability,
transition rejection, and invariant failure. Preserve a failed resolution's
primary cause and earlier message failure independently, with redacted public
formatting. Keep timing, publication admission, and cancellation behavior.

## Context

ADR-0019 established portable timing types but retained a raw relay constructor.
That constructor mixed startup validation with runtime errors. Runtime storage
invariants and rejected transitions collapsed into the same category; failed
retry/quarantine discarded the message failure that prompted persistence.
Calculated delays returned primitives, used silent overflow fallbacks, and
treated later timing validation failures as invalid startup configuration.

## Decision

Replace the five-primitive relay constructor in one workspace migration with
`RelayPolicy::new(owner, lease, max_attempts, backoff)`. `MaxAttempts` validates
1–100 inclusive. `RetryBackoff` validates `1ms <= base <= maximum <= 24h` and
preserves exact input precision. Their private fields prevent invalid public
construction. The constructor validates the existing 1–128-byte lowercase
ASCII owner token. These checks return `RelayPolicyError`; constructing a
portable lease retains its existing `OutboxTimingError`. Neither is implicitly
converted into a relay iteration failure. There is no parallel raw constructor.

Retain ADR-0019's portable contracts in full: `LeaseDuration` accepts 1ms–15min,
`OutboxRetryDelay` accepts zero–24h, both preserve exact input durations, and
PostgreSQL floors persisted timing to milliseconds. Storage methods still
require those types; broker redelivery retains its distinct nonzero type.
The relay deliberately requires nonzero backoff. This supersession changes the
relay constructor choice, not those timing limits or storage responsibilities.

Runtime `RelayErrorKind` maps portable storage categories one-to-one:
`Unavailable`, `StateTransition`, and `Invariant`. A transition rejection does
not by itself identify lease loss. Preserve the original `OutboxStoreError`
through `Error::source()`, including whether its adapter cause exists. If retry
or quarantine persistence fails, retain the earlier stored-contract,
publication, or budget failure through `triggering_failure()`. Do not replace
the primary source chain with that secondary context. Claim and confirmed
publication-marker failures have no message failure to invent. A receipt is
not an error and is not represented as one.

Calculate capped exponential equal jitter using bounded arithmetic, the same
FNV identity/attempt hash, and existing millisecond flooring. Validated base
is at most 86,400,000ms and exponent at most 63, so the `u128` product is below
2^90. The cap and nonzero base establish a nonzero jitter interval. Return a
validated `OutboxRetryDelay` throughout the internal retry decision; expose
only its existing `Duration` projection in successful retry outcomes. Checked
integer/timing conversion errors retain their actual cause as runtime
`Invariant`, with the publication trigger, before storage work. Never replace
an unexpected calculation error with a different delay or retry transition.

## Alternatives considered

### Retain raw inputs and one shared error type

- Benefits: no public Rust migration.
- Costs and risks: invalid values remain interchangeable and startup defects
  remain indistinguishable from storage failures.
- Reason not selected: configuration must establish its invariants before I/O.

### Retain only the most recent failure, or replace the storage cause

- Benefits: one error chain and less retained context.
- Costs and risks: either loses the triggering failure or obscures the storage
  failure that prevented a durable outcome.
- Reason not selected: primary cause and earlier trigger answer different
  recovery questions and must remain independently inspectable.

### Saturate calculation errors into a maximum delay

- Benefits: avoids returning an unexpected internal failure.
- Costs and risks: hides violated invariants and silently changes scheduling.
- Reason not selected: bounded arithmetic explains legitimate capping; failed
  conversions must fail explicitly rather than invent a successful schedule.

## Consequences

### Positive

- Startup and runtime failures have distinct types and actionable categories.
- Public configuration cannot bypass timing/order/budget validation.
- Failed resolution preserves both diagnostic contexts without payload leaks.
- Frozen jitter vectors and boundary tests protect unchanged scheduling.

### Negative

- Callers must migrate constructors and exhaustive runtime matches together.
- Operators must explicitly inspect and redact two cause paths when diagnosing
  a failed resolution; logging a source chain remains unsafe by default.

### Neutral or follow-up

No worker runtime, lease renewal, automatic compensation, exporter, or operator
API is introduced. ADR-0022 still governs recovered-claim publication budgets.

## Compatibility and migration

These unpublished Rust APIs change in one coordinated PR. Remove runtime
`InvalidPolicy`, `Storage`, and `Outbox` matches in favor of startup validation
and the three observed runtime categories. Migrate all workspace call sites,
tests, and examples without legacy overloads. No event wire, reason code,
database schema, dependency, or successful retry schedule changes. Existing
stored claims need no migration. Rollback restores coarser diagnostics and raw
configuration but does not change persisted state or ADR-0022 admission policy.

## Security and operations

Fixed public `Display`/`Debug` text never renders either cause, owner input, or
payload. Explicit cause access is for authorized diagnosis, not unredacted
logging. A returned runtime error confirms neither rollback nor non-delivery.
Continue through committed state, lease expiry, current-generation fencing,
and authorized replay. No error/cancellation path starts an alternate outcome.
The composition root must still ensure publication timeout is below the lease;
range validation cannot infer that relationship.

## Validation

Protected T2 policy contracts cover inclusive boundaries, invalid inputs,
precision, owner redaction, auto traits, and all runtime storage categories
with/without underlying adapter causes. Focused compile-fail doctests reject
raw lease, attempt, and backoff values. Unit tests preserve primary/secondary
sources and exact legacy jitter vectors, including overflow-sized history.
The isolated observation matrix checks failed claim/published/retry/quarantine
operations, both cause paths, bounded telemetry, original fences, and no
alternate transition. Discovery mutation tests reject missing, renamed,
duplicate, and unregistered contracts. Existing PostgreSQL and real JetStream
recovery proofs validate the migrated callers without claiming a new worker
lifecycle, wire ACK-loss, or exactly-once execution guarantee.
