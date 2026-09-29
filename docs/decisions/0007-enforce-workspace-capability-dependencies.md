# ADR-0007: Enforce workspace capability dependencies

- Status: Accepted
- Date: 2026-09-29
- Milestone: M01 - Rust engineering foundation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Add a repository-owned `edgeagent-xtask` package and expose
  `cargo xtask architecture` through a Cargo alias.
- Classify every workspace package by architectural role and allowlist every
  direct internal dependency as normal, development, or build policy.
- Read Cargo's normalized, locked metadata so aliases, optional dependencies,
  and target-specific declarations cannot evade the check.
- Reject missing or stale policy, unapproved edges, and production dependency
  cycles in local verification and CI.
- Keep the existing Python architecture verifier for its complementary manifest
  and repository-structure guarantees. Configure no temporary exceptions.

## Context

EdgeAgent documents inward dependencies, consumer-owned ports, adapter-owned
technology concerns, and composition roots that select concrete implementations.
Cargo manifests can still add any dependency that compiles. Without an
executable project-specific graph, reviewers must reconstruct the intended
architecture across every manifest and distinguish legitimate conformance-test
composition from a production dependency inversion.

The repository already has a dependency-free Python verifier. It validates
workspace membership, manifest metadata, target shape, publication settings,
and exact dependency keys. That check is intentionally based on source
manifests. It does not model package roles, resolve dependency aliases by their
actual workspace package, or prove the allowed production graph is acyclic.

Architecture prose explains intent, and the Python gate protects the scaffold,
but neither alone provides a Cargo-native capability boundary. The workspace
needs a small deterministic check that fails before an accidental package edge
becomes established design.

## Decision

Add `edgeagent-xtask` under `crates/xtask` and define the `cargo xtask` alias in
repository Cargo configuration. `cargo xtask architecture` requests locked
workspace metadata through the maintained `cargo_metadata` crate without the
resolved dependency-node graph. It derives direct internal edges from Cargo's
normalized package paths and compares them with repository-owned policy.

The policy classifies each workspace package as contract, port, application,
adapter, observability, runtime support, composition root, or tooling. It
records normal, development, and build dependencies separately and fails when:

- a workspace package is unclassified;
- policy names a package that no longer exists;
- a direct internal edge lacks an allowance for its dependency class;
- an allowance remains after its edge is removed; or
- the allowed normal-and-build graph contains a cycle.

Optional and target-specific dependencies remain visible without being active
on the host that runs the check. Renaming a dependency key cannot bypass the
policy because identity comes from the resolved workspace package path.
Registry dependencies and path dependencies outside the workspace remain under
the existing manifest, lockfile, review, and supply-chain policies.

The current policy has no temporary exceptions. If a future migration cannot
avoid one, the exception requires an accepted ADR, an exact source, target, and
dependency class, a tracked removal task and reason, and a visible successful-run
diagnostic. The check must reject the exception as stale when the edge disappears.

The Python architecture verifier remains required. It owns exact workspace
member paths, package names, target types, inherited metadata and lints,
publication settings, and complete dependency-key allowlists, including
third-party crates. The Rust command owns Cargo-normalized internal roles, edge
classes, stale policy, and cycle detection. Removing either gate requires parity
evidence for all of its guarantees in the same change.

Local verification, CI, contributor guidance, and pull-request evidence include
`cargo xtask architecture`. Because the command needs neither PostgreSQL nor a
message broker, it runs before environment-backed checks.

## Alternatives considered

### Rely on architecture review and diagrams

- Benefits: no maintenance code or additional build dependency.
- Costs and risks: prose cannot reject drift, and reviewers must repeatedly
  reconstruct exact normal and development graphs.
- Reason not selected: package-boundary regression must fail automatically.

### Extend only the Python manifest verifier

- Benefits: preserves one lightweight, dependency-free implementation.
- Costs and risks: duplicating Cargo normalization would be fragile around
  aliases, target-specific dependencies, optional declarations, and workspace
  inheritance; cycle analysis would still be based on a partial Cargo model.
- Reason not selected: Cargo should remain the source of dependency facts.

### Infer edges from a total layer order

- Benefits: less policy to enumerate as packages are added.
- Costs and risks: contract, application, adapter, runtime, and test
  relationships are more precise than one universal rank; broad rules permit
  coupling that EdgeAgent does not need.
- Reason not selected: exact edges produce clearer diagnostics and reviews.

### Adopt a source-level architecture framework

- Benefits: could inspect modules or APIs below the crate boundary.
- Costs and risks: adds more machinery without preventing the Cargo-level edge
  being protected and does not replace repository-specific policy.
- Reason not selected: direct workspace dependencies are the stable boundary
  required for this increment.

## Consequences

### Positive

- Adding a package or changing an internal edge now requires a visible policy
  decision in the same pull request.
- Development-only test composition cannot silently authorize production
  coupling.
- Cargo-native discovery covers aliases and conditional declarations while
  retaining deterministic, human-readable policy.
- Boundary violations fail locally and in CI before they become architectural
  precedent.

### Negative

- The exact graph has maintenance cost and must change when an intentional edge
  changes.
- The tooling package adds a development dependency and a small compilation
  cost to the quality gate.
- Two complementary verifiers create some intentional overlap that must remain
  consistent.

### Neutral or follow-up

- The checker validates crate dependencies, not module cohesion, API quality,
  file size, runtime topology, or the suitability of third-party dependencies.
- `edgeagent-xtask` is repository tooling. It does not enter a production
  service, OCI image, or deployment inventory.
- Capability extraction remains pull-request-sized behavior work rather than a
  reason to create empty crates.

## Compatibility and migration

The change affects repository verification and contributor workflow only. It
does not change commands, events, persisted data, service behavior, or runtime
topology. Existing internal edges become the initial exact policy, including
the development-only adapter edges used by event-spine conformance tests.

Every future workspace package and direct internal dependency change must
update both enforcement representations. The current Python verifier remains
until a later accepted decision demonstrates complete replacement parity.

## Security and operations

The architecture evaluation reads repository Cargo metadata and performs no
database, broker, credential, or deployment operation. Cargo may fetch and build
the locked tooling dependency on a fresh development host; normal dependency and
network policy still applies to that bootstrap. Locked metadata prevents the
check from silently changing workspace resolution. Exact policy reduces the
risk that domain or application code acquires infrastructure capabilities
through an unnoticed workspace edge.

The new tooling dependency remains subject to lockfile, advisory, license,
source, and SBOM policy. SBOM generation evaluates the workspace once, publishes
only the deployables declared by `deploy/images.toml`, and removes transient
tooling SBOMs so repository tools do not become release artifacts.

## Validation

- The current workspace passes `cargo xtask architecture` with zero temporary
  exceptions.
- Focused unit tests reject unclassified packages, stale policy, unapproved
  normal and development edges, dependency-class substitution, and production
  cycles.
- A focused path-resolution unit test proves that package identity comes from
  the workspace path rather than a renamed manifest key.
- The existing Python architecture test suite continues to pass.
- CI runs the command as a required architecture step in the repository-quality
  gate.
