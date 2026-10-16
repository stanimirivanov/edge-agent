# ADR-0022: Bound relay publication by acquired claims

- Status: Accepted
- Date: 2026-10-16
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Count committed outbox claims against the relay's publication-attempt budget.
The final allowed claim may publish; a recovered claim beyond the current
budget must quarantine without invoking the publisher. Cancellation never
refunds a claim or proves that an earlier publication did not persist.

## Context

The relay previously checked exhaustion only after a publisher returned a
failure. A worker could repeatedly stop after claiming or publishing, reclaim
after lease expiry, and publish beyond its configured budget. Storage fencing
prevents stale completion but cannot retract external I/O or count physical
requests exactly across crashes. A newly reduced policy can also encounter a
record whose acquired-claim count already exceeds its budget.

## Decision

After complete stored-envelope and registry validation, compare the acquired
claim's one-based attempt count with the current policy's inclusive maximum.
Allow publication when `attempt <= max_attempts`, including the final allowed
claim. When `attempt > max_attempts`, do not invoke the publisher. Request one
quarantine transition with the original claim, owner, and generation, using
`publication_budget_exhausted`. Report that result only after persistence
confirms it. Storage failure or cancellation must not start an alternate
transition. Validation failure retains its existing identity-free contract
quarantine policy, even for over-budget claims.

`RelayMessageFailure::PublicationBudgetExhausted` is a policy failure without
an underlying publisher error. It does not claim that earlier publication
failed. No publication telemetry event is emitted for a skipped publication;
the actual quarantine persistence result remains observable.

Claims consume budget even if a worker never invokes the publisher or loses
its result. This bounds admitted relay publication calls for a record between
authorized replays, not physical network transmissions within an adapter,
parallel workers' completion order, or an already admitted stale publication.
Policy changes do not reset persisted history. Same-owner recovery still
requires a new generation under [ADR-0009](0009-fence-outbox-transitions-by-claim-generation.md).

## Alternatives considered

### Check only returned publication failures

- Benefits: maximizes automatic recovery after worker cancellation.
- Costs and risks: repeated cancellation bypasses the configured budget.
- Reason not selected: the admission limit must survive worker replacement.

### Count only actual network requests or refund abandoned claims

- Benefits: avoids consuming budget before network work starts.
- Costs and risks: a crash can occur between a request, confirmation, and any
  local counter update. A refund can authorize additional unknown effects.
- Reason not selected: requires a different durable protocol and cannot infer
  an absent effect from an absent result.

### Permit a final recovery publication beyond the budget

- Benefits: may automatically reconcile an earlier confirmed broker effect.
- Costs and risks: another lost result creates another exception and no bound.
- Reason not selected: operator-authorized reconciliation/replay must own this
  explicit availability-versus-resource tradeoff.

## Consequences

### Positive

- Recovery and reduced policies cannot bypass admission through cancellation.
- Outcomes distinguish a skipped publication from a returned transport failure.
- Existing immutable-content, validation, quarantine, and fencing rules remain.

### Negative

- An exhausted record can quarantine despite an earlier broker persistence.
- Abandoned claims can exhaust the budget without any network request.
- Quarantine is not evidence of non-delivery; consumers still need durable
  idempotency, and operators must inspect retained state before replay.

### Neutral or follow-up

Typed configuration and runtime-error taxonomy remain a separate refactor.
The composition root must still bound publication time below lease duration;
this decision adds neither timeouts nor lease renewal.

## Compatibility and migration

Add variants to public Rust failure/reason enums and migrate all exhaustive
workspace matches together. No event format, SQL query, schema, retry timing,
or existing reason meaning changes. The bounded reason code is additive.
Readers must tolerate it as retained operator evidence. Roll out consistent
worker policy; mixed older workers can still admit over-budget publication.
Rollback restores that defect, so prefer a forward fix. Authorized replay
already preserves identity/bytes and resets attempts with audited evidence.

## Security and operations

Use fixed diagnostics only; no payload or database text enters reason codes.
Retain full validation before constructing diagnostic identity. Monitor
`publication_budget_exhausted` separately from publisher-returned exhaustion.
Reconcile uncertain effects or use the existing authorized replay boundary;
manual counter resets bypass evidence and are not a recovery procedure.

## Validation

Portable tests cover inclusive boundaries, policy reduction, maximum history,
publisher invocation counts, current-claim forwarding, cancellation before and
after quarantine commit, and returned storage errors. The protected telemetry
proof asserts no publication event for skipped I/O and retains identity-free
validation failures. A real PostgreSQL relay proof cancels pending publication,
expires its synthetic lease, and checks same-owner generation recovery and
stale completion rejection. The existing event-spine executable uses the real
JetStream publisher with a held confirmed receipt: recovery within budget
returns `Duplicate` with one broker message; exhausted recovery performs no
additional publication and retains quarantine. This proves application result
loss after confirmed persistence, not wire ACK loss or consumer exactly-once
execution. T2 discovery and T3 execution protect these named proofs.
