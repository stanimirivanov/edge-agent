# Architecture decision records

## TL;DR

- Use an ADR for durable decisions affecting compatibility, evidence meaning, security, data, topology, ownership, or foundational technology.
- Number ADRs sequentially and never reuse a published number.
- Accepted ADRs are historical records; supersede rather than rewrite them.
- Record alternatives, consequences, migration, security, operations, and validation.
- The Rust workspace, portable event architecture, dry-run execution boundary,
  persistence-neutral inbound coordination, and executable dependency policy
  are accepted foundations; repository governance is progressively enforced.
- Outbox completion is fenced by a per-claim generation, including same-owner
  reacquisition after lease expiry.

## When an ADR is required

Create an ADR when a decision materially affects one or more of:

- public contracts or compatibility;
- persistence, durability, evidence meaning, lineage, or migration strategy;
- security, privacy, authorization, or trust boundaries;
- repository, service, or deployment topology;
- language, framework, database, queue, workflow engine, market-data provider,
  model-provider boundary, or build foundation;
- research-artifact lifecycle or cross-component behavior; or
- a constraint that future contributors might otherwise simplify away.

Local implementation choices that are cheap to reverse do not require an ADR.

## Naming and lifecycle

Use a four-digit sequence and short kebab-case name:

```text
0001-select-the-initial-runtime.md
```

Statuses are Proposed, Accepted, Rejected, Deprecated, or Superseded. Milestone
metadata uses the exact identifier and title published in the roadmap index.
Only an Accepted decision can establish that it supersedes earlier decisions.
When accepted, update each earlier record to Superseded and record both
directions in the same change. A later-superseded replacement retains its own
`Supersedes` history. Proposed, Rejected, and Deprecated records do not name
decisions they supersede. Do not edit the decision or consequences of an
accepted ADR to make history appear cleaner; add a dated note or a new ADR.

Before allocating a number, inspect this index and the decision directory from
fresh repository state. Use the lowest unused number. Resolve a concurrent
collision by renumbering the later unpublished ADR.

The repository-policy checker records each published ADR's number, canonical
filename, and canonical title in its `PUBLISHED_ADRS` identity ledger. Accepted
identity is immutable: do not reuse a number, rename its file, or rewrite its
title. When publishing the next ADR, append its identity to that ledger in the
same change. Removing, renumbering, renaming, or substituting published history
must fail verification.

The `## Index` section uses one Markdown table. Its first rendered row is the
`ADR | Status | Decision` header and every published decision is a body row in
that same table; splitting history across tables is invalid.

## Template

```markdown
# ADR-NNNN: Decision title

- Status: Proposed
- Date: YYYY-MM-DD
- Milestone: MNN - Outcome
- Deciders:
- Supersedes:
- Superseded by:

## TL;DR

Summarize the decision, primary reason, and material boundary.

## Context

What forces a decision now? Include constraints and quality attributes.

## Decision

State the decision and scope precisely.

## Alternatives considered

### Alternative

- Benefits:
- Costs and risks:
- Reason not selected:

## Consequences

### Positive

### Negative

### Neutral or follow-up

## Compatibility and migration

## Security and operations

## Validation

How will the assumptions and consequences be verified?
```

## Index

| ADR | Status | Decision |
|---|---|---|
| [ADR-0001](0001-use-a-rust-workspace-with-multiple-deployables.md) | Accepted | Use a Rust workspace monorepo with independently deployable components. |
| [ADR-0002](0002-use-portable-at-least-once-event-messaging.md) | Accepted | Use versioned CloudEvents contracts and portable at-least-once messaging. |
| [ADR-0003](0003-add-dry-run-trade-execution.md) | Accepted | Add dry-run order execution without broker connectivity or live trading. |
| [ADR-0004](0004-separate-application-ui-gitops-and-substrate-ownership.md) | Accepted | Separate application source, UI, GitOps desired state, and substrate ownership. |
| [ADR-0005](0005-standardize-message-routing-and-retention.md) | Accepted | Standardize message subjects, ownership, partitions, size, and retention. |
| [ADR-0006](0006-keep-inbound-coordination-persistence-neutral.md) | Accepted | Keep inbound orchestration behind a semantic atomic store port while adapters own transactions. |
| [ADR-0007](0007-enforce-workspace-capability-dependencies.md) | Accepted | Enforce package roles and exact direct workspace dependency policy. |
| [ADR-0008](0008-enforce-progressive-coding-harness-policy.md) | Accepted | Enforce progressive repository guidance and deterministic governance policy. |
| [ADR-0009](0009-fence-outbox-transitions-by-claim-generation.md) | Accepted | Fence outbox transitions by claim generation. |
| [ADR-0010](0010-keep-delivery-metadata-transport-neutral.md) | Accepted | Keep delivery metadata transport-neutral. |
| [ADR-0011](0011-bound-untrusted-envelopes-before-decoding.md) | Accepted | Bound untrusted envelopes before decoding. |
| [ADR-0012](0012-keep-message-contract-diagnostics-payload-safe.md) | Accepted | Keep message contract diagnostics payload-safe. |
| [ADR-0013](0013-select-durable-workflow-execution.md) | Accepted | Select Temporal for durable workflow execution. |
| [ADR-0014](0014-use-sqlx-for-postgresql-inbox.md) | Accepted | Use SQLx for the PostgreSQL inbox adapter. |
| [ADR-0015](0015-allow-atomic-sqlx-outbox-enqueue.md) | Accepted | Allow atomic SQLx outbox enqueue. |
