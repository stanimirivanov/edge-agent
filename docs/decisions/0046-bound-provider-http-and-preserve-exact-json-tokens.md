# ADR-0046: Bound provider HTTP and preserve exact JSON tokens

- Status: Accepted
- Date: 2026-10-22
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Add an outbound HTTP adapter with exact JSON numeric tokens and explicit bounded I/O.
- Bind HTTPS GET requests and pagination to reviewed scope and immutable endpoint parameters.
- Disable redirects, automatic retries, inherited proxies and compression.
- Keep live provider mapping and durable/cross-replica budgets in later M03 slices.

## Context

Acquisition admission and immutable evidence contracts exist, but canonical
constructors cannot recover precision after a vendor number passed through a
floating-point parser. An HTTP client also needs explicit egress, byte, timing,
cancellation and retry boundaries before a real provider can be qualified.

## Decision

Add `market-data-http` as an adapter depending inward on market application and
domain values. Reuse pinned Reqwest, Serde JSON RawValue, Tokio, URL and HTTP-date.
Use Rustls/ring/native roots with verified TLS, no process-global provider
installation, and a pinned preconfigured-client boundary. Keep SDK types out of
application/domain crates. Move blocking Reqwest selection to the conformance
consumer rather than inheriting it in every HTTP consumer.

Provide non-cloneable local sessions anchored to the admitted remaining command
and retention deadline. Reserve attempts before polling I/O, bound streaming
before appending, and check explicit cancellation. Allow only configured cursor
changes at the same origin and exact path. Preserve raw numeric tokens and use
integer coefficient/scale conversion followed by existing checked value types.
Reject duplicate keys, excessive depth/count and unsupported numeric precision.

Initially support identity encoding only, so feature unification cannot introduce
an implicit decompressor. Classify errors and Retry-After without sleeping or
retrying. The [transport guide](../development/provider-http.md) owns exact limits,
failure behavior, trust assumptions and local verification commands.

## Alternatives considered

- Vendor SDK now: rejected; no provider is selected and SDK mapping may hide
  numeric conversion, retries, pagination or endpoint authority.
- Generic floating-point JSON values: rejected; precision loss precedes domain validation.
- New decimal engine: rejected; existing checked domain values already own units and scale.
- Automatic gzip/redirect/retry handling: rejected; it obscures byte, authority or attempt budgets.
- New durable workflow/budget system here: rejected; transport is not the stateful owner.

## Consequences

The adapter has directly testable behavior without a provider subscription or
manually started service. Callers must supply every timing/configuration value.
Strict endpoint/query and encoding policy can reject a future provider; qualify
and explicitly adapt that policy instead of weakening it implicitly.
Local counters do not survive recreation or restart and do not serialize accounts.

## Compatibility and migration

This is an additive, unactivated adapter. Evidence-v1 bytes, message definitions,
existing provider/application APIs and persisted records remain unchanged.
Reqwest blocking features remain on existing tooling, not on the workspace base.
TLS/RawValue feature unification must pass all existing compatibility baselines.
Review pinned Reqwest/Rustls together because preconfigured TLS is version-sensitive.

## Security and operations

Only trusted configuration chooses an initial HTTPS endpoint; no command supplies
arbitrary egress. Configuration must use data-only credentials and deployment
must restrict DNS/egress, including cloud metadata/private networks. No broker,
model, storage, worker or live-data activation is added. Public diagnostics redact
credentials, URLs and payloads; opt-in error causes are URL-sanitized. Test-only
HTTP loopback construction is private and absent from production APIs.

## Validation

Default acceptance includes an exact six-test HTTP census and feature-unified
numeric/transport proofs. Tests cover both successful data and failure boundaries,
including cancellation and dropped futures. Native trust-store access may require
an unsandboxed local test process; missing capabilities must not become passes.
Run dependency/SBOM policy for TLS feature edges and retain explicit not-run
results where tooling is unavailable. Live qualification remains a later gate.
