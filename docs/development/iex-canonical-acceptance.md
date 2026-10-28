# Owner-pinned IEX canonical acceptance

## TL;DR

- PR20C1 interprets normalized IEX input under explicit complete owner profiles.
- A matching configuration pin proves consistency, not independent owner approval.
- Unknown conditions, changed references, foreign partitions and invalid times reject.
- Native provenance survives alongside canonical values; no sequence is invented.
- PR20C2A supplies authenticated owner inputs; PR20C2B/C own publication and runtime.

## Profile and ownership

`IexCanonicalProfile` in `market-data-http` accepts one IEX snapshot contract:
trades OR raw intraday minute bars, with nanosecond source time. Supply source,
transformation, exact mapping/calendar references and explicit source-time windows.
There are no venue, timezone, condition, operating-window or identity defaults.
Calendar records label an explicitly reviewed exchange-local date; the owner verifies
timezone/DST and actual session notices. Capture windows may include reviewed extended
hours and never make raw minute bars core-session aggregates or simulation-eligible.

Supply 1..=128 identity partitions, at most 64 condition rules, at most 16 source/
canonical codes per rule, and at most 8 graphic ASCII bytes per source code. These
are portable ceilings, not recommended operating settings. Windows span at most
48 hours; encoded profiles cap at 64 KiB. Selected instruments remain bounded by
the existing 32-member selection. Reject duplicate namespaces/partitions, conflicting
references/windows for one instrument, closed calendars and incompatible mappings.

Condition rules match exact sets, not individual-code heuristics. Raw codes inside
each rule and canonical codes must be sorted/unique. Incoming source order is preserved
in native provenance while lookup compares the sorted set. Empty conditions require
an explicit empty rule. A mapped code is an owner interpretation, not a built-in
assertion of fill eligibility. Bars have no source condition field or trade rules.

The profile's versioned array-only canonical bytes include complete reference values,
checksums, known times, IDs, revisions, source, contract/semantic reference, timezone
revision, calendar hours, windows, namespaces and rules. Rule/lane order normalizes;
source-condition/canonical code ordering is validated rather than silently repaired.
Object-map feature unification cannot alter this array-only encoding.

Bind the digest to `StreamingSessionData.configuration` before the independent owner
installs the admission. During acceptance, compare complete selection witnesses and
resolve exact current reference histories. A profile hash, a matching sealed session
or caller-supplied catalogs cannot authenticate themselves. PR20C2A verifies actual
independent PostgreSQL owner installation and complete approved input bytes;
reference-catalog truth remains the trusted owner's responsibility.
This library introduces no approving production adapter or runtime mode.

## Identity, values and time

The owner reserves a nonzero 64-bit namespace per source/instrument/date/evidence-kind
partition and per exchange/tape for trades. The high half of `ObservationId` is that
namespace; its low half is the exact positive u64 provider trade ID or nonnegative
Unix minute bucket start. IDs larger than 2^53 remain exact. Replay/replicas using
the same approved profile retain the same ID; receipt/frame/item changes do not
create another logical fact. The library checks uniqueness inside one profile only.
PR20C2A enforces retained cross-profile/account namespace ownership; a changed profile
cannot reuse a namespace for a different identity scope. Private scope erasure keeps
an opaque non-reusable namespace tombstone, not reconstructible historical dedup.

Emit revision 1, preserve source and original receipt times, and leave sequence absent.
Receipt cannot precede the source event or bar end. Minute buckets must be aligned,
exactly 60 seconds and entirely inside the explicit source window. The supplied
evaluation instant/current authority and original capture receipt window must agree.
Local receipt is not a new market time. No trade ID, count, frame or timestamp becomes
source continuity. Protocol corrections/cancels/updated bars still require a durable
incident and independently reconciled session, not an invented successor revision.

`IexAcceptedObservation` retains both the canonical value and the complete original
`IexObservation`, plus the profile digest. Do not persist only `canonical()`: that
would discard source condition order/exchange/tape or bar trade count/VWAP. Coordinates
remain local capture positions, not confirmed accounting proof. PR20C2 must store this
native/profile provenance under the existing rights/retention boundary before exposing
evidence to consumers. No new public evidence format or persistence path ships here.

## Failures and readiness

Errors have fixed payload-free categories; debug output redacts profiles, rules,
references, native values and accepted observations. Bound untrusted tokens before
copying. Rejection does not schedule retries, dial providers, release account slots,
change durable counters or publish evidence. Runtime composition must persist the
appropriate incident and fail readiness on unknown/conflicting critical evidence.

Canonical construction validates values and interpretation consistency, not source
authenticity, durable identity history, immutable finality, rights provenance or
readiness. Use existing `ContinuityTracker`/`ReadinessPolicy`, never infer readiness
from successful conversion, capture, an empty queue or restored control alone.

## Verification and remaining integration

Run `cargo xtask verify --check iex-canonical-acceptance`, then the default
`cargo xtask verify` in CI. The protected census covers exact trade/bar values,
stable replay IDs, complete native provenance, canonical profile encoding, unknown
rules/partitions, calendar changes, authority/cancellation, bounds and redaction.
Fixtures are invented, require no credentials or network and claim no actual venue
calendar or eligibility rule. No provider or public test stream is contacted.

PR20C2A adds [complete independent owner input installation and namespace ownership](stream-owner-inputs.md).
Use `owner_inputs` to derive claims from this profile and the exact sealed selection,
then require the restricted loader to return the actual protected matching bytes.
PR20C2B adds [atomically linked native companion provenance](stream-native-provenance.md);
PR20C2C owns fresh restore/incident checks, bounded source/task supervision,
publication/recovery, domain readiness and synthetic integration qualification.
Vendor activation and original daily live qualification remain separately blocked.
See [ADR-0071](../decisions/0071-pin-iex-canonical-interpretation-to-complete-owner-profiles.md).
