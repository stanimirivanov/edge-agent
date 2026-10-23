# ADR-0049: Commit terminal acquisition publication atomically

- Status: Accepted
- Date: 2026-10-23
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Prepare verified stored evidence outside transactions; atomically commit receipt,
inbox, outbox and publication fences. Recover before repeating work. No provider
or worker activation is included.

## Context

Storage and durable budgets do not establish publication authority. Lost commit
confirmations can repeat acquisition; concurrent corrections can fork a series.

## Decision

The application owns finite receipt-first coordination and terminal preparation.
Ready reuses complete admission, current authority, quality freezing and verified
storage. A separate trusted local publication policy checks current retained
rights/provider review; read permission alone is insufficient. No external I/O
occurs inside database transactions.

Extend the existing MarketData PostgreSQL adapter with existing inbox/outbox
adapters in one commit. Lock the acquisition, retain exact reference-only event
bytes and return the first event on retries. Changed outcomes conflict. Ready
compares a scoped predecessor head and nonregressing capture/knowledge times,
and rejects changed scoped immutable observation versions. Version locks use
stable order. Sample database time after locks and again before commit.

Terminal receipts deny new attempts/calls. Rejections can retain unresolved
charged permits; trusted quiescence reconciliation remains possible. Historical
receipt recovery grants no object-read permission and cannot bypass current access.
Failed confirmation is uncertain; recover using the same command before any work.

## Alternatives considered

- Separate state/event/inbox commits: permit permanent half-publication.
- Direct publication after PUT: storage cannot serialize domain history.
- Recompute event metadata on retry: changes immutable event identity/content.
- Broker deduplication alone: finite retention and no correction fencing.
- Read permission as publication authority: omits admission/review checks.

## Consequences

Failed publication may leave private orphan objects for PR12 cleanup. Repeated
captures of a scoped series require explicit predecessor links. Local atomicity
does not imply exactly-once external effects or instantaneous global revocation.

## Compatibility and migration

Add private receipt/head/version tables after the ledger in the inbox/outbox
schema. Apply additive migrations before use. Existing v1 bus formats do not
change. Reverting code retains historical tables and immutable event evidence.

## Security and operations

No worker, provider/model call, broker topology or cloud backend is enabled.
Composition owns authenticated local policy, pool bounds and schema roles.
Uncertain effects never authorize automatic deletion, refunds or permit release.

## Validation

Five model proofs and four PostgreSQL proofs cover receipt-first recovery,
rollback after partial SQL work, competing outcomes/corrections, immutable
versions and retained charges. Exact census/mutation gates and checked SQLx
metadata protect the boundary; later M03 tests qualify real process failures.
