# ADR-0001: Use a Rust workspace with multiple deployables

- Status: Proposed
- Date: 2026-09-18
- Milestone: M01 - Rust engineering foundation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Use the Rust 2024 edition in one Cargo workspace and pin the toolchain in `rust-toolchain.toml`.
- Keep reusable domain and application behavior in library crates; package operational boundaries as separate Rust binaries.
- Begin with five deployables: gateway, market-data, research, dry-run execution, and audit projection.
- Communicate across deployables through versioned commands/events rather than shared internal crates or shared database tables.
- Build identical OCI images for portable Kubernetes and cloud-managed container targets.
- Accept the extra distributed-systems cost because demonstrating those boundaries is an explicit portfolio objective.

## Context

EdgeAgent needs deterministic market research, asynchronous evidence acquisition,
policy-bound publication, durable artifact history, and dry-run order execution.
These capabilities have different scaling, failure, security, and lifecycle
characteristics. The project also aims to demonstrate a credible multi-cloud,
event-driven architecture rather than only the minimum topology needed for an MVP.

This objective changes the optimization target. Operational simplicity remains
important, but the repository should expose service ownership, contract
evolution, asynchronous failure, replay, idempotency, deployment portability,
and observability as first-class engineering work.

Rust provides explicit ownership, strong algebraic modeling, predictable
resource use, small container images, async I/O through Tokio, and a single
toolchain for libraries and deployable binaries. The Rust 2024 edition is an
official language edition and remains compatible with dependencies written in
earlier editions; see the [Rust Edition Guide](https://doc.rust-lang.org/edition-guide/).

## Decision

If accepted, EdgeAgent will use a Cargo workspace with this intended shape:

```text
Cargo.toml
rust-toolchain.toml
crates/
  domain/                 # Value objects, invariants, state machines
  application/            # Use cases and consumer-owned ports
  contracts/              # Versioned external command/event schemas
  market-data-contracts/  # Canonical observations and quality outcomes
  testing/                # Synthetic fixtures and conformance helpers
services/
  gateway/                # HTTP/CLI ingress and query endpoints
  market-data/            # Provider acquisition and evidence snapshots
  research/               # Analytics, strategies, policy, artifact creation
  execution-simulator/    # Dry-run orders, fills, positions, and P&L
  audit-projector/        # Immutable audit/object-store projection
workers/
  surveillance/           # Expiry, invalidation, and scheduled reevaluation
deploy/
  local/                  # Containers and local dependencies
  kubernetes/             # Portable base and cloud overlays
  opentofu/               # Provider-specific capability modules
```

Directory names describe intended ownership, not permission to scaffold every
item immediately. A deployable or crate is created only with observable behavior
and tests in the same change.

The initial five deployment boundaries are:

| Deployable | Owns | Does not own |
|---|---|---|
| Gateway | Request authentication/validation, command acceptance, query composition | Market calculations, policy, order simulation |
| Market data | Provider adapters, canonical observations, evidence snapshots, data-quality outcomes | Strategy ranking or research conclusions |
| Research | Deterministic analytics, approved strategies, risk/publication policy, research artifacts | Provider wire formats or order lifecycle |
| Execution simulator | Dry-run orders, simulated fills, positions, cash, fees, and portfolio marks | Live broker connectivity or research ranking |
| Audit projector | Append-only cross-service audit projection and immutable exports | Authoritative service state or command handling |

Surveillance begins as a worker owned by the research capability and may become
a separate service when it needs independent scaling or state.

Services own their writes. They MUST NOT read or mutate another service's
private tables. Shared library crates contain stable domain vocabulary and
protocol-independent behavior; they MUST NOT become a path for one service to
reach another service's persistence or internal use cases.

Every deployable builds as an OCI image and exposes only documented health,
metrics, command/event, and optional HTTP interfaces. The same image runs on
portable Kubernetes or a managed container platform. Cloud-specific concerns
remain in deployment modules and adapters.

## Alternatives considered

### Python modular monolith

- Benefits: rapid access to quantitative libraries, low local operational cost,
  and a short path to the first research slice.
- Costs and risks: does not meet the portfolio objective of demonstrating Rust,
  service contracts, asynchronous delivery, or deployment boundaries.
- Reason not selected: the project explicitly prioritizes a Rust multi-deployable
  architecture over minimum initial complexity.

### Rust modular monolith with later extraction

- Benefits: simpler transactions, local debugging, and deployment while
  retaining strong language-level boundaries.
- Costs and risks: distributed contracts, outbox/inbox semantics, independent
  scaling, and multi-service observability remain theoretical until late.
- Reason not selected: those concerns are part of the intended demonstration.

### Polyglot services

- Benefits: Python could own quantitative research while Rust owns the control plane.
- Costs and risks: two build systems, duplicated contracts, cross-language
  numerical parity, more contributor setup, and generated bindings.
- Reason not selected: use Rust throughout first. A later specialized worker
  requires an ADR and measured evidence that Rust is insufficient.

### One repository per service

- Benefits: independent permissions, releases, and ownership.
- Costs and risks: high coordination cost for contracts, fixtures, atomic
  compatibility changes, tooling, and an early-stage contributor experience.
- Reason not selected: a monorepo preserves one review and verification surface
  while still producing independent artifacts.

## Consequences

### Positive

- The repository visibly demonstrates bounded contexts, service ownership,
  asynchronous contracts, containerization, and multi-cloud deployment profiles.
- One Cargo workspace enables shared linting, formatting, dependency policy,
  contract fixtures, and atomic compatible changes.
- Rust types can represent identifiers, units, time, lifecycle, and failure
  outcomes without relying on runtime conventions.
- Individual binaries can scale, deploy, and fail independently.

### Negative

- Local development needs a broker and eventually multiple state stores.
- End-to-end tests, schema compatibility, observability, and failure injection
  become mandatory much earlier.
- Cross-service workflows are eventually consistent and cannot rely on one
  database transaction.
- Rust's quantitative ecosystem may require more custom implementations or
  careful use of Arrow/Polars/DataFusion than a Python-first system.
- More deployables increase image, patching, configuration, and operational surface.

### Neutral or follow-up

- A monorepo does not imply one release version. Each binary has an image digest
  and compatibility manifest even when built from one commit.
- Separate deployables do not imply a service per crate. Most crates remain
  internal libraries with no network boundary.
- A web UI is not selected. If added, its language and repository location
  require a separate decision.

## Compatibility and migration

No application code or accepted runtime ADR exists, so replacement requires no
runtime migration. The current Python repository verifier remains a temporary,
dependency-free foundation check until the Rust scaffold provides an equivalent
or stronger check. Removing it requires parity evidence in the same change.

Contracts evolve additively within a major version. Breaking semantic changes
use a new command/event type or payload version and a staged producer/consumer
rollout. Workspace crates are not automatically public compatibility surfaces;
published schemas and service interfaces are.

## Security and operations

- Each service receives a distinct workload identity and least-privilege secrets.
- Service-to-service authorization is enforced at command ingress and data ownership boundaries.
- Containers run as non-root with read-only filesystems where the platform permits.
- Build provenance, dependency auditing, image scanning, and signed image digests
  become required before deployment.
- Multi-service tracing propagates W3C trace context independently of message identity.
- Service failure cannot bypass research or execution policy; consumers fail
  closed on unknown contract versions.

## Validation

The M01/M02 scaffold will validate this decision by proving:

- a pinned Rust 2024 workspace formats, lints, tests, audits, and builds on Linux and Windows;
- at least two binaries share versioned contracts without importing each other's internals;
- each binary builds as a reproducible OCI image;
- architecture tests reject forbidden dependency directions;
- a local environment starts the required services and broker with one command; and
- a synthetic request crosses at least two process boundaries and remains traceable by request, causation, correlation, and event identifiers.
