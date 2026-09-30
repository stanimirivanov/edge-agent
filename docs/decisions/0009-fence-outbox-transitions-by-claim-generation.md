# ADR-0009: Fence outbox transitions by claim generation

- Status: Accepted
- Date: 2026-09-30
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Each committed outbox claim advances a monotonic, per-record lease generation.
- The portable relay port returns an opaque `LeaseGeneration` with the claimed
  message and requires it for publish, retry, and quarantine completions.
- Storage commits an outcome only when message identity, worker token,
  generation, and unexpired lease still match. A stale worker cannot complete a
  later claim even if that claim reuses the same worker token.
- Audited replay resets the attempt budget but never resets the generation.
- Older owner-only relay binaries must be drained before the fenced version is
  activated; a mixed-version rollout does not provide this guarantee.

## Context

The outbox relay publishes outside a database transaction and returns afterward
to persist the result. An expiring lease lets another iteration recover work
when the first worker stops or stalls. The original completion predicate
compared message identity, lease owner, and expiry. Worker tokens are reusable:
after an expired claim is reclaimed under the same token, an old worker can
still satisfy that predicate and mutate the newer claim. This is an ABA problem
in the storage contract, not a broker-confirmation problem.

The attempt count cannot identify a claim. It describes retry budget and
authorized replay resets it to zero. Holding a database lock across publication
would move external I/O into a transaction and impair recovery. A separate
claim-scoped fence preserves the short-transaction boundary and can be
implemented by other storage adapters that offer an atomic conditional write.

## Decision

The outbox record stores `lease_generation BIGINT NOT NULL DEFAULT 0`. Every
successful claim atomically increments it once while acquiring the row. The
committed claim returns a positive, opaque `LeaseGeneration` in
`ClaimedMessage`. Generation zero is an unclaimed or migrated-row baseline and
does not authorize an outcome. Overflow fails the claim; it must never wrap or
reuse an earlier generation.

`OutboxRelayStore::mark_published`, `release_for_retry`, and `quarantine`
require the generation from the original claim. Every adapter must atomically
compare message identity, worker token, generation, unexpired lease, and
eligible record state before committing the transition. Zero matching rows
means lease loss, including when the owner token is unchanged but a later claim
has advanced the generation. A successful outcome clears the active lease but
retains its last generation for future claims and audit diagnosis.

`replay_quarantined` remains a separately authorized transition. It clears
quarantine and resets the attempt budget without changing `lease_generation`.
The first post-replay claim advances generation again. Application policy treats
the fence as an opaque claim identity, not as a retry count, timestamp, worker
credential, or sortable business event.

Fencing prevents stale workers from recording an outbox outcome. It cannot
cancel a publish already sent to the broker, and it does not create exactly-once
transport delivery. Stable CloudEvents identity, immutable bytes, broker
deduplication, and consumer inboxes remain necessary.

## Alternatives considered

### Compare only owner and expiry

- Benefits: Fewer parameters and no schema migration.
- Costs and risks: A stale claim can complete a later claim when the owner
  token is reused.
- Reason not selected: Worker identity does not uniquely identify a claim.

### Use attempt count as the fence

- Benefits: Reuses an existing incrementing field.
- Costs and risks: Authorized replay resets it, so historical values can recur.
- Reason not selected: Retry policy and claim identity have different meanings.

### Hold the row lock during broker publication

- Benefits: No completion token is needed while the transaction remains open.
- Costs and risks: Network latency and failure hold database resources and
  couple publication to a long transaction.
- Reason not selected: The relay must commit its claim before external I/O.

### Assign a random claim identifier

- Benefits: Distinguishes claims without a per-row counter.
- Costs and risks: Requires randomness and larger persisted values; uniqueness
  becomes probabilistic unless separately coordinated.
- Reason not selected: The row lock already provides an atomic increment point
  and a deterministic non-repeating generation.

## Consequences

### Positive

- Stale publish, retry, and quarantine completions fail even after same-token
  reacquisition.
- The portable port states the actual storage capability required by the relay.
- The claim remains bounded to one short transaction, and publication remains
  outside it.

### Negative

- The public port, its fakes and adapters, SQL migration, tests, and rollout
  procedure must change together.
- Existing relay binaries lack the generation predicate and cannot safely run
  alongside the fenced version.
- A stale but already-sent broker publication remains possible; transport and
  consumer deduplication still carry that recovery burden.

### Neutral or follow-up

- The generation is not exposed as business event identity or operator
  authorization evidence.
- Lease extension and a continuously running relay remain separate work.

## Compatibility and migration

The migration adds a non-null `BIGINT` column with zero default, preserving
existing rows and immutable message content. New code claims only positive
generations. The `ClaimedMessage` constructor and `OutboxRelayStore` outcome
methods change at compile time, so every in-workspace implementation and fake
must be updated in the same pull request. A storage adapter on another provider
must prove equivalent atomic conditional writes before claiming conformance.

Before activating the new relay, quiesce any old relay binary and drain its
in-flight work or let its lease expire. Apply the migration, then start only
generation-aware relays. The current workspace provides the relay as a library,
not a continuously running production worker; this ordering is a deployment
constraint for future composition roots and existing external callers. Schema
rollback is unnecessary because the additive column is inert to old readers,
but rolling back the relay implementation requires the same quiesce-and-drain
step and knowingly restores the weaker owner-only guarantee. Prefer retaining
the corrected relay while investigating other failures.

## Security and operations

A worker token and generation are concurrency controls, not authentication or
authorization credentials. Service identity and database privileges still
restrict who may claim and mutate outbox rows. Do not put message payloads or
raw database errors in outcome logs; a bounded lease-loss category suffices.
Count lease loss separately from publication failure so operators can diagnose
slow publication, expired leases, same-token reclamation, and skewed lease
duration. A persistent rise requires reviewing publication timeout and worker
concurrency before extending the lease bound.

## Validation

- A credential-free relay test uses a fake store to prove each publish, retry,
  and quarantine decision forwards the generation returned by `claim_one`.
- The isolated PostgreSQL conformance test expires and reclaims a record with
  the same owner token, then proves the stale generation cannot publish, retry,
  or quarantine the newer claim and that the current generation can complete.
- Replay tests prove attempt reset does not reset or reuse generation.
- The local-platform CI job runs the ignored PostgreSQL conformance test
  explicitly; default credential-free checks compile it but do not execute it.
