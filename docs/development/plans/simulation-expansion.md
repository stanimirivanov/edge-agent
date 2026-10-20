# Research and simulation expansion plan

## TL;DR

- P01 records the approved scope; P02-P13 implement it within existing milestones.
- Keep the approved M03 daily-data sequence and completion gate. Integrate new
  contracts before their provider/schema consumers, without duplicate ports.
- No live execution, selected vendor, new milestone or upstream runtime adoption.
- GitHub owns live delivery state; this file owns intended scope and sequencing.

## Ordered slices

P05's implemented finite-run contract is documented in
[deterministic runs](../deterministic-runs.md). Application-owned atomic checkpoint
storage and P07/P09/P13 consumers remain separate work.

P06's pure policy and recovery contract is documented in
[simulation risk](../simulation-risk.md). P08 supplies authoritative account reads,
atomic risk/reservation enforcement, persistence and multi-process qualification.

P07's pure order lifecycle and proposal contract is documented in
[dry-run orders](../dry-run-orders.md). P08 supplies funded admission and product wire
integration; P09 qualifies models and ledger effects. No simulator service path is
activated by this foundation, and M07 is not complete.

The [profiles](../simulation-profiles.md) own data meaning, supported/rejected
products, account consistency and qualification. The
[roadmap](../../roadmap/milestones.md) owns milestone outcomes. These slices
enrich corresponding work items, not a second implementation of them. P01 adds
no type/port stub or cross-milestone runtime prerequisite. Later implementation
PRs ship their negative tests and executable acceptance evidence together.

| ID | Milestone | Cohesive outcome | Depends on | Acceptance |
|---|---|---|---|---|
| P01 | M03 | Record vendor-neutral profiles, supersession and rejection gates | M03 values/reference baseline | Document/ADR policy and default verification |
| P02 | M03 | Checked derivative specifications, units and session/expiry semantics | P01 | Effective/knowledge boundaries; unsupported combinations and deliverables |
| P03 | M03 | Canonical bars/trades/index levels/option snapshots and provider capabilities | P02 and existing quality foundations | Synthetic conformance; no vendor types or implicit proxy substitution |
| P04 | M03 | Bounded continuity, recovery and evidence-readiness barriers | P03 | Duplicates, overlap, heartbeat gaps, reset, overflow and exhaustion |
| P05 | M04 | Virtual clock, canonical input sequence, deterministic IDs/RNG and run manifest | P03; P04 for sequenced feeds | Repeat/restart/same-time ordering produces identical domain results |
| P06 | M05 | Versioned account/strategy/session risk and authorized breach/reset policy | P02-P03 | Aggregate limits, stale valuation and session rollover cases |
| P07 | M07 | Event-applied dry-run order lifecycle and simulation-only execution port | P02, P05 | Partial fill/cancel/expiry races and conflicting retries |
| P08 | M07 | Atomic account admission, revisions and durable reservations | P06-P07 | Independent PostgreSQL workers cannot overspend; rollback/restart/ACK loss |
| P09 | M07 | Evidence-driven fills, fees/liquidity and operation latency | P04-P05, P08 | Limit protection, partial fills, balanced ledger and reproducibility |
| P10 | M07 | Qualified cash-settled European expiry/settlement | P02-P03, P08-P09 | Exact ITM/ATM/OTM cash flows; missing evidence never fabricates completion |
| P11 | M04 | Independently qualified Greeks/IV analytics | P02-P03 | Finite inputs, explicit convergence failures and near-expiry references |
| P12 | M08 | Account/order/risk replay and revisioned audit views | P08-P10 | Rebuild/checkpoint/reordered-event cases without duplicate effects |
| P13 | M08 | Frozen-run fidelity and cost/latency/liquidity sensitivity evaluation | P05, P09-P10, P12; P11 only when used | Reproducibility and chronological evidence, not performance promises |

P11 can run in parallel after its prerequisites and does not block quote-driven
fully funded simulation. M06 explanations are optional; they cannot participate
in risk, settlement or numeric authority. Existing M03-PR4 quality work remains
the next original slice; P02-P04 must precede new-profile provider/schema
consumers. Existing M03-PR11 implements a subsequently qualified adapter, not a
vendor chosen by the superseded ADR. Do not silently expand daily-v1 semantics.

## Deployment and user-facing follow-through

- M01 keeps license/dependency, architecture and default-verification gates.
- M02's completed spine is reused; new product contracts enter its existing
  registry/compatibility mechanisms, not a reopened milestone or replacement bus.
- M09 qualifies one authoritative fenced database primary per account ownership
  domain on each claimed profile; no active-active cross-cloud account writes.
- M10 qualifies failover/restore, lock waits/backpressure, bounded hot-account
  work, pause/reset audit and stale-evidence failure behavior. P08 correctness
  tests cannot be postponed to operational readiness.
- M11 exposes profile eligibility, evidence age, reservations, risk and projection
  revision/lag without browser financial calculations or risk authority. Initial
  UI scope still excludes emergency/operator controls.

## Harness and unresolved qualification

Every implementation PR runs `cargo xtask verify` and adds focused tests. P08
adds the protected `account-admission-processes` gate to the existing runner and
local-platform CI profile. It proves synthetic-only reservations and recovery;
P09 still owns qualified quote/fill/fee/ledger algorithms and service activation.
Provider selection/rights, authoritative index and settlement sources, concrete
freshness/skew/resource limits and product-specific cutoff/rounding rules remain
qualification inputs. Unknown values fail closed, never become guessed defaults.
