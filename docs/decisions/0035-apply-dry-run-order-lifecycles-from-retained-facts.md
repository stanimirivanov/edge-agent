# ADR-0035: Apply dry-run order lifecycles from retained facts

- Status: Accepted
- Date: 2026-10-20
- Milestone: M07 - Dry-run execution
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- P07 adds a bounded pure order projection in `simulation-domain::orders`.
- Apply contiguous immutable facts, including no-fill decisions. Exact duplicates
  never reapply a transition or resample a retained simulation attempt.
- Only the exact required `dry_run` mode constructs a submission. No live mode,
  broker interface, worker or new dependency is introduced.
- P08 supplies atomic account admission, reservations, persistence and product wire
  contracts. P09 qualifies execution algorithms. This does not complete M07.

## Context

Crash/retry and fill/cancel/expiry races require explicit order identity and lifecycle
semantics before shared-account admission can reserve cash or long inventory.
Virtual time alone cannot bind execution decisions to a run's frozen evidence.
Premature service activation would expose an unfunded path before P08 exists.

## Decision

Use immutable run/account/strategy/order ownership, positive whole shares/contracts,
buy-to-open/sell-to-close intent, market/limit styles and an explicit half-open
lifetime. Pin full option reference metadata. Initially permit only cash-settled
European index-style options whose entire lifetime fits effective/trading bounds
and whose specification was known at submission. Preserve nanosecond comparisons
against millisecond references. Structure does not establish product qualification.

Apply contiguous revisions with nonregressing virtual occurrence time. Compare
complete content on retained event IDs before lifecycle/capacity checks; conflicting
reuse fails without mutation. Recover gaps rather than apply speculatively. Retain
the complete journal up to an explicit 1..4096 bound; never evict deduplication
evidence. Canonical revisions, not broker arrival time, resolve same-time races.

Acceptance enables fills. Partial fills preserve unfilled quantity; pending cancel
does not release reservations and still permits fills. Cancel rejection restores
working/partial status; confirmation cancels only the remainder. Full fill, rejection,
cancellation and explicit expiry are terminal. No new execution occurs at/after the
exclusive deadline. Expiry preserves historical fills but does not settle positions.
Check incremental fill units, checked total quantity, ticks and directional limits.

The consumer-owned synchronous `SimulationExecution` port proposes fill/no-fill,
without broker capability. Require expected model revision/configuration, ready
evidence at the run's current time, exact frozen provenance/artifact membership in
the consumed prefix, and matching bar/trade or pinned-specification option quote.
Reassessment at a later execution instant may reuse identical consumed evidence;
it preserves frozen availability and must independently pass current readiness.
The coordinator assigns fact identity/revision/provenance, independently validates
output and returns an uncommitted fact. Retain no-fill decisions too: replay must
not resample until a desired fill appears. Retained attempts bypass model execution
but still require the same input bindings.

## Alternatives considered

- Mutable status setters hide race order and cannot reconstruct duplicate facts.
- Status-only snapshots discard exact deduplication evidence. Durable compaction
  requires an explicit history/identity design, not silent eviction.
- A generic live/paper broker interface introduces a forbidden capability.
- A service path before funded account admission creates unsafe parallel execution.

## Consequences

New no-fill attempts consume revisions; exhaustion fails closed. P09 must bound
attempt scheduling and shared liquidity, pin algorithms and retain RNG continuation.
New IDs do not permit repeated consumption of the same market liquidity. A pure
result never authorizes account reservation release outside its transaction.

## Compatibility and migration

Additive Rust API only; no CloudEvents/schema/database change or service activation.
P08 introduces product messages through existing registry/schema gates with atomic
admission. Recover existing orders from their original submission and full journal,
never a new empty aggregate with an existing ID. Retain run outputs/continuation
together. Reverting this unused foundation needs no deployed migration.

## Security and operations

IDs/digests/revisions are claims, not authentication, fencing or artifact integrity
verification. P08 must authenticate principals, verify retained artifacts, lock the
account root, enforce P06 risk/funding/inventory and atomically commit order facts,
reservations, inbox/outbox and P05 continuation. Discard uncommitted continuation
after a failed transaction. Port implementations must be I/O-free and deterministic,
without hidden time/entropy; P09 qualifies those claims. P10 owns settlement.
Diagnostics redact quantities/prices and never include source payloads/credentials.

## Validation

Run `cargo test --locked -p edgeagent-simulation-domain` and `cargo xtask verify`.
Synthetic tests cover units/options/lifetimes, exact duplicates/conflicts, partial/
full fills, cancellation/expiry races, revision/time/capacity/overflow failures,
replay and restored deterministic IDs. Simulation tests check consumed input binding,
model pinning, uncommitted output and retained no-fill decisions without resampling.
No database admission, realistic fills, balanced ledgers or broker qualification is
claimed. See [dry-run orders](../development/dry-run-orders.md).
