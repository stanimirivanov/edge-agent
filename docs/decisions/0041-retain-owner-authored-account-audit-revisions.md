# ADR-0041: Retain owner-authored account audit revisions

- Status: Accepted
- Date: 2026-10-21
- Milestone: M08 - Durable projections and evaluation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Retain complete bounded owner audit revisions atomically with simulation effects.
- Export checked records; never let projectors query peer private tables.
- Require genesis, contiguous revision/hash links and non-evicting duplicate receipts.
- Rebuild or resume disposable read models without granting financial authority.
- Keep public transport/deployment and performance qualification separate.

## Context

P08-P10 receipts preserve decisions but omit funding and full risk/hold state.
Command disposition `already_terminal` does not identify the terminal order state.
Replaying these facts alone would require guesses or private table coupling.

## Decision

Capture checked `edgeagent.account-audit.v1` owner exports under the existing
account-root/inbox transaction: genesis before the first successful mutation,
then one immutable full state per account revision. Bind run, configuration,
execution/postlude plans and risk policy. Reconstruct order views from original
checked journals. Include exact cash, cumulative balanced postings, positions,
remaining holds and original risk/valuation provenance. No price or risk
recalculation occurs in a read model.

Use an owner-authenticated bounded export API, not a new CloudEvents protocol or
peer SQL grants. The pure audit-domain reducer checks contiguous revisions,
canonical predecessor digests, same scope, duplicate content and financial
reconciliation before replacing its view. Gaps require prefix backfill; errors
leave both state and receipts unchanged. Checkpoint state/receipts together
with consumer progress in projector-owned storage and authenticate that storage.

## Alternatives considered

- Existing outcome-only replay cannot recover omitted genesis/risk facts.
- Peer SQL access violates ownership and couples readers to private journals.
- Ignoring old revisions hides conflicts and weakens duplicate evidence.
- Delta-only new events require a larger lifecycle protocol and migration now.
- Activating a public feed/server exceeds this finite synthetic qualification.

## Consequences

Snapshot retention and journal rereads cost storage/work. Explicit bounds and
existing transaction budgets fail closed; M10 must qualify load before production
activation. Views are read-only, revisioned and disposable, not fresh risk authority.
No new third-party crate is introduced; audit-domain uses existing pinned Serde,
Serde JSON and SHA-256 capabilities behind checked DTOs and replay receipts.
P13 still owns frozen-run evaluation; M09-M11 own rollout, operations and UI.

## Compatibility and migration

Add migration 0004 and owner SELECT/INSERT grants before new workers. Do not
rewrite existing v1 outcome schemas. Legacy nonzero accounts without history
fail closed instead of receiving fabricated genesis. Finish those runs with
old workers or explicitly start new account/run identities. Bootstrap affecting
positions/costs must precede capture. Rollback pauses ingress and preserves
financial/history records; old writers cannot safely continue logged accounts.

## Security and operations

Bound records to 1 MiB, pages to eight records, checkpoints to 3 MiB and retained
revision receipts to explicit capacity at most 8193. Decode unknown versions,
unknown fields, invalid scope and missing history as errors. Log no payloads.
Hash links/checkpoint checksums detect accidental corruption, not malicious
owner rewrites, authenticity or rollback; role/fencing/authenticated storage
remain mandatory. No external call occurs in account transactions.

## Validation

Protected default replay proofs exercise rebuild/checkpoint equivalence,
reordering/backfill, duplicates/conflicts, financial drift, scope, bounds and
corruption. Restricted independent PostgreSQL workers qualify atomic history
capture, rollback/death, lost-result retries and order/risk reconciliation.
SQLx metadata freshness covers the additive schema. These do not claim a public
query deployment, new broker feed, HA, throughput or provider/model qualification.
