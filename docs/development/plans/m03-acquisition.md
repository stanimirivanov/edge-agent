# M03 acquisition implementation plan

## TL;DR

- M03-PR1–PR10 are merged foundations; do not repeat their contracts or adapters.
- PR11A provides invented offline source/replay, not runtime or live qualification.
- Metered dispatch and selection precede connectors; retention precedes licensed activation.
- Daily completion and the streaming extension have separate acceptance boundaries.
- GitHub owns live delivery state; this plan owns intended scope and dependencies.

## Scope and ownership

The [daily profile](../market-data.md), [simulation profiles](../simulation-profiles.md)
and [milestone](../../roadmap/milestones.md#m03---market-data-fabric) own evidence
meaning and completion. Keep MarketData as the existing service composition root,
CloudEvents/NATS as the event spine, private PostgreSQL accounting/publication and
immutable scoped evidence storage. Do not import another daemon or event bus.

Default development and conformance use invented offline evidence. Live selection
requires approved vendors, exact capabilities/rights and explicit account limits.
No broker, model call, purchased account, crypto profile or automatic source
failover is authorized. Distinct provider coverage is never silently interchangeable.

## Ordered slices and dependency DAG

PR11 is split rather than renumbering the established retention/runtime gates.
Each implementation ships focused negative tests and docs with its behavior.

| Priority | ID | Cohesive outcome | Depends on | Parallel group |
|---|---|---|---|---|
| 1 | M03-PR11A | Network-free invented daily source and P05 replay | PR10 | A |
| 2 | M03-PR11B | Permit-bound per-page dispatch and usage projection | PR11A | B |
| 3 | M03-PR11C | Versioned instrument selection and early routing | PR11A | B |
| 3 | M03-PR12 | Retention, immediate revocation denial and orphan cleanup | PR10 | B |
| 4 | M03-PR11D | First selected daily-provider adapter and offline protocol conformance | PR11B, PR11C, vendor decision | C |
| 4 | M03-PR11E | Second interchangeable daily-provider adapter | PR11B, PR11C, vendor decision | C |
| 5 | M03-PR13 | Offline-first daily worker with explicit provider registry | PR11D, PR11E, PR12 | D |
| 6 | M03-PR14 | Worker/process death, acknowledgement loss and quota isolation proofs | PR13 | E |
| 7 | M03-PR15 | Authorized opt-in live daily qualification and cleanup | PR14, account/rights approval | F |
| Extension 1 | M03-PR16 | Finite streaming authority, session budgets and account connection fencing | PR11B, PR11C, profile decisions | G |
| Extension 2 | M03-PR17 | Versioned immutable streaming segments and atomic checkpoints/outbox | PR16, PR12 | H |
| Extension 2 | M03-PR18 | Bounded WSS transport and synthetic local protocol proofs | PR16 | H |
| Extension 3 | M03-PR19 | First selected underlying-stream adapter with exact subscriptions | PR18, PR11C | I |
| Extension 4 | M03-PR20 | Fair bounded multi-source supervision, readiness and recovery | PR17, PR19, PR13 | J |

## Acceptance boundaries

PR11A's [source guide](../offline-market-data.md) describes its finite synthetic
contract. It reuses P04/P05; it does not create a new workflow engine or authenticate
the supplied fixture/reference provenance. Daily quality remains a later acceptance
step after source fetch. Product services remain bootstrap programs until composition.

PR11B extends the existing ledger; it adds no parallel limiter. Every external
page, failure and uncertain call remains charged across restarts. Keep the daily
four-starts/minute and 15-second spacing ceilings unless a separate reviewed profile
changes them. Provider allowances may tighten, never implicitly raise, local limits.
Quota scope follows verified account/subscription sharing, not API-key count.

PR11C narrows requests/subscriptions before dispatch and routes before full payload
decode. Local filtering saves CPU/memory, not already-received bandwidth or billed
calls. Validate framing and process control/feed-sequence metadata before filtering;
never skip auth errors, resets, directory updates or heartbeat evidence. No native
ITCH connector is implied. Unknown mappings fail readiness rather than accepting all.

PR12 covers scoped objects and licensed private metadata/fingerprints. PR13 composes
existing authority, metering, conformance, quality and atomic terminal publication;
ACK only after confirmed commit. Minimal startup/readiness/shutdown are
**[Prerequisite from M10]**; M10 retains full operational qualification. PR14 tests
real owned process faults. PR15 remains opt-in, credential-safe and rights-gated;
missing prerequisites report not-run/nonzero, never passing live qualification.

The original daily M03 completion condition remains synthetic plus one qualified
live daily provider with common contracts/failure semantics. A second adapter
demonstrates substitution but needs its own live qualification before activation.
This does not qualify the additional streaming track or complete it by implication.

Streaming gets new versioned session/segment contracts, not changes to daily-v1
meaning or an indefinite 300-second acquisition. Explicit connection, lifetime,
frame/byte, reconnect, selection and recovery budgets are required before activation.
Use bounded per-source queues/fair scheduling. Overload invalidates readiness;
silent market-data dropping and unqualified gap-free claims are prohibited.

## Harness and outstanding decisions

Run `cargo xtask verify` for each implementation. PR11A adds the protected portable
`offline-source-isolation` gate and the separate Linux-only network-denial proof.
PR11B/PR11C and PR14 add protected metered-dispatch, routing and process-recovery
tests when implemented; these gates do not exist yet. Streaming adds protocol,
segment/checkpoint atomicity and bounded recovery proofs alongside implementation.

Open decisions: vendor pairs and comparable semantics; authenticated live catalogs
and rights; explicitly bounded intraday/streaming profiles; shared-account quota
allocation. Do not treat a free plan, OSS license or synthetic fixture as data rights.
