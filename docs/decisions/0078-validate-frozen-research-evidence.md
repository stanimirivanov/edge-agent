# ADR-0078: Validate frozen research evidence

- Status: Accepted
- Date: 2026-10-29
- Milestone: M04 - Deterministic research
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Keep research minimum-evidence policy pure and inward of adapters.
- Use an explicit historical universe/session census and pinned revisions.
- Return a sealed complete view or a stable order-independent abstention.

## Context

M04 needs a deterministic gate before momentum. MarketData already owns provider,
calendar, action and rights semantics. Research must not infer missing evidence
or accept current evidence relabeled as historical inputs.

## Decision

Add research-domain with only a market-data-domain dependency for existing value
objects. An owner-selected policy binds implementation, configuration and calendar
revisions, historical instrument/listing mappings, lookback and decision/freshness
bounds. Validate complete session membership, revisions, raw USD basis, positive
closes and event/receipt/availability chronology. Retain exact evidence digests in
an immutable borrowed eligible view. No caller may construct that view directly.

Adapters authenticate owner assertions, verify bytes and reuse existing market
quality reference resolution before domain assessment. Public frozen input structs
are not proof of authenticity. A hash or asserted entitlement cannot grant rights.

## Alternatives considered

Importing wire DTOs would mix transport and domain invariants. Reimplementing market
reference resolution would create a second quality authority. A simulator dependency
would couple research eligibility to execution behavior. No new external dependency
or framework is required.

## Consequences

A missing required candidate abstains for the whole request; no partial ranking is
produced. Within bounded inputs, reason precedence is entitlement, unsupported
universe, invalid evidence, future evidence, stale evidence and missing history.
Oversized inputs fail before assessment. This is minimum evidence, not strategy
eligibility or approval. Later application composition must preserve this boundary.

## Compatibility and migration

No wire, database, event or runtime contract changes. Existing M04 v1 reason strings
are returned by explicit code access, without importing serialized DTOs. Raw USD
is the only qualified basis. Existing owner quality remains authoritative; future
basis support requires separate qualification. No data migration is needed.

## Security and operations

No clock, broker, provider, model, credential or storage operation exists. Adapters
remain responsible for audience isolation, current reauthorization and private
input limits. The policy consumes explicit historical claims rather than fetching
current state. Rollback removes an unused additive library before dependent slices
adopt it; no retained data or operational state changes.

## Validation

The default research-evidence gate protects five deterministic tests covering
complete selection, temporal/rights boundaries, revisions/basis/duplicates,
permutations and bounded malformed selection. Run both architecture sensors and
all default verification before delivery.
