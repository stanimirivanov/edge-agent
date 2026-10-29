# Deterministic research rendering

## TL;DR

- M04-PR5 evaluates one bound request into a canonical artifact or abstention.
- Rank every evidence-qualified score descending; break ties by stable instrument ID.
- Keep zero and negative scores; add no strategy threshold or partial-universe filter.
- Run `cargo xtask verify --check research-rendering --python python3`.

## Application boundary

[`research-application`](../../crates/research-application/src/lib.rs) binds the
versioned request to an existing `EvidencePolicy` before evaluating owner assertions.
The application profile uses 32-character lowercase hexadecimal instrument IDs,
matching domain identity order. The generic v1 contract still permits other tokens;
this evaluator does not accept aliases or infer mappings. Decision cutoff,
implementation/configuration digests, lookback and complete universe must match.
Malformed requests and binding mismatches are errors, not evidence abstentions.

Authenticate caller/owner/audience claims, verify retained artifact bytes, and load
historical mappings/calendar/configuration through the existing owner boundaries
before calling this function. Hash equality does not certify those inputs. An
explicit immutable expiry is part of the admitted evaluation configuration and
must be retained for replay, not regenerated from wall time. The future configuration
loader must verify the selected configuration bytes, including expiry, freshness,
formula and renderer revisions. This pure slice accepts those selected assertions;
it does not implement that loader or establish permission to use licensed evidence.

## Ranking and evidence failures

The domain [ranking module](../../crates/research-domain/src/ranking.rs) consumes
sealed eligible evidence and [versioned momentum](research-momentum.md). It sorts
rounded integer ppm descending, then instrument identity ascending. Raw returns
that round to the same ppm use the identity tie-break. A calculation failure returns
an error and no partial ranking. No threshold, strategy registry or approval is added.

The existing [evidence policy](research-evidence.md) abstains for the entire request
when any required candidate fails. Empty evidence produces `insufficient_history`;
unauthorized, future, invalid or otherwise unusable required candidates produce
the existing stable reason. No subset is silently published. The reserved
`no_eligible_candidate` wire reason is not emitted: this slice has no additional
score-based exclusion policy. Scenario/risk illustrations stay `not_qualified`.

## Canonical outputs

Artifacts preserve the exact admitted request, pinned revisions, ranking and sorted
instrument/evidence-digest census. Each reference uses the maximum selected close
availability, which includes owner knowledge under the evidence gate. Expiry must
be after decision and no later than the earliest entitlement expiry. Later use
must still reauthorize current rights and freshness; expiry alone grants neither.

Renderer revision `research-ranking-raw-v1` uses a fixed ASCII template naming the
formula, lookback, units and basis. It reads no model, locale, environment or clock.
The [v1 contract](research-contracts.md) supplies sorted-key canonical JSON with one
final LF and SHA-256 content identity. `RenderedOutcome` retains immutable record,
bytes and digest. No object store, database, publication event or runtime is activated.
Identical admitted request, selected policy, evidence and expiry yield identical bytes.

## Verification

The default protected `research-rendering` gate enables the existing serde_json
map-order feature and freezes artifact/abstention examples in
[`fixtures`](../../crates/research-application/tests/fixtures/). Six tests cover
golden bytes, all 24 candidate permutations/session order, request binding,
whole-request failure, expiry/overflow and lineage-sensitive canonical identity.
No test mutates process locale, requires a service, or claims source authentication.
