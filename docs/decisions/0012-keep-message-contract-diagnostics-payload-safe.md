# ADR-0012: Keep message contract diagnostics payload-safe

- Status: Accepted
- Date: 2026-10-02
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Public contract and routing errors retain stable failure categories, safe
field names, byte counts, and JSON parser coordinates. They do not retain
free-form Serde or CloudEvents diagnostics or actual message metadata values,
which can carry untrusted or licensed content into logs and error chains.

## Context

The contracts crate converted Serde and CloudEvents failures to strings and
stored them in public error variants. Routing mismatches stored both expected
and actual values, while unsupported-type errors stored the supplied type.
Derived `Debug` and public `Display` rendered those strings. A downstream error
wrapper could redact its own message yet expose the unsafe contract cause via
`source()` or `Debug`.

Contract errors cross service, broker, inbox, outbox, and quarantine boundaries.
They need an operator-safe category, not the raw value that failed validation.
The exact input is available only through the authorized quarantine evidence
path when incident investigation requires it.

## Decision

`MessageContractError` uses unit variants for payload encoding/decoding,
CloudEvents construction, and envelope encoding. Envelope decoding retains
only parser line and column. Validation errors retain fixed field names and
fixed reasons; oversize errors retain byte counts. Free-form third-party error
strings are discarded at this untrusted boundary.

`MessageRoutingError` reports duplicate, unsupported, and mismatch categories
without copying the corresponding message or definition values. A mismatch
retains its fixed field name. Its `Envelope` variant can expose the now-safe
contract error as `source()`; downstream wrappers may preserve that classified
cause. Both `Display` and derived `Debug` remain content-free for these paths.

This is a narrow exception to the general rule to preserve wrapped causes:
retaining an unsafe third-party diagnostic would violate the stronger
no-payload-in-public-errors boundary. The category and parser coordinates
preserve actionable information without the raw diagnostic.

## Alternatives considered

### Keep raw causes and redact only Display

- Benefits: Full third-party diagnostics remain available through `source()`.
- Costs and risks: Derived `Debug`, source-chain logging, and downstream
  wrappers can still expose untrusted content.
- Reason not selected: Redaction must hold across ordinary diagnostic paths.

### Truncate or escape untrusted values

- Benefits: Operators see part of the actual mismatched value.
- Costs and risks: Even short fragments can contain secrets, personal data, or
  licensed content; escaping does not change disclosure.
- Reason not selected: Authorized quarantine inspection is the correct path.

## Consequences

### Positive

- Contract and routing errors can be logged by category without copying raw
  payload or metadata text.
- Redaction holds in `Display`, `Debug`, and nested contract error sources.
- Parser line and column remain available for malformed JSON triage.

### Negative

- Public Rust error variants change shape; exhaustive matches and constructors
  must update at compile time.
- Free-form third-party diagnostics are unavailable from these public errors.

### Neutral or follow-up

- This decision does not sanitize other error families or explicit application
  logging of payloads. Each future untrusted boundary needs its own review.

## Compatibility and migration

This is a compile-time Rust API change in the unreleased `0.1.0` workspace.
Update callers matching string-carrying variants to match unit variants or
`EnvelopeDecoding { line, column }`; match routing mismatches by `field` only.
CloudEvents bytes, subjects, database schema, quarantine evidence, and retry
classification do not change. Deploying old and new binaries together needs
no wire migration. Rollback requires rebuilding callers against the earlier
Rust API and restores unsafe diagnostics.

## Security and operations

Log the stable error category and bounded coordinates or byte counts. Do not
log rejected bytes or a third-party cause separately. Authorized operators
may inspect exact quarantined evidence under existing access and retention
controls. A routing mismatch field identifies the failed contract; it is not
permission to disclose the compared values. If a failure needs more detail,
add a reviewed stable reason code rather than returning raw text.

## Validation

- Synthetic Serde failures prove payload text does not enter `Display` or
  `Debug` for encoding and decoding errors.
- Malformed JSON and routing mismatch tests prove parser and metadata text is
  withheld while safe categories remain distinguishable.
- Workspace checks and tests run through `make verify`; opt-in broker/database
  conformance remains a separate environment-bound gate.
