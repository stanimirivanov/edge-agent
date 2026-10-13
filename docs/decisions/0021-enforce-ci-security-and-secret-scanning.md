# ADR-0021: Enforce CI security and secret scanning

- Status: Accepted
- Date: 2026-10-12
- Milestone: M01 - Rust engineering foundation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Make CI capability restrictions a semantic, fail-closed workflow sensor and add
pinned, checksum-verified Gitleaks scans of source and local Git history.
Default verification checks policy offline; a specialized gate proves the real
detector and scans with full-history checkout before heavier quality jobs.
Preserve only exact public local-fixture exceptions and never suppress exposure
with a baseline or broad ignore.

## Context

The repository already mechanically enforces documentation governance, Cargo
capability boundaries, and verification inventory. CI action identities,
privilege limits, credential handling, and secret prevention also need sensors:
written instructions and successful compilation cannot establish them.

Open-source PRs must validate without private tokens. Release publication needs
narrow GitHub registry, attestation, and OIDC capabilities that validation must
not inherit. Historical exposure survives removal from current source. The
default contributor loop must remain cross-platform and avoid installing tools
or making network calls beyond fetching missing locked dependencies.

## Decision

Add a T1 `cargo xtask workflow-security` sensor to the default verification
inventory. Parse reviewed workflow files as YAML 1.2 with the exact tooling-only
`yaml-rust2` 0.13.0 dependency, with default features disabled. Reject ambiguous
YAML and bound parser inputs. Check reviewed full-SHA action identities and job
scope, least-privilege permissions, safe checkout, trigger inventory, credential
boundaries, and non-skippable mandatory checks. Keep diagnostics payload-safe.

Keep write and OIDC permissions only in the tag-triggered release publish job,
with the existing registry-login and attestation-verification token boundaries.
Retain narrow failure diagnostics and always-run Compose cleanup exceptions.

Pin MIT-licensed Gitleaks 8.30.1 and official Linux x64/Windows x64 archive
digests in repository policy. Install explicitly with a checksum-verifying
Python-standard-library tool; do not use a licensed scanner action or private
token. Extend all upstream detectors with an EdgeAgent configuration-credential
rule. Allow only exact public local-fixture values at their exact canonical
fixture path. The inherited generic API-key detector additionally exempts
the public synthetic `cancellation-order-01` identity only in its exact
`crates/outbox-relay/tests/cancellation.rs` test file; the detector itself stays
enabled. Nested lookalike paths, changed values, and other files remain checked.
Disable inline suppressions and fingerprint/global ignores.

The default `secret-policy` check validates this configuration without invoking
a scanner. The `ci-security` profile requires the actual pinned scanner for
`secret-scanner-contracts` and `secret-scan`. The first proves rejection and
fixture behavior using automatically cleaned synthetic temporary environments;
the second checks current Git source and complete locally reachable history,
explicitly including separate merge diffs and removed merge-only credentials.
Missing tools and incomplete scans cannot produce a passing aggregate.

Repository-quality runs the semantic workflow check early and a read-only,
full-history security job before heavier acceptance, container, local-platform,
and supply-chain jobs. Exact commands and evidence boundaries belong to the
existing executable verification inventory, not a second command registry.

## Alternatives considered

### Keep security rules in contributor instructions

- Benefits: no tooling cost or dependency.
- Costs and risks: review can miss moving action tags, broadened privileges,
  conditional checks, and forgotten historical credentials.
- Reason not selected: deterministic properties require deterministic feedback.

### Inspect workflow text with regular expressions

- Benefits: small implementation with no YAML dependency.
- Costs and risks: comments, folded scalars, duplicate keys, and YAML structure
  can change behavior without matching line-oriented assumptions.
- Reason not selected: parse semantics with a maintained library and reject
  unsupported forms explicitly.

### Use a hosted or licensed secret-scanner action

- Benefits: convenient installation and hosted reporting.
- Costs and risks: private license tokens, action execution capability, network
  dependence, and less control over disclosure or exception policy.
- Reason not selected: a pinned standalone MIT tool supports open-source local
  verification and safe wrapper-owned evidence.

### Download a scanner during every default verification

- Benefits: one command would always run source and history scans.
- Costs and risks: network failure and platform downloads enter the cheap loop;
  installation becomes an implicit side effect.
- Reason not selected: keep policy deterministic by default and detector
  execution explicit in a required specialized CI gate.

### Adopt broad path ignores or a historical baseline

- Benefits: fewer initial findings and less triage.
- Costs and risks: hidden exposure and permissive exceptions survive unnoticed.
- Reason not selected: exceptions must match both public fixture value and path;
  suspected historical exposure needs private remediation, not suppression.

## Consequences

### Positive

- CI privileges and action identities become reviewable enforced capabilities.
- Mutation tests catch removal and bypass of mandatory checks.
- Real-scanner proofs distinguish detector execution from static policy checks.
- Historical credentials remain visible after current-tree removal.
- Contributor verification requires no provider, scanner-license, or cloud token.

### Negative

- New action, workflow, or scanner releases require coordinated policy updates.
- The YAML parser enlarges repository tooling, but no production dependency.
- Full-history scanning adds CI time and can block on an old exposure.
- Explicit installation currently supports only Linux x64 and Windows x64.

### Neutral or follow-up

- Regex detectors do not certify privacy, licensed-data absence, model safety,
  dry-run behavior, or all possible secrets.
- Locally absent refs and unreachable objects are outside the scan's claim.
- GitHub branch protection and approval remain maintainer-owned external state.
- Runtime dry-run and artifact capability enforcement remain separate M07 work.

## Compatibility and migration

No public Rust API, event, persisted schema, or deployable runtime changes.
Validation action tags become reviewed SHA pins. The default gate gains offline
policy checks, and repository-quality gains a prerequisite security job.
Contributors install the scanner only for specialized verification. Rollback
reverts the coherent tooling/policy slice; it requires no data migration and
must not hide findings with a temporary suppression.

## Security and operations

Require read-only validation tokens and disabled persisted checkout credentials.
Release permissions and two token consumers remain explicit exceptions. The
scanner reads repository source and local Git objects, not production services.
Verify archive digests before extracting the exact executable. Keep synthetic
secrets out of committed fixtures and raw diagnostic output.

The wrapper suppresses scanner diagnostics, validates redacted temporary
reports, and exposes only safe finding coordinates. Revoke or rotate suspected
credentials and use private reporting; current-tree cleanup does not erase
historical exposure. Do not automatically rewrite history, upload secret-bearing
evidence, or weaken detection to recover CI. Failed downloads, incorrect
digests, timeouts, incomplete history, and missing tools remain blockers.

## Validation

Rust mutation tests cover YAML ambiguity, action identity, permissions,
checkout, unsafe triggers, credential boundaries, dependency order, and removed
or skippable mandatory commands. Python tests cover pin/configuration drift,
archive authentication, safe extraction, redacted evidence, and incomplete
execution. Default Linux/Windows acceptance runs these deterministic tests and
policy checks without installing a scanner.

The specialized `ci-security` profile executes the actual pinned scanner's
synthetic proofs, including upstream detection, exact path/value exceptions,
inline-suppression rejection, and removed-token history. CI checks out full
history and runs source plus history scanning before heavier jobs. A static
policy pass or successful binary installation never substitutes for those
behavioral results. Operational commands and evidence limits are in the
[CI security guide](../development/ci-security.md).
