# ADR-0114: Render reconciled policy measurements

- Status: Accepted
- Date: 2026-11-12
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Reconcile both artifact and decision through the existing M05 execution path.
- Reuse deterministic formulas and bind measurements to their exact sources.
- Keep legacy renderers, preparation, companion replay and evaluation pins unchanged.

## Context

Retained reason codes do not encode observed measurements or configured limits.
Explaining why a limit applied requires the exact configuration and complete
owner-authorized evidence, including candidates excluded from ranking.

## Decision

Add the sealed `research-explanation-policy.v1` preview. Its constructor
resolves the exact execution configuration, verifies provenance, and independently
rebuilds the artifact and complete decision before accepting either. Callers
must first authenticate the owner assertions and retained identities.

Reuse M05 liquidity, volatility/gap and context functions over the same selected
sessions. Render units, revisions, inclusive bounds, regimes, closed event windows,
complete coverage and event instants, age ceilings and per-session chronology.
Unknown or incomplete critical coverage refuses. Cite the daily evidence on
each measurement and the separate event source on calendar claims. Comparisons
explain constraints independently; aggregate reasons do not imply every reason
applies to every source or ranked instrument.

## Alternatives considered

Inferring measurements from reason codes or trusting model-supplied values would
invent facts. Reimplementing numeric formulas would risk changing M05 semantics.

## Consequences

One source can visibly support several claims; event facts identify both daily
and calendar provenance. The preview replaces v2's unavailable chronology rows
with qualified owner chronology and retains its validated choices/disclosures.
The immutable v2 template is reused; the frozen evaluator is not changed here.

## Compatibility and migration

This additive pure API is consumed by later evaluator and guarded-runtime slices.
It changes no CLI, profile selector, canonical input, persisted record or serving
route. No migration is needed. Future orchestration must authenticate owner
inputs, check current rights and qualify its exact preparation/renderer revision.

## Security and operations

Reconciliation proves consistency, not owner authenticity or current permission.
The API performs no I/O, model dispatch, publication or policy mutation. Model
choices cannot populate the sealed measurement text or omit fixed disclosures.

## Validation

Test inclusive equality and breaches, both event endpoints, complete empty
coverage, unknown coverage refusal, source reuse and independent calendar
attribution, configuration/evidence tampering and deterministic typed replay.
Run focused tests under map-order unification and the default repository gate.
