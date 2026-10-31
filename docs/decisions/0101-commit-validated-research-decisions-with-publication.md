# ADR-0101: Commit validated Research decisions with publication

- Status: Accepted
- Date: 2026-10-31
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Require a complete canonical execution configuration and exact new owner approval.
- Seal independent reconciliation and retain configuration, decision, completion and both outbox events atomically.
- Recover historical receipts without inventing provenance or reauthorizing new execution.

## Context

PR2 declarations lack PR9's explicit allowed regimes and independent context age. Earlier
worker/preparation paths intentionally remain staged. PR12 must close publication bypasses
without rewriting old artifacts or falsely marking legacy results validated. The user
approved complete configuration and new approval rather than implicit legacy defaults.

## Decision

Add `research.execution.v1` around the unchanged strict declaration bundle, requiring an
explicit event revision, context age ceiling and sorted allowed-regime set. Its canonical
content digest becomes the request/profile configuration identity. Retain exact configuration
bytes. Resolve only compiled momentum, independent controls, event/regime/quality and numeric
revisions; unsupported declarations reject. `selected-independent-controls-v1` names the
existing complete control composition. `owner-lifecycle-v1` names current explicit owner
transitions; PR14 adds separately configured automatic invalidation. Benchmark remains a
retained declaration, not an invented computed benchmark metric.

The application constructs the complete policy from the selected bytes, requires matching
owner evidence age and exact TTL capped by evidence rights, computes a proposal, then
independently reconciles all terminal bytes. A private-constructor ValidatedExecution seals
that configuration to the actual computation. Owner negative facts can produce only explicit
not-evaluated abstentions; unavailable reads retry. Direct legacy evaluation drains to
MissingEntitlement under the same complete configuration and approval. Legacy preparations
cannot authorize a fresh transaction.

Before owner I/O, governed recovery requires the exact approved profile. At commit, check it
again while holding a shared scope advisory lock and profile share lock; administration takes
the corresponding exclusive locks. Scope-wide advisory fencing prevents a queued retirement
from being overtaken when the immutable profile row itself remains unchanged. No external
owner call runs inside the transaction. Approval is revision 2 in the existing closed history;
retirement remains terminal revision 3. Later control fences belong to PR13.

Commit immutable configuration/decision, completion, inbox and terminal/provenance outbox
events in one transaction. A deferred database constraint requires a companion for every new
completion, including inserts by old binaries. Exact duplicates/concurrent winners recover
original bytes. Recovery rechecks configuration/provenance references, retained approval and
both outbox proofs, but does not require approval still to be current. Pre-upgrade rows retain
explicit absence. Checksums/digests are integrity identifiers, never authentication.

## Alternatives considered

Silently defaulting missing regime/age settings changes evidence meaning. Keeping optional
legacy commit paths permits bypass. Separate post-commit provenance writes lose atomicity.
Editing Research v1 breaks strict readers and immutable identities. The existing SQLx,
transactional outbox/inbox, canonical Serde and deterministic kernels need no new dependency.

## Consequences

New publication requires the complete selected controls, independent validation and current
exact approval. The companion event has its own content identity and terminal causation.
The projector tolerates either delivery order: missing bases remain gaps without successful
progress; backfill then retry resolves them. Queries revalidate both retained inbox proofs.
Historical provenance is separate from current eligibility, entitlement and account risk.

## Compatibility and migration

Apply Research migration 0004 after 0001–0003 and existing inbox/outbox migrations. Existing
rows receive requires_decision=false without changing outcome bytes; subsequent inserts must
have the immutable companion at commit. Provision the additional decision subject and upgrade
projector/Gateway readers before starting new publication. Review, install and approve the
new configuration identity explicitly; old approvals do not transfer. Do not auto-migrate or
auto-approve at worker startup. Internal decision GET is additive; Gateway metadata understands
its digest and additional local revision. Existing outcome endpoints retain exact bytes.

Rollback stops intake/relay/serving and preserves all authoritative rows/outbox proofs and
approval history for a compatible reader. Old binaries cannot create new completions against
the new schema. Do not drop constraints, erase companions or rewrite approval/configuration
identities to restore availability. Rebuild disposable views only from retained owner facts.

## Security and operations

The offline operator controls canonical configuration and installed owner binaries. Runtime
roles need companion SELECT/INSERT plus existing completion/inbox/outbox grants, without DDL,
TRUNCATE or trigger-disable privileges. Scope/profile approval cannot override account risk.
No model, provider activation, live broker path or peer-owner-table read is introduced.

## Validation

Default proofs cover strict selection, unsupported revisions, sealed preparation, historical
wire compatibility and query revisions. Real PostgreSQL proofs cover companion statement/
commit rollback, immutability, missing proof, legacy absence, old insert refusal, concurrency,
cancellation and retirement fencing. Owner-process, broker/projector and four-service proofs
exercise actual configured publication, receipt restart, gap recovery and lifecycle/rebuild.
Generate checked SQLx metadata and report unavailable canonical qualification prerequisites.
