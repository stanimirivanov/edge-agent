# ADR-0087: Query Research through the local Gateway

- Status: Accepted
- Date: 2026-10-29
- Milestone: M04 - Deterministic research
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Gateway queries use the projector's bounded internal HTTP API.
- Enforce the existing trusted local operator scope before upstream I/O.
- Preserve observation states and exact canonical bytes without freshness claims.

## Context

Durable admission and persistent projections exist, but clients need a Gateway read
surface. Direct projector-table reads would couple storage ownership. A projection
may lag owner publication, and a successful admission need not yet be visible there.
Public identity and lifecycle transitions are outside this slice.

## Decision

Expose scoped status and outcome GET routes under `/v1/research-requests/{scope}/{id}`.
Use a Gateway-owned Reqwest adapter to the projector internal v1 API, with an explicit
optional literal loopback HTTP origin. Reuse the pinned workspace transport library
and its no-proxy/no-redirect/no-retry pattern; no new external version is introduced.
Use the existing explicit Rustls/ring provider with an empty root store so unified
`rustls-no-provider` features cannot panic during client construction; do not install
a global provider or enable HTTPS destinations. Missing configuration returns 503 for queries while retaining admission-only startup.
Malformed explicit configuration fails startup. No peer data or service implementation
imports, persistence changes, authentication scheme or owner operations are added.

Apply existing Host/Origin/operator restrictions before upstream calls. Ignore
forwarded identity and send no caller headers upstream. Bound time, response sizes
and header count; cap active upstream requests at eight and refuse saturation
without queueing. Validate the supported status/schema/revision/key combinations,
requested scope/ID and exact canonical artifact/abstention bytes. Unexpected, foreign,
oversized, redirected, encoded, malformed or unavailable responses fail closed as 503.
Metadata is reserialized into the known shape; outcome bytes are returned unchanged.

Preserve pending, artifact, abstained, gap and not-observed. Query absence is not
rejection and no metadata certifies current owner validity or entitlement. Opaque
applied keys remain per-view observations, never global watermarks. Disable caching
on all query responses. Admission and projection are separate: a newly accepted
request can be not-observed until trusted tracking or a terminal owner event arrives.

## Alternatives considered

Direct SQL violates ownership. A second projection or Gateway pending-state merge
adds reconciliation semantics beyond this slice. Transparent HTTP proxying would
expose unexpected schemas, headers and private diagnostics. Public authentication
cannot be substituted with client-supplied identity headers in this trusted local
profile. Use the established projector protocol instead of a new shared service crate.

## Consequences

The Gateway can serve bounded, scoped observations and exact immutable outcomes.

## Compatibility and migration

Existing POST acknowledgement and descriptors remain unchanged. The current revision
validator supports terminal-only zero/one views; lifecycle support must evolve it
with the projector. A controlled upstream adapter proof does not establish complete
multi-service research acceptance. No schema or owner contract migration is needed.
Rollback removes the endpoint/serving version while retaining admissions and views.

## Security and operations

The profile trusts local processes as one configured operator and must not be
exposed or tunneled. Literal loopback destinations avoid DNS/proxy rerouting; no
credentials or incoming authority override the configured origin. HTTP diagnostics
are static and errors retain concrete causes only for explicit diagnosis.

## Validation

Protect real loopback tests for scope/Host/Origin refusal before I/O, exact outcomes,
observation states, malformed/oversized/redirected/unavailable responses, bounded
stalls and unsafe destinations; rerun existing admission proofs and default checks.
