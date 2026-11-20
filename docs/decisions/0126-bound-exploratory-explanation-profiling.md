# ADR-0126: Bound exploratory explanation profiling

- Status: Accepted
- Date: 2026-11-20
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Extend nonqualifying performance v3 with bounded numeric timings, closed finish labels, in-flight slot counters and optional thermal/power observations.
- Compare one baseline and one thread candidate on P01 with fresh owned processes, unchanged inputs/settings and a 20-minute total cap.
- Idle admission remains the default. Explicit exploratory mode may run under load, always marked provisional and without approval authority.

## Context

The 32k run removed P03's context rejection but larger cases still timed out at
180 seconds. Existing records could not separate prompt evaluation, decoding,
output exhaustion or thermal/power effects. The maintainer authorized profiling
and a single six-versus-four-thread comparison without downloads, new licensing
or any change to the production 30-second bound.

## Decision

Adopt the pinned llama.cpp completion timing and slot-counter schemas behind
numeric allowlists, using existing dependencies. A completed response may report
cache/prompt/generated token counts and prompt/generation milliseconds; malformed
optional timings stay unknown without invalidating an otherwise valid proposal.
Finish labels are only stop, length, content_filter or other. Provider values are
untrusted diagnostics and cannot authorize output or establish independent time.

Sample the owned loopback slot endpoint at most once per second with a 250 ms
absolute request budget and a 65,536-byte wire cap. Retain only single-slot counts
and processing state; discard payloads and all unknown fields. Missing counters
are null, not zero. Polling adds overhead and does not measure time to first token.
Positive decoded counts establish observed decoding; zero or absent counts alone
do not prove prompt processing. Snapshot optional throttle counters and RAPL
energy/limits alongside existing frequency, governor, thermal and host-load data.
Never change these controls or infer throttling from temperature alone.

Add a separate comparison journal: one P01 baseline attempt, then one P01 candidate
attempt. All manifest semantics except thread count and derived deployment/profile
identities must match. Require explicit pair-digest execution consent. Both profiles
share the same pinned helper, evidence, weights/runtime, context, prompt and settings.
The 180-second observation cap remains; a 20-minute whole-process timer also covers
preflight, idle admission, startup and cleanup. Default idle waiting is bounded to
ten minutes; expiry stops without launching. Explicit exploratory mode skips only
idle admission and records provisional evidence. Never retry or select a winner.

## Alternatives considered

Full provider logs or generated text would violate payload-free diagnostics.
Timing only completed calls leaves timeout progress unknown. Grids, changed models,
GPU integration and increased deadlines broaden this first controlled comparison;
these remain separate work. Silent busy-host admission or reusing frozen grading
inputs would change evidence meaning and are rejected.

## Consequences

The comparison is a small diagnostic experiment, not qualification, an SLA or a
statistically controlled speed ranking. All failures and missing/interfered samples
remain visible. Default tests use synthetic local processes and need no idle CPU,
weights, paid provider or private evidence. A real exploratory pair may run under
load; a later idle run is needed for trustworthy performance conclusions.

## Compatibility and migration

Use new performance v3 and attempt v3 kinds, retaining historical v1/v2 bytes and
semantics. Comparison has its own v1 kind and is never a qualification collection.
Rebuild/pin the helper and changed owned launcher together; create new private
profiles/manifests/pair pins and exclusive reports. Production profile schemas,
30,000 ms deadline, HTTP request/validation, frozen grading inputs and owner grants
remain unchanged. The nine-attempt performance mode retains its idle gate.

## Security and operations

All requests target the parent's owned loopback runtime; model text, prompts,
provider errors and arbitrary headers never enter reports. Bounded malformed or
unavailable diagnostic replies become unknown. Owned cleanup runs before another
attempt; a budget interrupt during shutdown is reaped before propagating. Bounded
cleanup can follow execution-budget expiry, which cannot be swallowed as a network error. Optional
sysfs reads are read-only and permission failures stay unknown. Source digests
bind instrumentation; no approval, activation, download or automatic tuning occurs.
Rollback removes the new comparator without modifying stored owner state.

## Validation

TDD covers numeric-only completion extraction, malformed optional metrics without
acceptance changes, slot payload removal, absolute/byte bounds, optional counters,
one-variable pair admission, explicit consent/exploratory mode, preserved failures
and stop-on-budget behavior. Existing owned-process tests retain real cleanup
coverage. The approved real pair is six versus four threads with the same 32k
Qwen bundle; every under-load outcome must be labeled exploratory.
