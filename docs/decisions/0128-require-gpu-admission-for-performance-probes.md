# ADR-0128: Require GPU admission for performance probes

- Status: Accepted
- Date: 2026-11-20
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- CUDA performance attempts require five continuous minutes of joint CPU/GPU admission and explicit prelaunch VRAM headroom.
- Retain bounded numeric GPU telemetry; unknown observations or foreign compute clients contaminate a request and stop later attempts.
- Preserve CPU behavior, historical evidence, production deadlines and qualification authority.

## Context

ADR-0127 permits a separately pinned CUDA candidate. The existing performance
probe admits CPU/memory/I/O idleness but cannot establish GPU idleness or memory
headroom. The maintainer explicitly approved this measurement prerequisite before
another idle campaign. Health-only memory fit and the busy-host exploratory
request remain separate historical observations, not qualification.

## Decision

Before each CUDA attempt, retain the existing CPU admission requirements and
require the same five-minute continuous window for the selected physical GPU:
at most 5% GPU utilization, at most 5% memory utilization, no compute clients,
and free VRAM at least an explicit operator-supplied MiB requirement. Missing or
invalid headroom refuses the run. CPU-only behavior is unchanged. No process is
stopped and no driver setting is changed to make admission pass.

Adapt NVIDIA's read-only `nvidia-smi` selective-query interface without adding
NVML bindings or redistributing vendor code. Pin its observed executable digest
and recheck it during sampling. Select the manifest UUID and independently
verify kernel device/driver identity. Each query has a 500 ms timeout, clean
environment and 4 KiB accepted output limit; parse only the closed numeric shape.
Unknown/unsupported/changed output fails closed. NVIDIA recommends NVML for
cross-version stability; this narrow CLI adaptation deliberately refuses format
drift rather than adding another runtime dependency.

## Alternatives considered

NVML bindings provide a more stable API but add a dependency and packaging work
for this small local tool. Keeping CPU-only admission would leave GPU contention
unverified. The bounded installed CLI is suitable here when unsupported output
refuses admission rather than silently weakening it.

## Consequences

CUDA reports use `research.explanation-performance.v4`; CPU reports retain v3.
Record policy/source/tool digests and utilization, total/free MiB and foreign
compute count, never UUIDs, process identifiers/names or raw diagnostic output.
During owned inference, exclude only its model PID from compute-client counts;
its high GPU utilization and VRAM allocation are expected, not contamination.
Unknown observations, no observations, or a foreign compute client mark the
retained attempt contaminated and terminate the remaining census. Preserve the
attempt and all earlier records; never retry selectively. New admission is
required after cleanup before every later attempt.

The headroom requirement is explicit because weights alone cannot predict KV,
workspace and request-buffer memory. For the retained T1200 candidate, 3,656 MiB
is an initial operator choice: 3,528 MiB startup plus 128 MiB reserve. It does not
prove maximum-input memory fit. Admission cannot reserve VRAM or prevent races.
Aggregate utilization cannot attribute graphics contention during owned compute,
and sampling can miss short-lived clients. Record those limits; never claim
complete interference detection or qualification from a clean sample.

The existing whole-run budget, fixed nine-attempt census and 180-second
nonqualifying observation ceiling remain unchanged. Repeated admission consumes
that budget; expiry retains an incomplete census. Production remains 30,000 ms.
The thread-comparison runner is outside this increment and has no GPU admission
claim. GPU scheduling, full measurement campaigns, paid judging and activation
remain separate work on #508.

## Compatibility and migration

Existing CPU reports and retained tool snapshots remain unchanged. New CUDA
campaigns require the new tooling and explicit VRAM argument. Revert by ceasing
GPU-v4 campaigns while preserving reports; no owner state or approval migrates.

## Security and operations

Use only read-only queries and the manifest-selected identity. Numeric reports
contain no UUIDs, PIDs, process names or raw errors. Tool drift and unknown fields
refuse admission. Sampling adds bounded overhead and cannot guarantee exclusive
GPU use or attribute all graphics contention during owned inference.

## Validation

Synthetic tests cover parser bounds, unknown/device drift, headroom, joint-window
reset, repeated admission, owned-client exclusion, missing observations and
contamination stop/retention. A single read-only hardware sample verifies the
installed interface; no inference is needed. Reverting this tool increment
requires ceasing GPU-v4 campaigns; preserve their reports. No owner state,
profile schema, model artifacts or approval is migrated.

References:

- [NVIDIA selective queries and API stability guidance](https://docs.nvidia.com/deploy/nvidia-smi/index.html)
- [Owned CUDA candidate](../development/explanation-cuda.md)
