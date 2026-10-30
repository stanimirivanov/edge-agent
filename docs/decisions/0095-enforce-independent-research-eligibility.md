# ADR-0095: Enforce independent Research eligibility

- Status: Accepted
- Date: 2026-10-30
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Evaluate instrument, direction and horizon constraints outside strategy output.
- Preserve required-evidence checks and narrow only deterministic ranking entries.
- Retain existing wire compatibility and staged runtime activation.

## Context

M05 requires independent deterministic controls. Research v1 carries no direction or
holding horizon, and existing evidence rules require the entire declared universe.

## Decision

Introduce immutable domain policy/version/configuration bindings and explicit typed
intent for a pure governed use case. Validate bounds and reject malformed input. Allow
long-only intent, inclusive declared holding limits and explicit instrument selection.
Accumulate stable reasons in instrument/direction/horizon order independent of input.

Compute normal required-evidence evaluation before narrowing. Preserve allowed bytes;
narrow by removing ranking entries without changing scores, request or evidence census.
Empty eligibility uses existing no_eligible_candidate wire abstention. Evidence failures
retain their original reason and expose no apparent policy approval decision.

## Alternatives considered

Filtering evidence first would conceal invalid required inputs. Inferring holding
horizon from lookback or TTL would change units/meaning. Adding fields to strict v1
payloads would break compatibility. Allowing strategy output to choose policy weakens
independent controls.

## Consequences

These checks are directly usable and independently versioned. Their Approved status
means eligibility only. Remaining market constraints and validators must run before
mandatory governed publication in later M05 work. No account-risk control is replaced.

## Compatibility and migration

Existing Research payloads, workers and historical results remain unchanged. This is
an additive pure API with explicit narrowing/denial behavior. No stored-state migration
is required; rollback removes the API before later consumers adopt it.

## Security and operations

Callers must authenticate selected policy/configuration and explicit intent. A matching
digest alone grants no authority. No model, provider, transport or persistence enters
domain logic. Errors disclose no caller payload. No new dependency or service is added.

## Validation

Domain and application tests cover bounds, all denials, stable precedence, permutation
independence, exact byte parity, retained lineage and excluded evidence failure. Register
the application proof in protected rendering discovery and run the complete default gate.
