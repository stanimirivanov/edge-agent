# Independent Research eligibility policy

## TL;DR

- Explicit versioned policy enforces instruments, long-only direction and holding horizon.
- Governed evaluation validates complete required evidence before narrowing ranking.
- This pure staged API does not certify the remaining M05 controls or activate workers.

## Inputs and decisions

The [domain policy](../../crates/research-domain/src/policy.rs) retains immutable policy
revision, configuration digest, a sorted unique instrument allowlist and an inclusive
1..=252 session horizon ceiling. Zero revision sentinels and malformed selections reject.
Intent supplies direction and holding horizon explicitly; neither is inferred from
momentum sign, lookback or artifact TTL. Valid short intent is denied, while malformed
zero/over-252 horizons are input errors. No production defaults are introduced.

Assessment accepts at most 64 distinct candidates, tolerates input permutations, and
returns sorted eligible identities. Reasons have stable precedence:
`instrument_not_allowed`, `direction_not_allowed`, `horizon_exceeded`. All applicable
reasons are retained. Unsupported instruments narrow a nonempty selection; direction
or horizon denial removes all eligibility. An empty result abstains. `Approved` means
only these eligibility checks passed; it is not strategy approval, publication authority
or an account-risk decision.

## Governed evaluation

The [application API](../../crates/research-application/src/governed.rs) binds the policy
to the request's exact configuration digest. It validates intent and computes the
normal result over the complete required universe before applying narrowing. A bad
excluded candidate still causes the original evidence abstention; no eligibility
decision is exposed in that case. Numeric failure likewise never becomes partial success.

For allowed input, original canonical bytes remain exact. Narrowing removes only
ranking entries, preserving their scores/order, original request, full evidence census,
expiry and explanation. If none may proceed, use the existing `no_eligible_candidate`
wire abstention while retaining detailed reasons in the in-memory decision. Existing
Research v1 payloads gain no fields. Evidence failures retain their original wire reasons.
Malformed input and binding mismatch remain errors, not accepted-request denials.

## Staging, verification and rollback

Workers continue using their existing paths during staged rollout. This API is directly
usable for pure governed evaluation; remaining market constraints, independent artifact
validation, audited provenance and mandatory publication enforcement arrive in later
M05 slices. No full PR2 loader, approval token, account risk, public schema, database or
provider behavior is introduced. See [ADR-0095](../decisions/0095-enforce-independent-research-eligibility.md).

Run `cargo test --locked -p edgeagent-research-domain -p edgeagent-research-application`
and `make verify PYTHON=python3`. Tests cover exact boundaries, malformed/duplicate/oversize
input, stable reasons under permutations, identical allowed bytes, preserved narrowed
scores/lineage, empty eligibility, configuration mismatch and excluded evidence failure.
Rollback removes the additive pure API without stored-state migration.
