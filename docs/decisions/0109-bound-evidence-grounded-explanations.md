# ADR-0109: Bound evidence-grounded explanations

- Status: Accepted
- Date: 2026-11-07
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Publish explanations separately from immutable Research outcomes.
- Let the model select approved presentation variants. Deterministic code
  owns facts, coverage, rank order, policy reasons, and disclosures.
- Require current permission for model disclosure and for serving.
- Reserve one durable model attempt and recover uncertain attempts with
  deterministic fallback when serving remains authorized.
- Enable generation only after qualification against the frozen protocol.

## Context

Research already renders an explanation inside its canonical artifact. Its
independent validator reconstructs those exact bytes. Published outcomes and
retained policy decisions are also replay inputs. Changing the existing text
would change the artifact digest and break those bindings.

An explanation may expose evidence to a different audience at a later time.
Historical approval establishes the original decision and does not establish
permission for either of those disclosures.

## Decision

Produce a separate versioned companion after artifact publication. Bind it
to the exact scope, artifact and decision digests, evidence allowlist, and
explanation profile. The existing published-artifact event drives durable,
idempotent job admission. Abstentions do not become ranked artifacts through
this path.

Research constructs the allowlist from authenticated owner evidence and the
validated decision. The model proposes typed claim references and approved
presentation variants. The validator checks the full proposal against that
allowlist. It requires every ranked candidate in original order, every
required adverse reason and disclosure, and exact evidence bindings.
Deterministic rendering supplies identifiers, values, units, timestamps and
citations. A missing, duplicated, foreign, altered, or reordered required
claim makes the proposal invalid.

Bounded, explicitly supplied document excerpts may provide context under
current permission. Bind their source, digest, scope, and availability time.
Treat their contents as data. They confer no authority to run tools, fetch
URLs, establish market facts, or change decisions.

Use one optional local model adapter behind a Research-owned port. Pin the
runtime and weights identity, model settings, prompt, empty tool set,
validator, and renderer. Verify deployment identity independently of the
model response. Start with generation disabled.

Reserve the attempt durably before dispatch. Run model I/O outside the
transaction. On timeout, outage, budget exhaustion, invalid proposal, or
uncertain recovery, use deterministic fallback if serving is authorized.
An uncertain reservation is spent even if the original process died before
sending. The terminal commit fences late results and atomically retains the
companion and its publication intent. A denied result exposes no explanation.

## Alternatives considered

Embedding generated content in Research v1 would break byte reconciliation.
Unrestricted prose with citations would let a proposal omit adverse facts
or use a real citation to support a claim the evidence never established.
Retrying uncertain calls would make the external-call budget depend on
where a worker died.

## Consequences

Explanation failure is independent of artifact publication, ranking, policy,
and order eligibility. Replay checks retained bindings and repeats validation
and rendering without another model call. Separate read authorization can
make the companion unavailable while the original outcome remains intact.

## Compatibility and migration

Keep Research v1 and its deterministic explanation as supported contracts.
Add explanation storage and consumers before enabling companion publication.
Older outcomes may have no companion. Roll back by disabling new generation
and consumption while retaining committed history and unresolved reservations.

## Security and operations

Research owns explanation grants. A grant narrows current evidence-owner
rights and cannot replace them. Check model-disclosure permission immediately
before dispatch and serving permission on each read. A projector checkpoint
does not authorize serving. Unknown or unavailable serving authority denies
content. Revocation applies at the next check and cannot retract sent bytes.

Retain approved typed references, safe failure categories, and audit pins
subject to rights. Exclude raw rejected prompts and responses from retention
and telemetry.

## Validation

Use the [frozen explanation protocol](../development/explanations.md) for
qualification. The implementation plan includes strict contract, adversarial
validation, authorization, and real process recovery tests. Reviewers must
also verify byte equality for original Research outcomes.
