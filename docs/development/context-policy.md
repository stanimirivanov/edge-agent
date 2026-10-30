# Independent event, regime and quality policy

## TL;DR

- Classify each instrument by exact selected endpoint prices, independently of strategy output.
- Require complete coverage of a closed event-exclusion window; unknown is never clear.
- Apply explicit market/context age ceilings in addition to existing sealed owner checks.

## Regimes and event windows

`raw-endpoint-direction-v1` compares the final and initial raw USD closes in the shared
lookback+1 census: falling, exactly flat, or rising. This is a per-instrument historical
price-direction label, not a market-wide regime, forecast or learned classifier. Tiny
positive changes remain rising even when momentum rounds their return to zero ppm.
The policy supplies a sorted, unique, nonempty allowed set; there is no strategy-supplied
regime label or default allowed family.

`complete-closed-event-window-v1` checks `[decision-before, decision+after]`, including
both endpoints. A zero-width window still blocks an event at the decision instant.
Coverage assertions are half-open, so their end must be strictly after the exclusion
window's end. The entire window must be covered by an affirmative owner assertion.
Unknown, partial or unrepresentable windows abstain; they cannot become empty calendars.
Events must be sorted, unique and within declared coverage, with at most 256 instants.
Source identity, knowledge time and coverage are bound to the same retained daily digest
and instrument as the eligible closes. Coverage cannot exceed retained use authority.

## Independent quality and failure behavior

`owner-qualified-market-context-age-v1` retains all existing owner entitlement, raw-basis,
quality, reference, chronology and complete-evidence checks via sealed eligible evidence.
It adds an explicit positive age ceiling for both the latest selected market event and
the event-calendar knowledge assertion. Equality passes; future or over-age critical
facts abstain. Changing a receipt/read time cannot refresh either fact's own timestamp.
No client quality Boolean or caller-supplied regime is accepted by the new adapter.

All required candidates are checked, including those otherwise excluded. Within context
assessment, failure priority is missing entitlement, invalid evidence, future evidence,
stale evidence, then unavailable coverage. It is independent of supplied context order.
Existing earlier evidence/constraint gates retain their order. Critical failures produce
the corresponding existing Research abstention and no apparent eligibility decision.
Valid event conflicts and disallowed regimes instead narrow candidates or produce the
existing `no_eligible_candidate` abstention. Stable reasons append `event_excluded` then
`regime_not_allowed`; original scores, order, request and complete evidence census remain.

## Application and trust boundaries

Use the authenticated [owner companion](policy-evidence.md), then `policy_context` to
convert coverage while preserving its source digest/knowledge/interval/event census.
Conversion and hashes alone grant no authority. Current affirmative coverage is explicitly
synthetic; no real event feed or completeness claim is added. The independent domain
classifier consumes qualified prices directly, without model or strategy assertions.

Select exact revisions and explicit `Rules` through `Policy::with_context`. The additive
`ContextInputs` groups these inputs with existing `ConstraintInputs`; older public input
structs remain unchanged. `evaluate_with_context_inputs` preserves selected liquidity and
volatility/gap gates. Older entry points cannot bypass selected context by omitting it.
Full bundle loading, persistent decision provenance and mandatory worker publication
remain later M05 slices. See [ADR-0098](../decisions/0098-enforce-independent-research-context.md).

## Verification and rollback

Run `cargo test --locked -p edgeagent-research-domain -p edgeagent-research-application`,
`cargo test --locked -p edgeagent-research --test policy_evidence`, and the full default gate.
Tests cover exact-price classification, inclusive/zero windows, unknown/partial coverage,
future/stale/invalid/revoked assertions, independent market freshness, stable precedence,
old-entry-point refusal, narrowing and preserved lineage. Adapter tests preserve unknown
and explicit complete calendars. No dependency, SQL, schema, provider or deployment
change is needed. Rollback removes the additive selected gate before later consumers
adopt it; existing artifacts and workers require no migration.
