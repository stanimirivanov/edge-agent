# EdgeAgent documentation map

## TL;DR

- Start with [CONTRIBUTING.md](../CONTRIBUTING.md), then use this map to read
  only the sources relevant to the task.
- Product intent, architecture, engineering practice, operations, roadmap, and
  durable decisions have separate canonical homes.
- Update this map when a canonical source moves or a recurring task lacks a
  clear route.
- Keep API detail in rustdoc; add a focused guide only when contributors or
  operators need a workflow that code cannot explain.

## Purpose and authority

This page is the progressive-disclosure entry point for contributors and
coding agents. It routes readers to canonical sources instead of repeating
their policy. [CONTRIBUTING.md](../CONTRIBUTING.md) owns workflow, issue shape,
verification, and completion reporting. [AGENTS.md](../AGENTS.md) is the
concise tool-facing agreement.

Read the rows that match the work. Do not bulk-read every guide or decision.
Always consult the product and architecture sources when a change affects
observable behavior, evidence meaning, ownership, or a dependency boundary.

## Route by task

| Task or changed area | Read before changing it | Verify or consult as needed |
|:--|:--|:--|
| Product behavior, vocabulary, evidence meaning, or non-goals | [Product vision](product/vision.md) | [System architecture](architecture/system-overview.md), relevant [ADRs](decisions/README.md) |
| Capability ownership, crate roles, or dependency direction | [System architecture](architecture/system-overview.md), [dependency rules](architecture/dependency-rules.md) | [Engineering standards](development/engineering-standards.md), `make architecture` |
| Rust implementation, API design, or tests | [Engineering standards](development/engineering-standards.md) | Relevant capability guide, `make repository`, then `make verify` |
| Event contracts, routing, consumers, outbox, or inbox | [Message contracts](development/message-contracts.md), [messaging adapters](development/messaging-adapters.md) | [Event-spine conformance](development/event-spine-conformance.md), relevant persistence guide |
| Images, dependencies, releases, or artifact provenance | [Supply chain](development/supply-chain.md), [release guide](development/releases.md) | `make supply-chain`, `make image-smoke` when applicable |
| Local runtime dependencies or recovery tests | [Local platform](development/local-platform.md) | `make local-up`, relevant conformance guide |
| Deployment ownership or cloud portability | [Deployment portability](architecture/deployment-portability.md) | [System architecture](architecture/system-overview.md), relevant ADRs |
| Repository policy, CI, agent guidance, or developer experience | [Coding harness](development/harness.md) | `make repository`, `make architecture` |
| Milestone or issue planning | [Milestones](roadmap/milestones.md), [CONTRIBUTING.md](../CONTRIBUTING.md) | Relevant product and architecture sources |
| Vulnerability reporting or security-sensitive behavior | [Security policy](../SECURITY.md) | Relevant architecture and ADRs |

## Canonical collections

- [Product](product/vision.md) defines purpose, users, scope, and non-goals.
- [Architecture](architecture/system-overview.md) defines system boundaries,
  ownership, trust, and information flow.
- [Development](development/engineering-standards.md) defines implementation
  and testing practices; focused guides define individual workflows.
- [Architecture decisions](decisions/README.md) record durable choices and
  lifecycle history.
- [Roadmap](roadmap/milestones.md) groups intended delivery into outcomes.
- [Security](../SECURITY.md) defines vulnerability reporting and security
  expectations. [CODE_OF_CONDUCT.md](../CODE_OF_CONDUCT.md) defines community
  behavior.

## Keeping the map useful

A change updates this page when it adds, moves, renames, or removes a canonical
source or creates a recurring task without a route. Avoid listing short-lived
implementation details. `cargo xtask repository` checks mechanical document
policy and repository-local navigation; semantic accuracy and stale guidance
still require review.
