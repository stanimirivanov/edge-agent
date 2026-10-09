# ADR-0020: Retain settlement abandonment diagnostics

- Status: Accepted
- Date: 2026-10-09
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Carry a single payload-safe diagnostic guard from an owned delivery into its
settlement future. Warn once if either is abandoned before the adapter returns
a result. Never settle from `Drop`; incomplete settlement has an unknown
confirmation outcome and recovery continues through broker redelivery and
durable inbox deduplication.

## Context

`MessageDelivery` stored an optional settlement solely to support its own
`Drop` implementation. Consuming `settle` took that value and disabled the
warning before the returned future ran. Dropping an unpolled or pending future
therefore hid an incomplete operation. The empty-settlement error branch was
unreachable through the safe API, which already consumes the delivery once.

JetStream settlement performs asynchronous confirmation. Other adapters may
perform synchronous work while constructing their futures, so an unpolled
future does not universally prove that no broker operation occurred.

## Decision

Store the settlement directly as `Box<dyn DeliverySettlement>`. Move delivery
ownership and diagnostics into a cohesive private delivery module while
retaining the existing public façade and `SettlementFuture` alias. The
delivery itself has no `Drop` implementation and can transfer its fields
without an optional or missing settlement state.

A private guard owns only positive attempt and payload byte count. An
unsettled delivery owns that guard. Consuming `settle` releases the delivery's
payload and metadata, keeps the guard alive during the synchronous adapter
method call, then transfers it into a wrapper future. The guard disarms only
when the adapter returns a result, including an explicit error. Adapter errors
and their causes pass through unchanged; returning an error is not abandonment.

Dropping a delivery emits the existing unsettled-delivery warning. Dropping an
incomplete settlement future emits a settlement-abandonment warning with
`settlement_state = "confirmation_unknown"`. Both expose only attempt and
byte count. No warning includes identity, route, payload, disposition, or an
error chain. A guard can emit at most once.

Mark the delivery and settlement methods `must_use`. Document that callers
must await confirmation and that cancellation never performs an automatic
acknowledgement, retry, or terminal settlement. Preserve synchronous adapter
invocation timing and all broker disposition mappings.

## Alternatives considered

### Keep diagnostics only on delivery drop

- Benefits: no wrapper future.
- Costs and risks: transferring ownership hides incomplete settlement.
- Reason not selected: the future still owns the one-shot operation.

### Settle automatically when a delivery or future is dropped

- Benefits: might reduce abandoned broker deliveries.
- Costs and risks: `Drop` cannot await confirmation; acknowledgement can lose
  work and terminal settlement can bypass durable quarantine ordering.
- Reason not selected: a diagnostic must not become a recovery policy.

### Require a new public concrete future type

- Benefits: could redesign allocation and polling control.
- Costs and risks: expands public compatibility scope and adapter changes.
- Reason not selected: a private guard and wrapper preserve the existing port.

## Consequences

### Positive

- Delivery ownership cannot contain a missing settlement.
- Abandoning either an unpolled or pending future remains observable.
- The wrapper retains no delivery payload or metadata after ownership transfer.
- Explicit errors remain caller-visible without duplicate abandonment warnings.

### Negative

- Wrapping the existing boxed adapter future adds one small boxed future.
- Intentional cancellation emits a warning; shutdown must account for it.

### Neutral or follow-up

- Warnings are best-effort process diagnostics, not durable audit evidence.
- Broader publication and persistence cancellation contracts remain separate.

## Compatibility and migration

Public method signatures and trait bounds remain unchanged. Code that drops a
settlement future now warns instead of silently abandoning it. Callers must
await and inspect its result; tests that intentionally discard it must expect
abandonment. No broker configuration, event wire format, SQL, or schema changes.
Rollback changes diagnostic coverage only and does not require data migration.

## Security and operations

The guard stores only scalar diagnostic fields, avoiding payload retention and
identity leakage. Dropping an incomplete future cannot establish whether an
adapter's request reached the broker. Do not infer success or emit an automatic
replacement disposition. Redelivery must use the existing committed inbox
identity; durable quarantine still precedes terminal broker settlement.
Process abort and forgotten values do not run Rust destructors and cannot be
covered by this warning mechanism.

## Validation

An isolated T2 tracing test verifies one warning for an unsettled delivery,
unpolled settlement, and pending cancellation. Controlled futures prove exact
one-time forwarding of all dispositions, resource release, payload-safe
diagnostics, and unchanged success and explicit-error results. No sleeps,
broker, or async runtime dependency is needed. Compile-fail doctests enforce
consuming ownership and `must_use`. Existing Linux/Windows workspace tests and
doctest gates run these contracts; JetStream conformance verifies unchanged
acknowledgement, retry, and terminal settlement behavior.
