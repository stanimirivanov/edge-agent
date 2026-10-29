# ADR-0085: Project immutable Research outcomes

- Status: Accepted
- Date: 2026-10-29
- Milestone: M04 - Deterministic research
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Keep pure Research request/outcome views independent of account audit state.
- Preserve one exact canonical artifact or abstention; duplicates do not advance state.
- Surface lifecycle records as gaps until their separate transition implementation.

## Context

Research publishes immutable artifact/abstention records, but has no per-request
terminal revision chain. The future projector needs a bounded rebuildable reducer
without inventing owner sequencing, current evidence authority or query certainty.

## Decision

Initialize a ResearchProjection from an independently authenticated request anchor.
Expose pending/artifact/abstained observation and exact terminal record bytes. Reuse
Research v1 validation and the audit domain's replay result/error vocabulary. The
first matching terminal fact advances local projection revision zero to one;
identical replay is harmless and different terminal content conflicts. A changed
request under the same scoped key conflicts; foreign scope/request rejects.

Local revision is not a new owner protocol. Lifecycle records arriving before the
artifact, or requiring the later lifecycle handler, return Gap without mutation.
Known wrong artifact references or lifecycle facts against an abstention conflict.
Callers must retain unresolved input and never advance consumption progress on
errors/gaps. This slice implements no lifecycle transition or financial mutation.

## Alternatives considered

Account audit revisions cannot sequence unrelated Research requests. Inventing a
terminal owner revision/predecessor field would redesign already-published contracts.
Silently skipping unsupported lifecycle facts would hide projection lag. Buffering
gaps or retaining unlimited deliveries is unnecessary for one immutable outcome.

## Consequences

Pending means no terminal fact observed, not authoritative rejection. Artifact means
observed immutable data, not current validity, expiry or read entitlement. A wrapper
must combine the view with unresolved consumption/gap status before claiming currency.
Checksummed versioned checkpoints retain the request and exact terminal bytes;
restore revalidates all contracts and binds the separately trusted request anchor.
Checksums detect corruption, not malicious writers or authorized rollback.

## Compatibility and migration

Add a separate pure audit-domain module and private EARP1 checkpoint format; account
projection APIs/checkpoints and Research v1 records do not change. No deployment or
data migration occurs. Future persistence must atomically store checkpoint and
consumer progress and authenticate/fence storage separately. Rollback discards only
disposable new views and rebuilds from trusted owner data.

## Security and operations

The reducer has no I/O, clock, broker, database, HTTP or authentication capability.
The caller authenticates request admission and Research ownership/route. Bounds are
checked before parsing/restoring checkpoints, and errors expose no payload details.
No model/provider/market evidence read or live execution path is introduced.

## Validation

Protect focused replay discovery under serde_json map-order feature unification.
Test pending/terminal checkpoint rebuilds, duplicate/conflicting records, foreign
scope/intent, reordered lifecycle gaps, immutable bytes and corruption/version/
anchor/outcome refusal. Run existing audit tests and the full repository gate.
