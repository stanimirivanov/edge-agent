# Architecture decision records

## TL;DR

- Use an ADR for durable decisions affecting compatibility, evidence meaning, security, data, topology, ownership, or foundational technology.
- Number ADRs sequentially and never reuse a published number.
- Accepted ADRs are historical records; supersede rather than rewrite them.
- Record alternatives, consequences, migration, security, operations, and validation.
- The Rust workspace, portable event architecture, and dry-run execution boundaries are accepted foundations.

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

Statuses are Proposed, Accepted, Rejected, Deprecated, or Superseded. A
superseded ADR links to its replacement. Do not edit the decision or
consequences of an accepted ADR to make history appear cleaner; add a dated
note or a new ADR.

Before allocating a number, inspect this index and the decision directory from
fresh repository state. Use the lowest unused number. Resolve a concurrent
collision by renumbering the later unpublished ADR.

## Template

```markdown
# ADR-NNNN: Decision title

- Status: Proposed
- Date: YYYY-MM-DD
- Milestone: MNN - Outcome
- Deciders:
- Supersedes:
- Superseded by:

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
