# ADR-0120: Compose guarded owned explanations

- Status: Accepted
- Date: 2026-11-13
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Add an opt-in finite owned worker; preserve the default deterministic worker.
- Collect and execute with the same pinned service helper and shared preparation.
- Require exact runtime qualification, current disclosure and one durable attempt.
- Real model qualification and activation remain M06-PR33.

## Context

ADR-0119 establishes production/qualification preparation parity. Composing the
model port requires an owned process, reviewed artifact identities, current owner
and Research permissions and transaction-fenced qualification. A standalone HTTP
endpoint or an environment enable flag cannot establish these properties.

## Decision

Add canonical `research.explanation-qualification.review.v2` for `approve_runtime`.
Independently rescore the runtime suite and actual prepared adversarial context
identities; use separate v2 collection and assessment domains. Preserve historical
v1 approvals, revocations and their exact identities. Existing owner administration
and explicit provenance/reviewer-independence attestation apply unchanged.

Use the same `edgeagent-research` executable for private runtime collection and
`offline-explanation-worker-owned`. The launcher v2 manifest retains the existing
bounded artifact fields. Its runtime identity hashes hardware, threads, context,
runtime/weights/helper/library digests, the launcher source, the shared qualification
supervisor source and its hashing support source. Exclude paths and ephemeral ports;
exclude the profile file to avoid a circular profile identity. Prompt and profile
remain independently pinned, and qualification binds the entire exact manifest.
Rust independently reproduces this identity from compiled supervisor sources.
Changes to the helper require a fresh runtime/profile/manifest and qualification.

Recover the durable job and any spent attempt before runtime startup or owner
access. The helper emits a bounded control record only after preparing independently
reconciled owner evidence and checking qualification and disclosure. Its sibling
supervisor then verifies reviewed files, launches a CPU-only IPv4-loopback model,
and passes the observed PID/start/arguments/listener. The helper independently
checks Linux process ownership, actual executable bytes and listener ownership.
The model receives a clean environment containing no database or owner credentials.
The supervisor bounds output/time and terminates and reaps both children.
A child-side exec wrapper sets Linux parent-death SIGKILL and rejects set-ID or
capability-bearing targets, whose exec transitions would clear that signal
([Linux manual](https://man7.org/linux/man-pages/man2/PR_SET_PDEATHSIG.2const.html)).
It checks the expected parent after installing the signal to close the startup
race. This avoids Python fork callbacks and prevents live helper/model orphans
when the supervisor is killed; orphaned dead children are reaped by their adopter.

Reserve before exactly one HTTP call. A qualified send token carries the exact
review observation and authenticated owner permit; it cannot be reconstructed
from a spent row. The durable row already retains exact profile/input/deadline and
Research grant revision. The immutable qualification ledger retains the reviewed
assessment and manifest. No new schema or reusable owner bearer token is needed.
Check exact current qualification and model disclosure under the reservation and
completion transaction fences, alongside existing serving/lifecycle fences.
Recheck the owner externally before dispatch and after inference, using a fresh
challenge nonce for each authenticated exchange and preserving every disclosure
subject field. Interrupted,
ambiguous or late attempts remain spent and recover to deterministic fallback;
no retry obtains another send token. Generated choices require shared validation
and commit atomically with companion publication. Serving rights stay independent.

## Alternatives considered

Reusing a historical fixture approval would misstate runtime qualification.
Launching before recovery would add an external dependency to receipt recovery.
An arbitrary configured URL, synthetic process verifier or enable flag would not
establish ownership. Reconstructing a send permit after restart would permit
redisclosure and duplicate generation. General document retrieval is not composed.

## Consequences

The opt-in worker supports a guarded runtime without installing an approval or
activating a real model. Private collection includes blinded presentation pairs;
two independent reviewers and the schema owner still supply the review command.
A missing qualification produces deterministic fallback. Historical default worker
behavior remains unchanged. Queries continue serving typed companion choices.

## Compatibility and migration

No database migration, historical receipt rewrite or dependency addition. Install
existing Research migrations through 0012 before collection/approval/use. Build the
helper before creating the v2 manifest/profile. Qualify/review that exact package
before invoking the owned worker. Historical replay remains supported, with an
additive shared-preparation verifier for runtime companions. Rollback stops owned
execution and retains immutable receipts and attempt/qualification history.

## Security and operations

The schema owner remains the trusted reviewer of local artifact ownership, license,
provenance and reviewer independence. Hashes do not prove those external facts.
Private reports are created exclusively with mode 0600; model diagnostics and
rejected proposal bytes are not retained. No private artifacts or real approval
are committed. Optional documents require a later separately authorized consumer;
this worker prepares only authenticated artifact/policy evidence.

## Validation

Preserve v1 qualification goldens; reject cross-version evidence. Rescore runtime
reviews and test revocation through PostgreSQL reservation/result fences. Compare
Rust/Python runtime identities, reject changed files and exercise helper recovery
without runtime files and failure cleanup. Run finite worker recovery, repository,
architecture and default verification. Real-model evidence remains pending PR33;
the broader owned-process crash/rebuild campaign remains PR34.
