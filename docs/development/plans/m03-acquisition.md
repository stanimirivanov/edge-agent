# M03 acquisition implementation plan

## TL;DR

- M03-PR1–PR10 are merged foundations; do not repeat their contracts or adapters.
- PR11A provides invented offline source/replay, not runtime or live qualification.
- PR11B binds HTTP pages to durable permits; PR11C supplies pinned selection/routing.
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
| 4 | M03-CREDITS | Durable daily allocation and endpoint-weight accounting prerequisite | PR11B, PR12 | C |
| 4 | M03-DATE | Native date-only temporal contract and reviewed bucket provenance | PR3, PR6 | C |
| 5 | M03-PR11D | Conditional Twelve Data daily adapter and offline protocol conformance | PR11B, PR11C, M03-CREDITS, M03-DATE, semantics/rights review | D |
| 5 | M03-PR11E | Conditional Alpaca historical daily adapter | PR11B, PR11C, M03-CREDITS, M03-DATE, semantics/rights review | D |
| 6 | M03-PR11F | Native-date whole-series acceptance, scoped storage and terminal publication | PR11D, PR10, PR12 | E |
| 4 | M03-PR13 | Offline-only daily worker with pinned invented profile | PR11A, PR11B, PR11C, PR12 | C |
| 6 | M03-PR14 | Worker/process death, acknowledgement loss and quota isolation proofs | PR13 | E |
| 7 | M03-PR15 | Authorized opt-in live daily qualification and cleanup | PR14, PR11D, PR11E, PR11F, activation review, account/rights approval | F |
| Extension 1 | M03-PR16 | Finite streaming authority, session budgets and account connection fencing | PR11B, PR11C, profile decisions | G |
| Extension 2 | M03-PR17 | Versioned immutable streaming segments and atomic checkpoints/outbox | PR16, PR12 | H |
| Extension 2 | M03-PR18 | Bounded WSS transport and synthetic local protocol proofs | PR16 | H |
| Extension 3 | M03-PR19 | First selected underlying-stream adapter with exact subscriptions | PR18, PR11C | I |
| Extension 4 | M03-PR20 | Fair bounded multi-source supervision, readiness and recovery | PR17, PR19, PR13 | J |

## Acceptance boundaries

The conditional provider pair is Twelve Data first and Alpaca historical data
second; neither selection authorizes credentials, live calls or data use.
The [date-only prerequisite](../evidence-artifacts.md#date-only-temporal-prerequisite)
adds an independently versioned temporal value, not a fabricated precise source
instant. PR11D must integrate it into a new date-only observation/object path with
consumer format dispatch; old daily-v1 publication/storage remain unchanged and
must never silently accept date-only input. PR11E reuses that integration.
PR11D delivers that capture/object path and protected `twelve-data-conformance`
proofs, not activation of the precise-only worker. Before PR15, native
acceptance PR11F must integrate whole-series quality, scoped storage and terminal
publication with explicit consumer compatibility. PR11E can reuse capture in
parallel; live qualification cannot proceed without native acceptance integration.
The [daily-credit prerequisite](../acquisition-ledger.md#reviewed-daily-credits)
must precede adapters. It extends the existing ledger, requires an exclusive
owner-reviewed allocation, known opening debit and maximum endpoint prices, and
does not infer advertised free-tier entitlements. Existing offline composition
remains unchanged. PR11D/PR11E must qualify endpoint costs, reset/billing semantics,
date-only precision/provenance, permitted authentication and applicable non-display
rights. They must not weaken quotas or treat public pricing as a data license.
Before long-lived activation, add a reviewed non-resetting policy renewal path if
the immutable review's expiry would be reached. PR15 remains blocked by these
adapter/activation prerequisites; this accounting slice performs no provider calls.

PR11A's [source guide](../offline-market-data.md) describes its finite synthetic
contract. It reuses P04/P05; it does not create a new workflow engine or authenticate
the supplied fixture/reference provenance. Daily quality remains a later acceptance
step after source fetch. PR13 composes MarketData only for invented offline data;
other services remain bootstrap programs until their own composition.

PR11B extends the existing ledger; it adds no parallel limiter. Every external
page, failure and uncertain call remains charged across restarts. Keep the daily
four-starts/minute and 15-second spacing ceilings unless a separate reviewed profile
changes them. Provider allowances may tighten, never implicitly raise, local limits.
Quota scope follows verified account/subscription sharing, not API-key count.
The implemented [metered adapter](../provider-http.md) retains uncertainty and
supports accounting-only confirmation retries. PR13/PR14 still own worker fencing
and process-recovery qualification before live activation.

PR11C narrows requests/subscriptions before dispatch and routes before full payload
decode. Local filtering saves CPU/memory, not already-received bandwidth or billed
calls. Validate framing and process control/feed-sequence metadata before filtering;
never skip auth errors, resets, directory updates or heartbeat evidence. No native
ITCH connector is implied. Unknown mappings fail readiness rather than accepting all.
The [selection guide](../instrument-selection.md) describes the implemented frozen
catalogs, narrowed request/source construction and invented header adapter. It
does not persist watchlists or configure live subscriptions. PR13 must pin the
chosen configuration durably; admission-v2 binds its compiled profile and selection
without changing legacy admission-v1 meaning.

PR12's [retention boundary](../evidence-retention.md) covers scoped objects and
licensed private metadata/fingerprints with opaque denial tombstones and
generation-fenced account blocks. PR13 must supply authenticated current authority,
write/provider quiescence evidence, maintenance scheduling and restore/readiness
gates; no timeout can stand in for worker fencing. PR13 composes
existing authority, metering, conformance, quality and atomic terminal publication;
ACK only after confirmed commit. Minimal startup/readiness/shutdown are
**[Prerequisite from M10]**; M10 retains full operational qualification. PR14 tests
real owned process faults. The [offline worker](../offline-worker.md) is local-only;
it does not qualify distributed fencing, authenticated live catalogs or cloud restore.
PR11D/PR11E remain prerequisites for vendor activation, not offline composition.
Activation additionally requires reviewed runtime authority and fencing; installing
an adapter does not enable it. PR15 remains opt-in, credential-safe and rights-gated;
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
PR11B adds the protected `metered-provider-dispatch` gate. PR11C adds
`instrument-selection-routing` and selected-source replay proofs. PR14's
process-recovery gate is `offline-worker-recovery`: actual worker death before
terminal COMMIT and after commit before ACK, request/confirmation loss, owned
same-store broker restart and cross-process account quota isolation. It retains
uncertain charges until owner-confirmed quiescence and proves zero external quota
for synthetic work; it does not qualify vendors, distributed fencing or HA. PR13 adds protected
`offline-worker-contracts` and local-platform `offline-worker-processes`; clean
process restart does not qualify injected worker death or acknowledgement loss.
Streaming adds protocol,
segment/checkpoint atomicity and bounded recovery proofs alongside implementation.

Open decisions: vendor pairs and comparable semantics; authenticated live catalogs
and rights; explicitly bounded intraday/streaming profiles; shared-account quota
allocation. Do not treat a free plan, OSS license or synthetic fixture as data rights.
