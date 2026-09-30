# ADR-0008: Enforce progressive coding-harness policy

- Status: Accepted
- Date: 2026-09-29
- Milestone: M01 - Rust engineering foundation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Add a task-routed documentation map and a fast Rust repository-policy sensor
alongside the existing Python text verifier. Run repository policy before
architecture and heavier CI work. The sensor validates rendered local links,
TL;DR placement, ADR and milestone integrity, and stable GitHub authoring
templates without accessing the network or entering production artifacts.

## Context

EdgeAgent records contributor workflow, architectural constraints, decisions,
milestones, and review expectations in the repository. Those contracts guide
implementation, but several deterministic rules previously depended on a
reviewer noticing drift. A renamed heading could break an inbound fragment, an
ADR could be omitted from the index, milestone boundaries could diverge from
their index, and the GitHub templates could stop asking for evidence required
by contributor policy.

The existing Python repository verifier remains valuable. It checks UTF-8,
line endings, final newlines, required paths, and a simple independent
documentation baseline. Its regular-expression link and heading checks do not
model rendered CommonMark structure, exact path capitalization on a
case-insensitive workstation, duplicate heading anchors, or governance
lifecycle relationships.

ADR-0007 established an executable Cargo dependency policy in the tooling-only
`edgeagent-xtask` package. That package never enters a deployable binary or OCI
image and provides the appropriate home for an additional fast repository
sensor. The harness also needs a task-oriented documentation entry point so a
contributor can load relevant guidance without treating the entire repository
as mandatory context.

## Decision

Extend `edgeagent-xtask` with `cargo xtask repository`. The command discovers
policy-controlled Markdown documents beneath the repository root and emits all
deterministic violations in stable path and line order. Diagnostics identify
the rule, violation, likely correction, and canonical policy location.

Use `pulldown-cmark` for rendered Markdown events and source spans, and
`percent-encoding` for link targets. The command validates:

- contained repository-local paths with exact component capitalization,
  directory README resolution, percent decoding, and existing GitHub-style
  heading fragments;
- a visible first level-two `TL;DR` for long and policy-sensitive documents;
- EdgeAgent ADR filenames, headings, metadata, required section order,
  contiguous numbering, published high-water mark, singleton index coverage,
  lifecycle states, and reciprocal supersession declarations;
- milestone index numbering, titles, outcome statements, direct boundary
  headings, and the published high-water mark; and
- stable capability and bug issue forms, issue-form configuration, and pull
  request headings, verification commands, constrained-environment prompts,
  and ordered checklist.

Comments, images, raw HTML, and fenced code do not satisfy visible heading or
summary requirements. Raw HTML links and external URL availability remain
outside the deterministic local command. Stable governance contract shapes
live in checker code so an edit cannot weaken both an authoring template and
its check in the same data file.

Keep `scripts/verify_repository.py` and the Rust sensor complementary. The
Python tool owns byte-level formatting and required-path inventory. The Rust
tool owns rendered Markdown semantics and repository governance. `make
repository` runs both. CI runs Rust repository policy before architecture and
before all build, test, integration, image, and supply-chain jobs while
preserving the existing prerequisite job identifier and displayed check name.

Add `docs/README.md` as the task-routing map and
`docs/development/harness.md` as the feedback-tier inventory. The harness
orders feedback into task routing, fast structural checks, complete default
acceptance, and affected-boundary specialist checks. A recurring finding moves
to the cheapest reliable combination of canonical guidance, deterministic
sensor, focused behavioral test, or explicit owned exception.

## Alternatives considered

### Keep repository governance as review prose

- Benefits: No new tooling dependencies or implementation work.
- Costs and risks: Deterministic drift continues to consume review attention
  and can remain unnoticed after merge.
- Reason not selected: Review judgment is better reserved for semantics,
  trade-offs, and maintainability than mechanically decidable structure.

### Expand the Python regular-expression verifier

- Benefits: Reuses an existing script and Python standard library.
- Costs and risks: A growing partial Markdown grammar would mishandle rendered
  visibility, reference links, code fences, source spans, and anchor behavior.
- Reason not selected: A maintained CommonMark parser provides a sounder
  structural boundary while the Python verifier retains its distinct strengths.

### Use an external link or documentation service

- Benefits: Could include external URL availability and hosted reports.
- Costs and risks: Adds network variability, rate limits, third-party state,
  and possibly credentials to a check whose local rules can be deterministic.
- Reason not selected: The fast contributor loop and default CI must remain
  credential-free and reproducible.

### Add an LLM reviewer or broad quality thresholds

- Benefits: Could flag prose concerns beyond deterministic structure.
- Costs and risks: Nondeterminism, false confidence, opaque scoring, cost, and
  pressure to optimize global coverage or file size instead of behavior.
- Reason not selected: Semantic review remains human-owned; focused sensors
  and behavior tests provide stable evidence.

## Consequences

### Positive

Broken local navigation, hidden or missing summaries, ADR lifecycle drift,
milestone mismatch, and template drift fail before expensive checks. The task
map reduces unnecessary context loading, and diagnostics lead contributors to
the governing policy and likely correction.

### Negative

The tooling package gains two pinned parsing dependencies. Stable governance
changes now require synchronized policy, implementation, focused mutation
tests, and template updates. Exact template enforcement intentionally makes
casual wording changes review-visible.

### Neutral or follow-up

The sensor does not prove factual accuracy, clarity, current external links,
or correct implementation behavior. Those remain review and test concerns.
Harness health metrics, fuzzing of parsers, and additional behavior sensors may
be added when evidence justifies them; they are not required by this decision.

## Compatibility and migration

This change affects only contributor tooling, documentation, CI ordering, and
GitHub authoring templates. Production packages, message contracts, persisted
data, services, images, and deployment profiles remain unchanged. The existing
architecture job identifier and displayed check name remain stable for
branch-protection compatibility.

The published ADR high-water mark advances to 0008. Future ADR additions must
advance the checker-owned high-water constant in the same change. The milestone
high-water mark remains M11. Existing documentation is corrected where needed
to satisfy rendered-link and TL;DR policy; accepted decision substance is not
rewritten.

## Security and operations

Repository policy performs no network calls, executes no document content, and
canonicalizes local paths before accepting them. Links cannot escape the
repository through traversal or symlink resolution. CI checkout steps disable
persisted GitHub credentials so later build and test processes cannot reuse the
checkout token from Git configuration.

The parser treats repository Markdown and link targets as untrusted input. It
reports malformed paths and encoding without panicking. The new dependencies
remain isolated to `edgeagent-xtask`, use exact workspace pins, enter the
committed lockfile, and remain subject to dependency, advisory, license, and
SBOM policy.

## Validation

Focused fixture mutations cover link containment, exact case, fragments,
rendered TL;DR visibility, ADR identity and lifecycle, milestone correspondence,
and issue or pull request template drift. The real repository must pass both
`make repository` and `make architecture` before the complete `make verify`
gate. Dependency-policy and SBOM checks validate the changed lock graph, while
CI independently runs the gate on Linux and Windows and runs the specialized
supply-chain job on Linux.
