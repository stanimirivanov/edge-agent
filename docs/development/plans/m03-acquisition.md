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
| 5 | M03-PR11E | Conditional Alpaca precise historical daily adapter | PR11B, PR11C, M03-CREDITS, semantics/rights review | D |
| 6 | M03-PR11F-A | Native-date whole-series quality and accepted artifact | PR11D | E |
| 6 | M03-PR11F-B | Native-date scoped storage, retention and atomic terminal publication | PR11F-A, PR10, PR12 | F |
| 4 | M03-PR13 | Offline-only daily worker with pinned invented profile | PR11A, PR11B, PR11C, PR12 | C |
| 6 | M03-PR14 | Worker/process death, acknowledgement loss and quota isolation proofs | PR13 | E |
| 7 | M03-PR15A | Owner-approved finite profiles and fail-closed offline preflight | PR14, PR11D, PR11E, PR11F-B | G |
| 8 | M03-PR15B1 | Independent PostgreSQL owner grants and atomic reservation/publication guards | PR15A | H |
| 9 | M03-PR15B2 | Authenticated bounded vendor worker, fencing, recovery and authorized cleanup composition | PR15B1, activation review, account/rights approval | I |
| 10 | M03-PR15C | Separately authorized opt-in live daily qualification and cleanup evidence | PR15B2, explicit live opt-in | J |
| Extension 1 | M03-PR16 | Finite streaming authority, session budgets and account connection fencing | PR11B, PR11C, profile decisions | G |
| Extension 2 | M03-PR17A | Bounded normalized streaming segments and scoped immutable storage | PR16, PR12 | H |
| Extension 3 | M03-PR17B | Durable segment inventory/retirement and atomic checkpoints/outbox | PR17A, PR12 | I |
| Extension 2 | M03-PR18 | Bounded WSS transport and synthetic local protocol proofs | PR16 | H |
| Extension 3 | M03-PR19 | First selected underlying-stream adapter with exact subscriptions | PR18, PR11C | I |
| Extension 4a | M03-PR20A | Offline bounded multi-source queues, fair dispatch and explicit unresolved/failure contracts | PR17B, PR19, PR13 | J |
| Extension 4b | M03-PR20B | Durable incidents, restore/readiness barriers and exact pending-publication recovery | PR20A, PR17B | K |
| Extension 4c | M03-PR20C1 | Complete owner-pinned IEX canonical interpretation, identity and native provenance | PR20B, PR19 | L |
| Extension 4d | M03-PR20C2 | Authenticated bounded offline streaming runtime, publication, readiness and integration proofs | PR20C1, PR20B | M |

## Acceptance boundaries

Maintainer-approved PR15 split: [PR15A](../qualification-preflight.md) implements
library-only exact-profile checks and an independent owner-assessment port. No production
approving adapter, credential access, provider request or dispatch capability ships.
A passing report is not live qualification, a reservation or permission to execute.
Initial profiles stay inside one current UTC credit day, require worst-case remaining
call credits, discard raw responses and cover the original licensed retention ceiling.
PR15B must authenticate actual owner/configuration/reference inputs independently,
compose bounded vendor workers with durable per-dispatch account/cancellation/authority
rechecks, recover uncertain work and integrate scoped accepted storage, atomic terminal
publication, maintenance and restore readiness. It cannot activate from profile hashes
or self-approved DTOs. PR15C alone runs explicitly authorized live qualification with
data-only credentials, finite budgets and cleanup evidence; missing prerequisites remain
not-run/nonzero. PR15A verification remains entirely offline.

PR15B is split at the security boundary: [PR15B1](../qualification-owner.md) implements
the maintainer-approved independent PostgreSQL owner role, exact durable grants and
transactional reservation/publication checks. Role and transaction proofs run offline
with actual restricted logins. It adds no vendor worker or automatic cleanup authority.
PR15B2 must consume this boundary, authenticate complete reference/configuration input,
implement actual worker/write fencing, compose provider recovery and accepted publication,
and authorize retention cleanup. PR15B1 does not complete PR15B or authorize PR15C.
PR15B2's [runtime composition](../qualification-runtime.md) adds independent complete
input loading, finite vendor intake, accepted-preparation recovery, durable process claims,
terminal publication/relay and exact owner-approved cleanup. Crashes retain uncertainty;
incomplete page traversals require reconciliation and a separately admitted run, not
automatic refetch. PR15C still requires explicit live opt-in and actual rights, credential,
restore/consumer and licensed-copy cleanup evidence.
PR15C's [opt-in harness](../qualification-harness.md) is a separately approved offline
preparation slice: reproducible file pins, bounded single-command supervision and private
reports. It grants no approval and cannot certify command effects. Actual live daily
qualification and cleanup evidence remain **not run** until separately authorized owner
inputs, credentials and operational evidence are supplied; this does not complete PR15C.

The conditional provider pair is Twelve Data first and Alpaca historical data
second; neither selection authorizes credentials, live calls or data use.
The [date-only prerequisite](../evidence-artifacts.md#date-only-temporal-prerequisite)
adds an independently versioned temporal value, not a fabricated precise source
instant. PR11D must integrate it into a new date-only observation/object path with
consumer format dispatch; old daily-v1 publication/storage remain unchanged and
must never silently accept date-only input. Alpaca reports precise source opening
instants; the maintainer approved preserving them instead of converting them to
dates. [ADR-0058](../../decisions/0058-preserve-alpaca-source-instants-and-bound-historical-sip-pages.md)
retains PR11D's native-date decision while replacing its Alpaca reuse assumption.
PR11D delivers that capture/object path and protected `twelve-data-conformance`
proofs, not activation of the precise-only worker. Before PR15, native
acceptance is split by maintainer approval: PR11F-A provides whole-series quality,
explicit owner lineage and the separately versioned accepted-native artifact;
PR11F-B integrates scoped storage, durable preparation/retention and atomic terminal
publication with explicit consumer compatibility. The [native acceptance guide](../native-daily-acceptance.md)
and [ADR-0059](../../decisions/0059-accept-native-daily-evidence-with-explicit-owner-lineage.md)
define this boundary. Native publication requires migration 0004, retained preparation
recovery and format-aware consumers; the existing offline worker stays precise-only.
Neither slice constitutes vendor activation completion.
PR11E reuses precise canonical
observations plus common metering/selection in parallel, with protected
`alpaca-conformance` proofs. Neither adapter activates the worker. Live qualification
still requires the applicable acceptance and authenticated runtime integration.
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

PR16 implements the [finite session and account ledger](../streaming-sessions.md)
offline-first with explicit limits and synthetic contracts/PostgreSQL proofs. Owner
policy fixes lifetime starts/opening debit, concurrent unresolved connections, start
spacing, expiry and duration; session budgets and frozen authority/selection survive
reconnects. Only independent owner-confirmed exact-generation quiescence releases
capacity, never process death, timeout or cancellation. It adds no transport, daemon
mode, vendor activation or readiness claim. PR17 adds segment/checkpoint publication;
PR18 enforces transport allocation/connect/idle limits; PR19 supplies the adapter;
PR20 composes bounded supervision and recovery. Before activation, these slices must
also address scoped metadata retirement, restore/reference freshness and any actual
provider entitlement shared with HTTP. Daily PR15C live qualification remains not run.

## Harness and outstanding decisions

Maintainer-approved PR20C split: [PR20C1](../iex-canonical-acceptance.md) freezes
complete owner-selected condition/reference/window/identity profiles and converts
IEX normalized observations without discarding native provenance. A matching sealed
configuration pin is not independent approval. PR20C2 must authenticate installed
owner inputs, enforce namespace consistency across profiles, persist complete native/
profile provenance and compose bounded tasks, restore/incident fences, publication
and domain readiness. PR20 and the streaming extension remain incomplete until that
runtime composition and synthetic integration qualification ship. No vendor activation
or live qualification is authorized by either slice.

Maintainer-approved PR20 split: [PR20A](../stream-supervision.md) implements a
pure offline bounded scheduler over caller-supplied canonical observations.
Explicit per-source/aggregate ceilings include unresolved FIFO heads. Round-robin
dispatch skips busy/failed lanes without evicting evidence or retrying dropped work.
Overload, binding failure and cancellation block health; health never grants evidence
readiness. No provider, task spawn, runtime selector, persistence or activation ships.
PR20B supplies protected sticky incidents, revision-fenced restore barriers and
exact publication recovery through existing preparations/receipts. Independent
owner approval cannot clear incidents or bypass pending work; begin a new restore
before reconstructing the transient scheduler. See [ADR-0070](../../decisions/0070-fence-stream-restores-with-durable-incident-barriers.md).
PR20C must supply
authenticated condition/calendar/identity semantics, bounded producer/task composition,
publication and existing domain readiness with synthetic integration proofs. Until both
are composed and qualified, PR20 and the streaming extension remain incomplete. Vendor activation
and original daily live qualification remain separately blocked/not run.

PR19 selects Alpaca IEX trades and minute bars for [offline protocol conformance](../alpaca-iex.md)
only. Explicit Xiex venue coverage is not historical SIP or consolidated evidence.
Each existing sealed session admits one contract (trade or minute bar); no combined
admission or extra account capacity is implied. Corrections/cancels require explicit
reconciliation. PR20 must compose authenticated semantics/identity, durable invalidity,
capture publication and recovery/readiness; no vendor activation ships in PR19.

Maintainer-approved PR17 split: [PR17A](../streaming-segments.md) supplies bounded
normalized capture and scoped create-only storage, not readiness or publication.
PR17B inventories before PUT, verifies current charged ranges/generations, atomically
advances capture checkpoint/chain/outbox, and integrates durable denial, scoped metadata
retirement and independently quiescent orphan cleanup. Stop/cancel preserves published
reads until original licensed expiry; withdrawal denies them immediately. Capture heads
explicitly record reconnects, skipped frames and first/retired history, never continuity.
Notifications carry only opaque IDs. PR18 remains parallel with PR17A;
PR20 depends on PR17B. Both slices remain offline.

PR18 supplies the [bounded WSS library](../streaming-transport.md) and protected
synthetic loopback TLS/protocol proofs. It commits one start before dialing and
charges all complete frames before provider decoding. Explicit upgrade/frame/
fragment/outgoing limits and original connect/idle/lifetime bounds fail closed;
uncertain accounting or abandoned operations stop socket reuse. No subscription,
vendor runtime, readiness or provider quiescence is implied. PR19 supplies the
selected provider adapter; PR20 composes fresh authority, recovery and supervision.

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
