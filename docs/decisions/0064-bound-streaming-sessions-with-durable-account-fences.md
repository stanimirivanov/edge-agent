# ADR-0064: Bound streaming sessions with durable account fences

- Status: Accepted
- Date: 2026-10-26
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Admit finite, exactly selected streams separately from daily acquisitions.
- Require explicit session ceilings and an independent account allocation.
- Commit connection starts and generations before dialing; retain uncertain slots.
- Charge all frame arrivals before routing, including controls and filtered frames.
- Ship synthetic proofs only; activate no vendor, transport or subscription.

## Context

Daily acquisition deadlines and HTTP credit counters do not describe streaming
lifetimes, physical connections or reconnect uncertainty. A per-process semaphore
cannot enforce shared account capacity across replicas. Expiring a database lease
cannot prove an external connection has stopped consuming provider capacity.

## Decision

Seal immutable session inputs against a complete frozen instrument selection and
current rights/provider reviews covering the entire finite window. Bind concrete
authority values, witnesses, contract semantics and full configuration digest.
Require explicit start, frame, per-frame byte, aggregate byte, subscription,
connection-timeout and idle-timeout ceilings. No defaults imply provider allowances.

The independent PostgreSQL owner installs immutable account policy and exact session
inputs. Account policy includes concurrent unresolved slots, lifetime start allocation,
opening debit, positive start spacing, maximum session duration and exclusive expiry.
Worker LOGIN roles cannot write owner controls, approve themselves or confirm
quiescence. Early-bound SQL functions read genuine approvals despite temporary tables.
Ordinary counter DML remains trusted application access, not a malicious-SQL sandbox.

Serialize every mutation under the account row, then admission and session locks.
Charge starts and allocate an account-monotonic generation atomically before any
dial. A lost result, failed dial, crash, cancellation or expiry releases nothing.
Only the independent owner can confirm physical quiescence of an exact generation.
No takeover, renewal, refund or policy-reset API exists in this slice.

Charge every distinct session-local arrival ordinal, including empty controls,
duplicates and unselected frames. Preserve counters through reconnects. Repeat only
the last committed ordinal and size to recover an accounting confirmation; this is
not content deduplication or permission to process a frame twice. Charge a detected
overage and sticky-stop intake. Exact caps stop intake as well. Transport must later
bound frame allocation before receiving a complete frame, not rely on this counter.

Recheck current authority, cancellation and deadline using database time after lock
contention and before commit. Durable withdrawal locks the same account. Trusted
catalog composition must supply current authenticated references; a checksum is not
authentication. Configuration pins do not authorize credentials or data use.

## Alternatives considered

- Reuse daily acquisitions: reject indefinite renewal and mismatched quota meaning.
- Local-only limiter: reject replica oversubscription and lost crash state.
- Expiring connection leases: reject unknown physical provider ownership.
- Refund on disconnect/error: reject uncertainty laundering and retry amplification.
- Add WSS/vendor code now: defer to PR18/PR19 and explicit activation review.

## Consequences

Availability yields to account safety. Operators must reconcile uncertain physical
connections before freeing capacity; spent starts remain charged. A policy that
expires or exhausts requires a separately reviewed lifecycle design, not a fresh
namespace or counter reset. Session accounting does not establish evidence quality,
gap-free continuity, storage acceptance, market freshness or licensed-data readiness.

## Compatibility and migration

Apply additive MarketData migration 0007 after 0006. Existing daily message formats,
receipts, HTTP quotas and runtimes do not change. Provision stream-control ACLs
separately; see the streaming guide. Deploying this code enables no vendor path.
Revert application use without dropping unresolved markers or resetting counters.
Do not roll back the schema destructively while any stream state remains unresolved.

## Security and operations

Store no frames or credentials in this ledger; redacted errors/Debug avoid selected
identities. Persisted selection/authority metadata is private and needs scoped
retirement and restore fencing before live use. HTTP call credits and streaming
slots are distinct resources; an activation review must reconcile any actual shared
provider entitlement. No public plan advertisement constitutes authority.

## Validation

Protected portable tests check exact binding, finite ceilings, current revocation,
control-frame accounting, overages, invalid state and scoped generations. Isolated
PostgreSQL tests use independent authenticated workers for contention, atomic fault
rollback, confirmation loss, reconnect counters, expiry and owner-only release.
Checked SQLx metadata and migration inventories cover the additive schema. These
proofs qualify synthetic database behavior, not a live source or physical disconnect.
