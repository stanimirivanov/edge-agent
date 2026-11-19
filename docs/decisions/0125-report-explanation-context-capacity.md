# ADR-0125: Report explanation context capacity

- Status: Accepted
- Date: 2026-11-19
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Distinguish a bounded upstream context-capacity rejection in nonqualifying performance v2 reports without exporting provider messages.
- Preserve production failure behavior, the 30-second profile ceiling, full evidence inputs and historical measurements.
- A larger context requires a new explicitly reviewed private deployment/profile; byte limits do not establish token capacity.

## Context

The P03 twelve-instrument calibration contains 39,218 prepared bytes. A separate
bounded diagnostic using the original pinned Qwen/llama.cpp deployment reproduced
HTTP 400 in 13 ms: the runtime reported 24,762 prompt tokens against a 16,384-token
context. The input digest matched all three historical P03 measurements. The
byte-bounded application request is valid, but exceeds this deployment's token
capacity. Generic `invalid_response` concealed that distinction.

## Decision

Add HTTP status and an optional context-capacity pair to the separate performance
measurement envelope. Admit the pair only from an unencoded bounded JSON HTTP 400
body with exact `exceed_context_size_error` type, code 400, positive u32 context
and a larger u32 prompt count. Discard all other fields, especially messages.
These untrusted provider counts are diagnostics, never evidence or authority.
Only an opted-in performance call reads the error body, under its existing cap
and the same 65,536-byte stream bound as successful responses. Ordinary production
calls continue to reject non-success headers without waiting for an error body.
No body/header/schema rejection is bypassed; the outcome remains invalid response.

Use new `research.explanation-performance.v2` and
`research.explanation-performance.attempt.v2` kinds. The importer checks the closed
numeric shape and context/status consistency, rejecting payload-bearing records,
legacy kinds and malformed counts. Profiles and grading/qualification contracts
are unchanged. No automatic context increase, evidence truncation or retry occurs.

## Alternatives considered

Reading and logging raw errors would expose prompts or provider-controlled text.
Labeling every HTTP 400 as context exhaustion would misdiagnose other failures.
Silently truncating evidence or raising context would change the input/profile and
misrepresent historical observations. Parsing error bodies on production calls
would unnecessarily delay fallback. These approaches are rejected.

## Consequences

A future probe distinguishes an observed context shortfall from a generic HTTP
rejection without manufacturing a successful completion. Absent counts remain
unknown. Missing/invalid diagnostics do not weaken rejection. Full local capacity
and latency still require separately identified measurements; a larger context
alone cannot demonstrate acceptable response latency or successful qualification.

## Compatibility and migration

Historical private packages and v1 reports remain immutable and interpreted as
v1. New measurements require a rebuilt/pinned helper, matching v2 probe source,
new deployment identity, canonically rebound profile/manifest and exclusive report
paths. Do not import v1 reports into v2 or reinterpret them as qualification.
The production 30,000 ms deadline and 180-second performance cap remain unchanged.

## Security and operations

Never retain provider bodies, messages, private prompts, tokens or generated text.
Both known-length and streamed bodies are bounded. A stalled diagnostic body can
consume only the existing observation cap; ordinary fallback timing is unchanged.
Exact deployment/listener checks, cancellation, ownership and cleanup still apply.
Rollback restores the old probe/helper without modifying retained state or reports.

## Validation

A regression test first reproduced missing P03 capacity metadata, then passed
with the fix while checking exact input preservation and no provider text export.
Loopback tests cover malformed/oversized errors and production rejection without
waiting for an error body. Python tests cover the closed v2 shape, invalid counts,
extra fields, historical-kind rejection and process cleanup. No hosted judging,
qualification approval, deadline change or activation is part of this fix.
