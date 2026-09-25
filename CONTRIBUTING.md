# Contributing to EdgeAgent

## TL;DR

- Deliver one coherent, verified capability per issue and pull request.
- Proceed on documented, reversible assumptions; request a decision when ambiguity changes behavior, safety, compatibility, data meaning, or external state.
- Keep deterministic market analysis and policy independent from adapters, infrastructure, and generative models.
- Preserve service ownership, at-least-once delivery semantics, and the dry-run-only execution boundary.
- Use an existing issue when supplied. Otherwise implement the smallest coherent scope and include a proposed issue in the completion report.
- Run the exact repository checks. Report blocked or unavailable checks as **not run**, never as passed.
- Name milestones `MNN - Outcome`, starting with M01, and identify one milestone in every issue.
- Include tests, documentation, risk review, assumptions, and verification evidence with implementation.

## Policy language and sources of truth

The uppercase terms in this repository have these meanings:

- **MUST** and **MUST NOT** identify requirements. Deviations require an explicit reviewer-approved exception.
- **SHOULD** and **SHOULD NOT** identify strong defaults. A deviation records its reason and trade-off.
- **MAY** identifies an optional choice.

Lowercase wording is explanatory and does not create hidden policy.

| Concern | Canonical source |
|---|---|
| Product purpose, scope, and invariants | [Product vision](docs/product/vision.md) |
| Architecture, ownership, and control flow | [System architecture](docs/architecture/system-overview.md) |
| Deployment profiles and provider capability mapping | [Deployment portability](docs/architecture/deployment-portability.md) |
| Contributor workflow and completion | This document |
| Concise contributor and agent entry point | [AGENTS.md](AGENTS.md) |
| Engineering and testing practice | [Engineering standards](docs/development/engineering-standards.md) |
| Durable decisions | [Architecture decisions](docs/decisions/README.md) |
| Intended sequencing and scope | [Milestones](docs/roadmap/milestones.md) |
| Security reporting | [SECURITY.md](SECURITY.md) |
| Exact local commands | [README.md](README.md), `Makefile`, and checked-in tool configuration |

The narrower source governs its stated concern. An accepted ADR governs the
decision it records until superseded. Contributors MUST surface unresolved
conflicts rather than silently choosing one source.

## Before starting

A contributor MUST:

- inspect the working tree and preserve unrelated changes;
- read the relevant product, architecture, roadmap, standards, and accepted ADRs;
- identify the smallest observable outcome that can be reviewed independently;
- identify affected public contracts, evidence meaning, security boundaries,
  provider behavior, documentation, and operations; and
- select the issue workflow below.

Do not discard, overwrite, reformat, or absorb unrelated work to obtain a clean
diff. Destructive changes and writes to external systems require explicit
authorization.

An ADR is required when a change establishes or revises a durable decision
about public compatibility, persistence, evidence meaning, security, deployment
topology, foundational technology, model or market-data boundaries, ownership,
or cross-component behavior.

## Ambiguity and escalation

Before escalating, inspect relevant code, tests, fixtures, contracts,
documentation, decisions, and issue history.

A contributor MAY proceed with a documented assumption only when the choice:

- stays inside the stated goal and exclusions;
- is local, reversible, and inexpensive to change;
- does not alter public compatibility, persisted meaning, security/privacy,
  market-data rights, external state, or destructive behavior;
- does not weaken acceptance criteria or verification; and
- would not materially change what a reviewer believes they are approving.

The contributor MUST request a decision when missing information can materially
change behavior, scope, acceptance, safety, compatibility, evidence meaning,
data loss, authorization, privacy, operational ownership, cost, deployment, or
external state. State the exact decision, evidence checked, viable options,
consequences, and safe work that can continue.

## Issue timing and structure

Issue-first is preferred. Issue-after is the traceability fallback when work is
requested without an issue.

- When an issue exists, follow its goal, milestone, and acceptance criteria.
- When no issue exists, implement the smallest coherent scope and include a
  complete proposed issue in the completion report.
- Do not create a GitHub issue, milestone, pull request, release, or other
  external record unless requested or explicitly authorized by the workflow.

Milestone titles use `MNN - Short outcome`. Published numbers are immutable and
must not be reused. Refresh GitHub before allocating a new milestone number.

Implementation issues use this body:

```markdown
**Milestone:** MNN - Outcome

## Goal

Describe the problem and observable result.

## Scope

- Included behavior and boundaries.

## Design decisions

- Assumptions, constraints, compatibility effects, and ADR links.

## Acceptance criteria

- [ ] Observable behavior and verification evidence.
- [ ] Relevant failure or negative behavior.
- [ ] Documentation and operational effects.

## Out of scope

- Explicit exclusions and deferred work.
```

Acceptance criteria describe observable behavior or verifiable invariants, not
implementation activities.

## Pull-request-sized work

A pull request MUST:

- solve one problem or deliver one coherent vertical capability;
- keep the repository buildable and verifiable;
- include implementation, tests, contracts, documentation, and migrations
  needed for that capability;
- provide evidence for success and important rejection/failure behavior; and
- remain reviewable without an unmerged speculative follow-up.

Split independent behavior, broad cleanup, dependency upgrades, schema
redesign, unrelated formatting, and separate architectural decisions. Do not
create empty packages, placeholder ports, generic repositories, or unused
services solely to mirror a future diagram.

## Development workflow

1. Select the issue workflow and milestone.
2. Create a focused branch when the surrounding workflow uses branches.
3. Add or update tests with behavior.
4. Implement the smallest coherent solution.
5. Format and run the exact repository checks.
6. Review the entire diff for secrets, licensed data, generated churn,
   accidental compatibility changes, and unrelated edits.
7. Update affected documentation, decisions, operational guidance, and
   migration notes.
8. Open a linked pull request only when authorized.
9. Produce the completion report.

Commit subjects SHOULD be imperative and specific. Generated output is
reproducible and is committed only when consumers cannot reasonably generate it.

## Verification and constrained environments

Run every applicable required repository check supported by the environment.
The current foundation requires:

```text
python scripts/verify_repository.py --format-check
python scripts/verify_repository.py
python scripts/verify_architecture.py
python scripts/verify_images.py
python scripts/verify_local_stack.py
python scripts/verify_supply_chain.py
python scripts/verify_release.py
python -m unittest discover -s tests -p "test_*.py"
cargo fmt --all --check
cargo metadata --locked --offline --format-version 1 --no-deps
cargo check --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets
```

`make verify` runs the same commands where `make` is available. Application
tooling may add stricter commands but must preserve a documented one-command
verification entry point.

Changes to `Dockerfile`, `.dockerignore`, `deploy/images.toml`, service package
names, component descriptors, or runtime packaging MUST also run
`make image-smoke` where Docker is available. If Docker is unavailable, report
the image smoke test as **not run**; the Ubuntu CI job remains required before merge.

Changes to `deploy/local/`, its image versions, ports, credentials, readiness
rules, or dependency configuration MUST run `make local-up` and
`make local-down` where Docker is available. Never substitute production
credentials or licensed data into the checked-in local profile.

Changes to Cargo dependencies, `Cargo.lock`, `deny.toml`, `Dockerfile`,
`deploy/images.toml`, `supply-chain/tools.toml`, `supply-chain/release.toml`,
release automation, or SBOM generation MUST run
`make supply-chain` where the pinned tools and Docker are available. If a tool,
network, or daemon is unavailable, report the affected policy or artifact stage
as **not run**; the Ubuntu supply-chain CI job remains required before merge.

The examples use `python`; contributors MAY substitute the platform's Python
3.11+ launcher, such as `python3` on POSIX or `py -3` on Windows. This changes
only executable discovery, not the check being run.

When a required check cannot run because of sandbox restrictions, missing
network, credentials, services, fixtures, platform support, or another
constraint, report:

1. the exact check as **not run**;
2. the blocking condition and safe attempts made;
3. evidence from checks that did run;
4. residual risk and where the missing check should run; and
5. no fabricated or substituted passing result.

A failing check remains **failed**, even if unrelated. Identify verified
pre-existing failures separately.

## Market-research integrity

Changes affecting research output require focused review of these invariants:

- Numeric and time-sensitive facts come from deterministic, versioned
  transformations over identified evidence.
- Provider payloads, retrieved content, user input, and model output are
  untrusted at their boundaries.
- Missing, stale, contradictory, out-of-entitlement, or corrupt critical data
  causes a typed abstention or explicit degradation—not invented continuity.
- Every research artifact identifies event time, observation time, source,
  strategy version, policy version, expiry, and material assumptions.
- Published artifacts are immutable. Corrections, invalidation, expiry, and
  withdrawal are appended lifecycle events.
- Evaluation uses point-in-time inputs, frozen versions, explicit market
  calendars and corporate actions, representative costs, and declared fill
  rules. Look-ahead and survivorship bias are defects.
- Generated language cannot authorize an action, bypass policy, create a
  market fact, or certify its own output.
- Live-provider tests are opt-in and isolated. Default CI requires no secrets,
  paid services, or mutable external data.
- Cross-service handlers assume at-least-once delivery and preserve one domain
  transition under duplicate, delayed, reordered, or redelivered messages.
- Dry-run orders, fills, positions, cash, and P&L are deterministic,
  point-in-time, versioned, append-only, and replayable.
- The execution simulator accepts only `dry_run` and contains no broker
  credential, broker endpoint, or live broker adapter.

Any change that can transmit an order to a live venue, connect to a brokerage
account, or introduce a live execution mode is outside the current product
boundary. It requires explicit maintainer approval, a separate deployable,
accepted architecture decisions, and a dedicated security review before
implementation.

## Documentation

A long document MUST contain `## TL;DR` near its start when it has 800 or more
words, more than five second-level sections, or is an architecture, security,
operational, migration, product, or end-to-end guide.

Documentation is part of the contract. Public behavior, configuration,
failure semantics, operations, and troubleshooting change with implementation.
Examples SHOULD be executable or mechanically verified. Accepted ADRs are
historical records; supersede rather than rewrite them.

## Pull request description

A pull request MUST state:

- linked issue and exact milestone;
- problem and resulting behavior;
- scope, exclusions, assumptions, and unresolved questions;
- design, data-meaning, and compatibility decisions;
- verification commands and outcomes, including checks not run;
- market-data, model-trust, security, migration, rollout, rollback, and
  operational considerations; and
- known limitations and follow-up work.

## Review checklist

- [ ] The issue belongs to one milestone and follows the required structure.
- [ ] The change is one coherent capability with explicit exclusions.
- [ ] Material assumptions and unresolved questions are visible.
- [ ] Dependencies point inward and infrastructure does not own domain policy.
- [ ] Public contracts and persisted meaning are compatible or have an approved evolution plan.
- [ ] Tests cover observable success and important rejection/failure paths.
- [ ] Time, units, evidence lineage, lifecycle, idempotency, cancellation, and retries are explicit where relevant.
- [ ] Event consumers tolerate duplicate, delayed, reordered, and poison messages where relevant.
- [ ] Execution changes preserve dry-run-only capability and prove no live broker path exists.
- [ ] Untrusted input, secrets, privacy, licensing, and model boundaries were reviewed.
- [ ] Backtests and simulations protect point-in-time integrity.
- [ ] Public contracts and non-obvious invariants are documented.
- [ ] Applicable checks passed; unavailable checks are reported honestly.
- [ ] No secrets, personal data, licensed datasets, local caches, or unrelated changes are included.
- [ ] An ADR exists when the decision meets the ADR threshold.

## Completion report

After every completed work item, print:

1. the exact milestone in the form `MNN - Outcome`;
2. a proposed GitHub issue title;
3. the complete issue body using the required structure and matching the work performed;
4. assumptions, unresolved questions, and limitations; and
5. verification commands and outcomes, explicitly classified as **passed**, **failed**, or **not run**.

The issue-after report is a traceability record; it does not imply that an
issue, milestone, or pull request was published.
