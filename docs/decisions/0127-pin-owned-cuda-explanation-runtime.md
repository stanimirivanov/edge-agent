# ADR-0127: Pin an owned CUDA explanation runtime

- Status: Accepted
- Date: 2026-11-20
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Add an explicit owned CUDA manifest v3 as the approved prerequisite to M06-PR33; preserve CPU v2 and historical evidence.
- Pin device/driver, offloaded layers, cache precision, runtime/libraries/helper and source identities; verify physical/process observations before disclosure.
- Keep the 30-second deadline, frozen suite and approval rules. GPU readiness is not qualification or production activation.

## Context

The CPU candidate exceeded 30 seconds even for P01, with prompt evaluation
contributing most of the measured time. The workstation has a T1200 Laptop GPU
with 4 GiB VRAM and a working NVIDIA driver. The maintainer approved a GPU-runtime
prerequisite that reuses the existing Qwen weights rather than downloading a new
model. ADR-0111 describes CPU execution only and remains Proposed; its operator
review and the full qualification/review campaign remain tracked by issue #508.

## Decision

Retain the exact CPU v2 field set and command. Owned v3 additionally requires a
closed CUDA descriptor: Linux PCI address, GPU UUID, loaded-driver version digest,
1–64 explicitly offloaded layers and key/value cache precision (f16, q8_0 or q4_0).
No missing/null descriptor, CPU-v2 CUDA field or unsupported value is admitted.
Bind the descriptor and CUDA support source digest into a distinct deployment
identity; retain all existing file/hardware/profile bindings. Partial offload is
explicit, not a claim of GPU-only execution. Cache quantization changes execution
semantics and requires fresh qualification of the exact profile.

Select the GPU UUID through a clean CUDA environment and PCI device ordering.
Use explicit CUDA0, positive layer count, flash attention, cache types and disabled
automatic fitting. Read bounded kernel observations for the physical GPU/driver;
independently check the runtime environment and its actual NVIDIA device FD.
Both the parent and helper refuse identity drift. Extend the existing deployment
verifier to repeat these checks before model disclosure and after completion.
A model response cannot establish GPU identity. Existing PID/start/executable,
exact-argument, parent/listener and cleanup fences remain mandatory.

CUDA v3 permits individually pinned libraries up to 2 GiB because the official
CUDA bundle contains a roughly 717 MiB cuBLASLt library. CPU v2 retains its 512 MiB
limit; both retain at most 64 library pins and streaming verification. No CUDA
SDK package, Rust GPU dependency, driver or container/GitOps deployment is added.
Private official runtime archives, vendor licenses and actual library inventories
remain operator-reviewed artifacts outside Git.

## Alternatives considered

Changing CPU flags without a new identity would misrepresent the qualified
runtime. Trusting an API's device/model label or accepting arbitrary launcher
arguments would weaken ownership. Automatic fitting, CPU fallback and an unpinned
cache precision would obscure what execution was measured. New model weights,
driver installation and production GPU scheduling broaden this prerequisite and
are deferred.

## Consequences

The existing model can be investigated on available hardware without changing
market facts or qualification thresholds. Memory fit and performance are separate
observations: a health-only 32k/32-layer/q4 cache startup used 3,528 MiB of the
4,096 MiB GPU. Display/other workloads reduce headroom, and larger requests may
still fail or miss the deadline. Neither startup nor a capacity probe qualifies
the profile. No silently adjusted configuration or selective rerun is permitted.

## Compatibility and migration

CPU-v2 packages remain executable with their retained matching helper/supervisor
sources. New helper/source bytes always need fresh pins; never rewrite old reports.
GPU v3 requires new deployment/profile/manifest and any owner disclosure reviews
and qualification approval. Profile schemas and frozen input/collection protocols
remain unchanged. Historical CPU diagnostic replay remains CPU-only rather than
retuning its evidence. Receipt recovery still starts without a new model request.

## Security and operations

The operator trust profile includes kernel procfs, installed driver and reviewed
files. Never log real GPU UUIDs, private paths, prompts or model/error payloads.
Model/library pins are checked before and after owned execution. CUDA runtime
components have NVIDIA terms in addition to llama.cpp MIT and existing model
materials; retain and review those separately, without inventing rights or owner
attestations. Reports stay exclusive/private and grant no authority. Rollback
removes GPU-v3 use without changing owner state, CPU records or approved profiles.

## Validation

TDD covers version separation, null/unknown/cache/path refusal, exact commands,
GPU identity binding, driver/device/environment/FD drift and unchanged CPU tests.
Synthetic tests require no GPU. A separately retained hardware health/memory check
uses the publisher-pinned runtime and existing weights, then reaps it. Full frozen
qualification, independent hosted reviews, owner approval and activation remain
outside this prerequisite and #508 remains open.
