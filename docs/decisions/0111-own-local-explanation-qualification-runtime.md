# ADR-0111: Own the local explanation qualification runtime

- Status: Proposed
- Date: 2026-11-11
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Qualify invented cases through an owned Linux CPU runtime with local file pins.
- Reuse the model HTTP adapter and frozen evaluator for candidate collection and review.
- Keep incomplete suites and missing reviews ineligible for activation.

## Context

ADR-0109 requires a verified deployment observation before model disclosure.
The HTTP adapter deliberately delegates that observation to a trusted launcher.
The frozen suite currently has 20 unresolved reader tasks, so a transport run
can collect evidence while qualification remains incomplete.

## Decision

Use an opt-in Python supervisor to verify the runtime executable, declared
runtime libraries, weights, model license record, prompt, canonical profile,
helper executable and observed hardware. The operator reviews the runtime's
library inventory and model rights before supplying the manifest. The runtime
identity binds executable and library digests, weights, hardware and fixed
CPU launch settings and supervisor source digest. The profile binds that runtime identity and request settings.

The supervisor owns the server and helper as sibling child processes. The
helper checks process start identity, parent, executable bytes, command line,
and exclusive loopback listener ownership before using the existing HTTP port.
During collection it rechecks executable metadata and listener ownership.
The supervisor verifies file pins again before retaining a completed collection,
and terminates its server on success or failure. Local operator control of the
host, filesystem and process namespace is part of this qualification profile.
This launcher qualifies CPU inference only.

Collect all available usefulness cases and the pinned adversarial contexts.
Validate every proposed output independently and retain accepted typed choices,
safe failure categories and exact review presentation identities. Raw rejected
responses are discarded. Reviewers receive a separate shuffled pair file without
candidate labels. Import both independent reviewer records through the existing
evaluator. Authentication of reviewers and review of model licensing remain
operator responsibilities.

## Alternatives considered

A server's claimed model name cannot verify loaded weights. A separate HTTP
implementation would duplicate the bounded adapter's settings and response
validation. Downloads and an inferred default model would introduce unreviewed
model rights and deployment assumptions.

## Consequences

Qualification artifacts remain private and require explicit operator execution.
The current incomplete suite cannot pass. Runtime activation remains a separate
consumer of reviewed qualification evidence. A real run requires operator-supplied
licensed model artifacts, a compatible runtime and two independent reviews.

## Compatibility and migration

Use the bridge as an operator tool alongside the existing services. The manifest
and report formats are local qualification records. A new corpus or runtime
identity requires a fresh collection and matching independent reviews.

## Security and operations

Use the reviewed library inventory and local filesystem under the operator trust
profile. Reports are created outside the repository with mode 0600 and exclusive
creation. Keep the collection report from reviewers and distribute its separate
pair file. Missing inputs and interrupted collection remain unqualified. Stop
using the driver by terminating its owned server and retaining the report.

## Validation

Use the Python manifest tests and the Rust local transport test described in
[the explanation guide](../development/explanations.md#pinned-model-qualification-driver).
The latter owns a real loopback listener and uses the existing model adapter.
