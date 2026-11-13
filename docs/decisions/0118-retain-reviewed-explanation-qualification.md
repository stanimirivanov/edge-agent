# ADR-0118: Retain reviewed explanation qualification

- Status: Accepted
- Date: 2026-11-13
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Independently rescore complete evidence before exact-profile owner approval.
- Require an audited owner attestation of real runtime and reviewer provenance.
- Keep approval separate from current disclosure rights and model activation.

## Context

The frozen evaluator is available, but a caller's `passed` Boolean cannot be
runtime authority. Historical v1 collection is incomplete. Synthetic process
proofs qualify orchestration, not a production model. The approved PR31 decision
requires rescoring plus explicit schema-owner attestation rather than local
runtime-artifact verification during approval.

## Decision

Define a bounded canonical `research.explanation-qualification.review.v1` command
with approve/revoke actions, caller scope, positive consecutive revision and a
nonzero digest of the private owner attestation record. Approval includes the
full immutable profile, complete suite identity, preparation census, exact private
manifest and rebuilt helper digests, owned-local-runtime provenance, all thirty
case observations and both presentation-bound independent reviewer records.

The preparation identity hashes the ordered complete case/prepared-input census
and four pinned adversarial contexts. Renderer identity hashes the complete
presentation revision; validator identity pins the existing allowlist protocol.
The suite and helper pins additionally bind actual reviewed validation semantics.
Context inputs wrap U08's complete preparation with each exact untrusted context.
All four require independently valid generated proposals; outage/fallback provides
no evidence of model resistance. Require the complete evaluator's unchanged two
21-of-30 preference thresholds and all 47 deterministic safety checks. Invalid
proposals refuse before persistence; raw rejected output is never retained.

Compute collection and assessment digests locally from the exact accepted evidence,
ratings, preferences and safety census, using recursively ordered JSON plus LF.
Never accept submitted scores or a passed flag. Canonical typed decoding rejects
unknown/duplicate fields, alternate encodings, unsupported/synthetic provenance,
legacy reports and commands exceeding 2 MiB. The sealed application review is the
only input accepted by approval persistence; current reads also rescore retained
bytes and reconcile stored digests, scope, profile, revision and action.

The schema owner explicitly attests that the private manifest/helper and evidence
belong to the actual owned runtime and exact profile, that imported records match
the retained collection/assessment, and that two named reviewers were authenticated
and independent. These are externally reviewed facts, not properties proven by
JSON, hashes or synthetic tests. A dishonest owner can attest falsely; this actor
is the existing trusted administration root. The attestation digest binds the
private review record to the persisted command without retaining private files.

Use a separate append-only ledger keyed by caller scope and exact profile identity.
Record authenticated actor and database time. Exact latest retries by that actor
are idempotent; stale/conflicting revisions refuse. Exclusive review locks and
shared current-read locks serialize approval and revocation. Select the latest
revision before action; absence or revocation denies without falling back to an
older approval. Approval has no implicit expiry and remains subject to explicit
revocation; any profile, suite, protocol or evidence change needs matching review.

## Alternatives considered

Trusting a report's declared disposition permits forgery and stale protocol reuse.
Verifying runtime artifacts at approval time would require operator artifacts
that are deliberately deferred to the real qualification work. Reusing strategy
promotion or disclosure grants conflates independent purposes.

## Consequences

Finite `offline-explanation-qualification` administration is scope checked and
uses the existing private database identity. No default approval or test bypass
is installed. Tests use invented packages/attestations in isolated schemas and
never qualify an actual runtime. The current driver still needs a later explicit
producer for this complete protocol; legacy incomplete collections cannot upgrade.

## Compatibility and migration

Apply additive Research migration 0012 after 0011 before administration/reads.
Preserve all old strategy approvals, companion receipts and qualification files.
Retain audit rows on rollback; older binaries ignore the table. No generation or
qualification-driver format changes occur here. PR32 must adopt these protocol
pins and transaction checks, and PR33 must supply real reviewed evidence.

## Security and operations

Approval is a current database observation, not a bearer lease. PR32 must recheck
it at its dispatch/commit fences, separately attest runtime identity and actual
preparation, and obtain current Research and evidence-owner disclosure grants.
Approval grants no training, serving, document rights or production activation.
Store no model weights, private manifests, reviewer documents or raw rejected
responses in the repository/logs. Only bounded accepted proposals and opaque
artifact/review pins enter the owner ledger.

## Validation

Use pure complete-scoring/canonical/binding refusal tests, alternate JSON map
ordering, and real PostgreSQL role, audit, retry, contention, rollback, corruption,
restart and revocation tests. Exercise finite owner administration in a real
process. Generate/check SQLx metadata and run repository/architecture/default gates.
