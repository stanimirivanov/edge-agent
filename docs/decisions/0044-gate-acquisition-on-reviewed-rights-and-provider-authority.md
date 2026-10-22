# ADR-0044: Gate acquisition on reviewed rights and provider authority

- Status: Accepted
- Date: 2026-10-22
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Application-owned admission requires reviewed rights and provider conformance.
- Bind exact principal/account/source/instrument/endpoint/audience/storage semantics.
- Recheck authority after I/O; canonical values alone cannot authorize their use.
- This slice activates no provider, storage, runtime or model integration.

## Context

PR4 checks whole-series quality, not rights or provider authority. API access,
source declarations and a current fetch of old dates cannot establish legal
permissions, reviewed feed equivalence or past knowledge. Those concerns must
remain outside pure market-value types and concrete transport implementations.

## Decision

Add `edgeagent-market-data-application`, depending only on the market domain.
It owns bounded acquisition requests, cancellation/deadline contracts, rights
admission, the `DailyProvider` port and reusable response conformance checks.
Require explicit remaining call/page/byte/time limits within the approved daily
profile. Limits describe one operation; they do not implement durable budgets.
The request carries its original admission time and exclusive command deadline.

Authenticate review catalogs before calling admission. Select latest known
reference revisions before filtering scope/time. Exactly one matching rights
record and provider review must authorize the exact request; ambiguity denies,
even when one matching record has expired. Renew the logical record by revision,
not by leaving competing grants. Unknown, denied, revoked, expired and missing
permissions fail closed. Records bind agreement reference, non-display/derived
processing, canonical storage, audience, raw storage, embargo, validity and
retention/deletion restrictions. Arbitrary URLs, credentials and agreement text
do not belong in these contracts. Endpoint IDs resolve through reviewed adapter
configuration; they cannot expand pagination authority.

Only admitted requests enter the provider port. Adapters enforce byte/time/page
bounds during I/O, cooperative cancellation and no hidden retries. Future
coordination owns persisted request counts and distributed account limits.
Dropping a future does not establish remote cancellation. There is no timeout
executor or exactly-once remote claim here.

After I/O, recheck current catalogs and require the original authority versions.
Reject cancellation, deadline/retention expiry, changed reviews, foreign command
or source, oversized/empty response inventories, out-of-range instruments/dates,
future receipts and embargo violations. Capture uses actual receipt; historical
mode requires qualified retained versions whose receipts precede its cutoff.
A fresh HTTP fetch cannot backdate canonical receipt times. Response conformance
does not replace PR4 completeness, reference/calendar/action or outlier checks.

Canonical retention is bounded from the admission check by 24 hours or shorter
license expiry/deletion/lifetime restrictions. Raw retention is explicit and
independently permitted, never longer than canonical retention. These deadlines
are use/retention contracts; later storage must enforce revocation and physical
deletion. Metadata deadlines alone do not delete bytes or authorize reads.

## Alternatives considered

- Put rights in provider DTOs: lets adapters redefine consumer policy; reject.
- Trust provider declarations or credentials: neither establishes reviewed authority.
- Fold acquisition into the pure domain: introduces effect/identity policy into values.
- Build a workflow/retry engine now: exceeds this foundation; retain approved sequencing.

## Consequences

One application seam supports later HTTP, synthetic and retained-input adapters.
The pure domain remains provider-agnostic. Reviewed record constructors are not
cryptographic authentication or legal approval. External catalog ownership and
review procedures remain required before any live activation.

## Compatibility and migration

Additive private-workspace API and a calendar-day count accessor; no event,
database or existing evidence encoding changes. No deployable is activated.
Revert the crate and its registration without a data migration.

## Security and operations

Use data-only account identities, never broker credentials. Authenticate principal
and review catalogs, retain withdrawals, and recheck current rights before reads,
publication and later use. No licensed payload or secret enters diagnostics.
The supplied clock is trusted orchestration input, not provider-supplied time.
Live provider selection, rights approval and operational deployment stay deferred.

## Validation

`cargo xtask verify --check market-data-conformance` protects six synthetic
contract proofs, including scope, permission, revision, capability, time,
retention, cancellation and response rejection paths. The default gate includes
this check, census mutation tests and enforced dependency boundaries. These
tests qualify contracts, not a real vendor's network/parser behavior or license.
