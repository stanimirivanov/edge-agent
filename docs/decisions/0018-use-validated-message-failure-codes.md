# ADR-0018: Use validated message failure codes

- Status: Accepted
- Date: 2026-10-08
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Represent service-owned inbound and outbound failure reasons with one
`FailureCode` value. Validate its static token when declaring a constant,
before any retry or quarantine path can use it. Build portable inbound
quarantine evidence from validated delivery metadata rather than loose fields.

## Context

`HandlerFailure` previously accepted arbitrary static strings. The inbound
coordinator and quarantine constructor validated the code only after a handler
had failed, so an invalid code could interrupt durable quarantine. The outbox
relay port also accepted raw reason strings and left validation to its storage
adapter. Inbound quarantine separately revalidated delivery key, subject, and
attempt even though `DeliveryMetadata` had already established those bounds.
It classified bad caller evidence as a storage invariant. This refines the
pre-adapter validation ownership recorded in
[ADR-0006](0006-keep-inbound-coordination-persistence-neutral.md) without
changing that decision's transaction or settlement order.

## Decision

`FailureCode` holds a static, 1–64-byte token containing only lowercase ASCII
letters, digits, `_`, and `-`. Production codes are declared as constants with
`FailureCode::from_static`, so an invalid literal fails compilation. The typed
value is required by `HandlerFailure`, portable inbound quarantine, and the
outbox relay retry and quarantine ports. Storage adapters retain their own
validation at direct public entry points and before SQL writes.

`InboundQuarantine::new` takes a reference to `DeliveryMetadata`, bounded raw
payload bytes, and a `FailureCode`. It borrows the already-validated key,
subject, and positive attempt from metadata and checks the independent payload
bound. A direct oversized-evidence request returns a contract error, not a
storage-invariant error. The handler no longer has a late failure-code
validator or an `InvalidHandlerFailure` outcome.

## Alternatives considered

### Keep raw strings and duplicate validation at each use

- Benefits: no Rust API change.
- Costs and risks: a code typo can fail after processing has rolled back and
  before evidence commits; adapters may disagree on accepted codes.
- Reason not selected: the port must prevent invalid evidence values from
  reaching one-shot recovery decisions.

### Accept dynamic owned failure codes

- Benefits: callers could derive codes from runtime input.
- Costs and risks: creates unbounded vocabulary and makes externally supplied
  text part of retained operational evidence.
- Reason not selected: current retry and quarantine reasons are a closed,
  service-owned classification; dynamic operator replay reasons retain their
  separate fallible boundary.

### Retain loose quarantine metadata parameters

- Benefits: smaller constructor change.
- Costs and risks: repeats validation and allows transposed identity fields.
- Reason not selected: transport adapters already supply a validated,
  transport-neutral `DeliveryMetadata` value.

## Consequences

### Positive

- Invalid service-owned reason literals fail before deployment, not during
  quarantine.
- Inbound and outbound ports share one bounded reason-code vocabulary shape.
- Portable quarantine evidence cannot be constructed from mismatched primitive
  metadata fields, and invalid direct payloads receive a contract category.

### Negative

- Rust callers must pass `FailureCode` rather than `&str` to changed methods.
- `from_static` panics if called at runtime with an invalid static string;
  production code must declare codes in constant context. Future dynamic codes
  need a separate fallible constructor and review of their evidence meaning.

### Neutral or follow-up

- No CloudEvents wire format, PostgreSQL schema, retained value, retry delay,
  delivery identity, or settlement ordering changes.
- Typed lease duration and settlement ownership remain separate refactors.

## Compatibility and migration

This is a workspace Rust API change. Update service handlers, relay policies,
adapters, and tests together; deploy no mixed binary artifacts built against
different port signatures. Persisted failure codes and reason columns retain
their existing token format. No data migration or broker replay is required.
The portable payload bound from
[ADR-0017](0017-align-delivery-and-quarantine-bounds.md) remains in force.

## Security and operations

Failure codes are reviewed classification labels, not raw exception messages,
provider responses, payloads, or operator-supplied text. Public error
formatting remains redacted while explicit `Error::source()` chains retain
causes for controlled diagnosis. PostgreSQL adapters still validate tokens
when called directly. A failed payload bound leaves the delivery unsettled;
the coordinator must not report durable quarantine without a committed record.

## Validation

- Unit tests cover one-byte and 64-byte valid codes and reject empty,
  65-byte, uppercase, whitespace, and non-ASCII values.
- A compile-fail doctest rejects an invalid constant; CI runs workspace
  doctests separately from `--all-targets` tests.
- Portable and adapter tests verify that typed retry and quarantine reasons
  reach persistence without changing their stored text.
- Repository policy, architecture, formatting, Clippy, and workspace tests
  verify the updated public API and downstream callers.
