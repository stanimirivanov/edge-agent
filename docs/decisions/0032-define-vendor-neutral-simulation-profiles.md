# ADR-0032: Define vendor-neutral simulation profiles

- Status: Accepted
- Date: 2026-10-19
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes: ADR-0003, ADR-0031
- Superseded by:

## TL;DR

Preserve raw daily evidence and the dry-run-only safety boundary. Defer vendor
selection and approve separately qualified intraday/options profiles. Model all
cash/physical and European/American combinations, but first simulate only fully
funded long cash-settled European index-style options. This records requirements,
not working runtime support.

## Context

The accepted expansion needs underlying bars/trades, option quote snapshots,
account-wide risk and reproducible simulation, not live trading. ADR-0003's
blanket options exclusion and ADR-0031's selected vendor candidate no longer
match that scope. Keep their historical text; this decision replaces those
choices and explicitly carries forward the remaining safeguards.

## Decision

Use [research and simulation profiles](../development/simulation-profiles.md)
as the canonical capability/acceptance matrix. Retain `us-equity-daily-v1`'s
unadjusted provider-day semantics, bounds, point-in-time knowledge, reference
authority, rights, retention and budgets unchanged. Add distinct planned
intraday-underlying and options-quote profiles, with capability and resource
qualification before activation. Massive and other earlier candidates are
historical research, not selected providers. Provider selection remains deferred.

Retain ADR-0003's dedicated simulator ownership, stable identities, immutable
lifecycle/ledger, atomic state/outbox publication, deterministic costs/fills,
long-only fully funded initial scope and absence of broker connectivity. Extend
instrument modeling and later simulation eligibility only as specified by the
new profiles. Unknown exercise, deliverables, settlement evidence, rights or
reference coverage fail closed; modeling a product does not enable it.

ExecutionSimulator owns all account mutations. Later implementation will
serialize account-wide and per-strategy risk through the account row in the
existing inbox/domain/outbox PostgreSQL transaction. One fenced writable primary
is required; broker partitioning is not a consistency guarantee. Canonical
persisted input order and deterministic identities/randomness separately govern
reproducibility. These requirements need executable concurrency/recovery proofs.

Keep NATS/PostgreSQL, service ownership and [ADR-0013](0013-select-durable-workflow-execution.md)'s
long-running workflow decision. This decision selects no new runtime/dependency
and authorizes no live broker port, provider credential or model call. Use Nautilus
ideas only, without code copy; MIT source eligibility still needs provenance,
notice retention and implementation review. Deliver through the
[P01-P13 milestone plan](../development/plans/simulation-expansion.md).

## Alternatives considered

### Broaden daily-v1 or enable every modeled option

This would silently change evidence meaning and confuse metadata with exercised
capabilities. Reject; use explicit profiles and fail unsupported combinations.

### Commit a provider or adopt an upstream runtime now

This couples domain contracts to unqualified coverage/rights or imports an
unneeded runtime/license footprint. Reject; qualify interchangeable capabilities
and adapt designs before selecting dependencies or vendors.

### Rely on broker ordering or independent strategy budgets

These do not guarantee a consistent shared-account view across replicas.
Reject; one account transaction evaluates both account and strategy limits.

## Consequences

### Positive

The roadmap can support intraday/0DTE research without weakening provenance,
account integrity, portability or the dry-run-only boundary.

### Negative

Options require new data contracts, specifications and lifecycle proofs. Unknown
index sources, freshness budgets or settlement rules block qualification.

### Neutral or follow-up

Product services remain bootstrap programs. P02-P13 must implement and prove
these requirements; M09/M10 must qualify operational assumptions. The original
M03 live-daily-data gate is retained, not satisfied by this documentation change.

## Compatibility and migration

No runtime API, schema, database, dependency or configuration changes. Retained
daily evidence keeps its meaning. Future product contracts use the existing
schema registry and compatibility process; no old payload acquires new units or
time precision. Update only supersession metadata in ADR-0003 and ADR-0031.
Reversing an accepted scope decision requires a new ADR, not edited history.

## Security and operations

No live execution mode, broker SDK, account connectivity, credential or endpoint.
Live market data requires separate source/rights/retention qualification and
does not grant permission to route orders. Default tests stay synthetic/offline;
source code licenses do not convey market-data rights. Physical/American
simulation and margin/short-option behavior remain unavailable until approved
implementation and proofs exist.

## Validation

Run `cargo xtask verify`. Register ADR identity and reciprocal supersession in
the existing immutable ledger/index and advance its independent deletion floor.
Review the profile rejection matrix and milestone dependencies. P01's checks
validate governance, not account serialization, numerical correctness, provider
behavior or live-data qualification; those need the named future proofs.
