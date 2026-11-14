# ADR-0122: Retain payload-free explanation diagnostic replays

- Status: Accepted
- Date: 2026-11-14
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Explicitly requested local diagnosis may retain fixed application-owned failure labels, transport phases and elapsed milliseconds.
- Preserve the failed qualification and its settings; diagnostics cannot become qualification evidence.
- Never retain raw model/server diagnostics, prompts, response bodies or credentials.

## Context

The first PR33 census retained 34 `failed` observations without their typed causes.
The maintainer explicitly requested safe diagnostics before a fresh trial.
ADR-0111 calls for safe failure categories; ADR-0120 forbids retaining model
diagnostics and rejected bytes. This decision permits narrowly defined,
application-authored diagnostic records for the separate replay below. It does
not authorize collecting raw runtime/provider logs or model text.

## Decision

Add an opt-in `--diagnose` path to the existing owned supervisor. Bind it to the
exact original collection and manifest. Freeze weights, runtime, libraries,
license, prompt, hardware, threads, context, sampling settings, output limit,
deadline, validator, renderer and tools. Only the instrumented helper/launcher
identity and its corresponding profile identity may change.

Run the complete fixed 30-reader/four-probe census once. Write an exclusively
created mode-0600 JSONL report with source/manifest/profile/suite/preparation
digests, fixed input IDs/hashes, attempt-start and attempt-finish markers, closed
failure/status and transport-stage enums, monotonic elapsed milliseconds, and a
completion marker. Flush records before continuing, so an interrupted replay
does not invent outcomes for unfinished attempts. Raw errors, response content,
prompts, paths and process/server logs are excluded from this report.

Use a distinct `research.explanation-runtime.diagnostics.v1` record format and
diagnostic completion envelope, both granting no permission. The hosted-review
preparer must refuse these formats. A replay can establish a reproducible cause;
it cannot retroactively recover missing error categories from the original trial.
No selective retry, budget increase, prompt tuning, cloud call, approval or
activation is part of diagnosis. A fresh qualification remains a separate review.

## Alternatives considered

Raw stderr/HTTP bodies could disclose protected content. Guessing from duration
cannot distinguish timeout from framing or identity failure. Rewriting the
original collection would erase the boundary between observed and inferred facts.

## Consequences

Diagnosis becomes auditable without producing ratings or accepted proposals.
Closed labels may localize a failure without revealing its full underlying cause;
further controlled investigation must remain separately recorded.

## Compatibility and migration

Ordinary v2 collection and worker receipt formats remain unchanged. Clear inherited
diagnostic flags in the supervisor; only explicit diagnostic collection may set
them. Rebuild and pin the helper/launcher together, preserving all historical
packages. No database migration is required. Rollback stops diagnosis and retains
its incomplete or complete private report.

## Security and operations

Use only the existing invented local corpus and owned CPU runtime. Preserve the
existing process/listener/file checks and cleanup. The journal is local debug
evidence, not a rights grant or a model's claim of authority. Existing approval,
disclosure, qualification and maximum-profile bounds remain in force.

## Validation

Test closed status/phase serialization, source/configuration binding, private
exclusive creation, flushed partial records, inherited-flag isolation, cleanup,
actual loopback timeout phases and refusal by the qualification preparer. Run the
repository default gate. Report diagnostic replay separately from model
qualification; neither software tests nor a diagnosis qualify the candidate.
