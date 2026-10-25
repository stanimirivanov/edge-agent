# ADR-0059: Accept native daily evidence with explicit owner lineage

- Status: Accepted
- Date: 2026-10-25
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Accept native daily snapshots through a separate domain quality path and canonical
artifact. Preserve source dates and actual local times. Require explicit owner
observation lineage. Split PR11F-A quality/contracts from PR11F-B persistence and
terminal publication; neither slice authorizes vendor activation.

## Context

ADR-0056 preserves date-only temporal evidence; ADR-0057's native capture decision
is retained by ADR-0058. Captures are deliberately not whole-series quality results.
The precise daily-v1 format requires source instants and cannot represent a native
date honestly. The maintainer approved separating quality/artifact work from storage,
retention and terminal publication to keep review boundaries coherent.

## Decision

Add `NativeDailyObservation` with separate `DateOnlyTime`, raw values, context and
owner-assigned `ObservationVersion`. It has no source-instant conversion. Extend
the existing explicit `DailyQualityPolicy` with native snapshot checking, preserving
daily reference-age, complete calendar/mapping/action coverage, twelve-hour eligible
bucket age and exact raw-close threshold semantics. Do not claim independent-source
comparison or infer action completeness from missing actions.

The application binds a complete capture to its original native admission and
frozen schedule, rechecks current authority/cancellation, validates full reference
records and native provenance, and freezes a distinct accepted format. Observation
identities, revisions and row transformation are explicit owner inputs, never
generated from market values. Originals require original revisions; corrections
require a supplied, separately authorized same-scope predecessor and exact revision
links. Reused revisions cannot change native content or receipt/provenance.

Seal `edgeagent.evidence.native-daily.v1` as sorted JSON plus LF, bounded to 4 MiB.
Embed the original capture, row lineage, selected reference witnesses, quality
parameters, actual acceptance time, exclusive deletion deadline and optional prior
object digest. Clamp retention to original admission/current rights and the existing
daily 24-hour ceiling. No historic availability is inferred from a source date.

The wire constructor/decoder checks structural invariants, not that a trusted
producer actually ran quality or authenticated its source records. `DailyObject`
dispatches accepted native evidence separately from raw captures and precise objects.
The accepted artifact grants neither read access nor permission to publish.

## Alternatives considered

- Promote dates to bucket start/core close: invents source precision and availability.
- Alter daily-v1 or native-capture-v1 in place: changes published evidence meaning.
- Reuse capture shape as a quality grant: omits quality, reference and owner lineage.
- Infer IDs from date or price: conflates separate observations and correction history.
- Bundle persistence and activation: obscures quality review and bypasses unmet runtime prerequisites.

## Consequences

PR11F-A can prove quality and wire compatibility without external services.
PR11F-B must integrate scoped storage, pre-PUT retention inventory, current denial,
immutable observation/version guards, correction heads and atomic terminal recovery.
It must not treat this producer result as an authenticated runtime policy token.
PR15 still requires the applicable integration and explicit account/rights approval.

## Compatibility and migration

Append an independent schema/golden identity; preserve existing published bytes,
precise/capture meanings, terminal routes, admissions and SQL migrations. Existing
precise storage/publication APIs cannot take this new artifact. Old format readers
reject it; the new `DailyObject` variant is an additive workspace API change requiring
exhaustive callers to handle the new case. No runtime is activated or data rewritten.

## Security and operations

Use invented offline fixtures. No provider, credential, storage, SQL, model or broker
calls occur. Authenticate full retained records and source bytes before constructing
owner inputs, and authorize predecessor reads separately. Redact DTOs and source
data in diagnostics. Rollback before activation leaves the existing worker unchanged;
never remove retained evidence as compensation.

## Validation

Protect native acceptance with a named-test census and independent mutation tests.
Prove success, complete open/closed dates, current authority, immutable pins,
reference/action failures, exact thresholds, explicit lineage and strict bounded
decoding. Add schema/golden freshness and preserve every existing baseline. Run the
complete default harness, including strict Clippy, doctests and feature-unification
contracts. Storage/process/publication fault proofs belong to PR11F-B, not this ADR.
