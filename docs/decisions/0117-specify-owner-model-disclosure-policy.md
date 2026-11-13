# ADR-0117: Specify owner model-disclosure policy

- Status: Accepted
- Date: 2026-11-13
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Require a distinct owner review for disclosure to a pinned model recipient.
- Bind a fresh owner-check snapshot to exact evidence, input and operation.
- Define contracts now; PR29 implements the ledger and PR32 integrates dispatch.

## Context

Acquisition, automated Research use and explanation serving are separate purposes.
None establishes permission to disclose evidence to an inference runtime. Existing
serving permits and synthetic qualification grants cannot become model authority.
An immutable profile identifies intended runtime, weights, prompt, settings,
validator, renderer and disabled tools; it does not attest the actual deployment.

## Decision

MarketData owns reviews of its sealed acquisition rights/feed scope. Derive that
scope as SHA-256 of recursively key-sorted JSON plus one LF: an envelope with kind
`market-data.model-disclosure-rights-scope.v1` and data containing the exact sealed
string fields `rights`, `principal`, `account`, `source`, `instrument`, `listing`,
`endpoint`, `audience`, `storage`, `semantics_id`, `semantics_revision`, `precision`
and `market_scope`. The full rights pin includes identity, revision, source,
known-at time and checksum. Never accept a caller's scope or legal interpretation.
Listing is included even though the historical serving scope omitted it.

The canonical review key hashes kind `market-data.model-disclosure-review.v1`,
owner rights scope, logical model audience, full profile identity, deployment,
input-contract digest and purpose `model_disclosure`. It intentionally excludes
individual acquisition ranges and prepared input instances. An allow review must
cover the entire pinned scope and preparation contract. Agreements requiring
unsupported narrower conditions deny until a policy revision represents them.
The initial supported recipient is an independently attested operator-owned local
runtime with tools disabled. Locality alone confers no permission.

PR29 will use schema-owner administration and append-only audited, consecutive
revisions with exact actor-bound retry idempotency. Each review records allow or
deny, opaque agreement reference and validity interval. Select the latest revision
before evaluating decision or time: missing, revoked, denied, future or expired
reviews refuse; never fall back to an older allow. Recheck current rights, object
access and retention at each operation, independent of the review interval.

The bounded request carries a fresh 128-bit operation challenge, canonical Research
intent, final artifact/decision anchor, acquisition, exact owner-publication hash,
evidence digest, audience, full immutable profile, preparation-contract hash and
actual prepared-input hash. Research alone reconciles its artifact/decision and
actual bytes against the compiled preparation path, current grants, qualified
profile and verified runtime. MarketData independently resolves its retained
correlation/publication, sealed admission, object access and current rights. No
peer-owned table access or caller assertions replace these checks.

The versioned permit echoes the entire request and records owner scope, recomputed
review key, positive review revision, agreement reference, check time, review and
retention bounds and exclusive expiry. Check time cannot precede the decision;
expiry must follow check time and cannot exceed either bound. Request and permit
limits are 8192 and 16384 bytes, checked before parsing. Strict canonical codecs
reject unknown fields, duplicate keys, unsupported versions/purposes and alternate
encodings. Review-key hashing uses the same canonical encoder plus final LF.

A permit is a non-bearer observation, not a signed grant or cacheable lease.
`bind` checks only exact request equality and time. Every disclosure requires an
authenticated fresh owner response with a new challenge immediately before dispatch,
plus durable-attempt authority and actual deployment/input checks. Runtime deadlines
and model context lifetime must respect rights and retention. Already disclosed
bytes cannot be recalled; revocation fences subsequent fresh operations.

## Alternatives considered

Reusing serving or Research-use grants conflates legal purposes. Trusting a caller
rights scope or accepting a cached permit bypasses current owner decisions. A new
signed-token infrastructure adds key lifecycle work without replacing fresh checks.
Adapt existing owner review and canonical contract patterns without new dependencies.

## Consequences

The contract is testable without services. PR29 owns scope derivation, review
persistence, administration and authenticated owner checks; PR32 owns dispatch
integration. Missing implementations grant nothing. MarketData permission covers
only its evidence. Research must obtain a fresh permit for every contributing
owner acquisition; a missing permit refuses the whole dispatch. Independently
licensed documents need their own owner authority and Research checks. No permission for training, prompt/response persistence,
cross-request reuse, tools or explanation serving follows from model disclosure.

## Compatibility and migration

Additive internal v1 contracts leave all old canonical bytes, serving permits,
Research-use grants and production behavior unchanged. No migrations, endpoints,
issuance, approvals or model activation ship here. Later consumers must explicitly
adopt the fresh-check protocol; malformed or legacy payloads cannot upgrade it.

## Security and operations

Use opaque pins rather than document content, endpoints or credentials. Contracts
omit Debug representations; validation errors contain no subject values. Transport
identity, authenticated administration, current review enforcement and context
cleanup remain required implementation gates, not guarantees of deserialization.

## Validation

Contracts tests exercise strict codecs, purpose/version separation, every dispatch
subject and recipient substitution, review-key scope and expiry/revision bounds.
Run them under both serde_json map-order modes, plus repository, architecture and
standard verification gates. No database or live-model proof is claimed by PR28.
