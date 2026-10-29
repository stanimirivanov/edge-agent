# Frozen research evidence

## TL;DR

- M04-PR3 supplies a pure minimum-evidence gate, not momentum or a worker.
- The owner selects historical instruments, listings, mappings and sessions explicitly.
- Authenticate owner assertions and verify evidence bytes before constructing domain inputs.
- Run `cargo xtask verify --check research-evidence --python python3`.

## Boundary

[`research-domain`](../../crates/research-domain/src/lib.rs) reuses existing
market-data identities, Gregorian session dates and validated USD price values.
It depends on no wire contract, framework, provider, storage, clock or hashing
library. The application must resolve reference histories using existing market
quality policy, authenticate the caller/audience and owner, verify artifact digests
and pin implementation/configuration/calendar/mapping revisions before use.
Public `FrozenSeries` fields are assertions, not proofs or permission tokens.

The policy does not reimplement exchange calendars or acquisition rights.
Dates are the exact selected open-session census, not inferred weekdays. The
supported universe is explicit historical instrument/listing membership; current
listings cannot silently substitute. The request lookback needs lookback+1 closes,
up to 253, and at most 64 instruments. Raw USD closes are the only qualified basis;
other currencies must fail adapter conversion and adjusted data abstains.

## Point-in-time behavior

Decision time and maximum age are explicit milliseconds, never wall-clock reads.
Event, observation and availability times must be nonnegative and ordered.
Availability must include all owner knowledge/processing delays. Future owner
knowledge or availability abstains even when the event is historical. Freshness
uses the latest market event close, not its recent receipt time. Age equal to the
maximum and availability equal to decision time pass; rights expire exclusively.
The application must reauthorize later use independently of historical assessment.

## Results and compatibility

A malformed policy/selection returns `InvalidSelection`. Oversized input is rejected
before sorting. Accepted-request abstention precedence is: missing entitlement,
unsupported universe, invalid evidence, future evidence, stale evidence, insufficient
history. Bound violations take precedence before assessment. Within bounds, all
required instruments are checked and the highest-priority reason wins regardless
of input order. Missing one candidate or session never produces a partial success.
No transport DTO or v1 wire format changes; an outer adapter maps `code()` explicitly.

The sealed eligible view borrows immutable inputs and orders candidates by selected
instrument and closes by session. Duplicate series/sessions, foreign sessions,
changed revisions/basis/listings, zero prices/digest sentinels and failed owner
quality assertions cannot pass. This view does not certify actual source bytes or
licenses, and performs no calculation, ranking, persistence or publication.

## Verification

The default `research-evidence` gate protects five named proofs covering complete
selection, exact temporal/rights boundaries, revision/basis/lineage conflicts,
input permutations and malformed/bounded selection. No network, service, system
clock or random data is used. Existing market-data quality tests retain ownership
of calendar, corporate-action and provider semantics.
