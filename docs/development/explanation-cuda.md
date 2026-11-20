# Owned CUDA explanation candidate

## TL;DR

- CPU v2 remains unchanged. GPU v3 is a separate explicitly pinned local candidate, not an approved production deployment.
- Reuse the existing weights and model port; verify GPU/driver, exact arguments/environment, device FD and existing process/listener fences before disclosure.
- Keep 32k context, 30,000 ms production deadline and frozen qualification criteria. Fit, latency and usefulness each need their own evidence.

## Contract

[ADR-0127](../decisions/0127-pin-owned-cuda-explanation-runtime.md) records the
maintainer-approved prerequisite to M06-PR33 (#508). The owned supervisor accepts
v2's exact CPU field set or v3's exact set plus a required `cuda` descriptor:

```json
{"pci":"0000:01:00.0","uuid":"GPU-11111111-1111-1111-1111-111111111111","driver":"ACTUAL_DRIVER_VERSION_FILE_SHA256","layers":32,"cache":"q4_0"}
```

This is an invented shape example, not an executable profile. Obtain the actual
PCI address, UUID and device minor from `/proc/driver/nvidia/gpus/PCI/information`
and pin the bytes of `/proc/driver/nvidia/version`. Only 1–64 positive offloaded
layers and f16/q8_0/q4_0 cache types are supported. Both key/value types match.
Explicit null or unknown fields do not select a default. CPU v2 cannot carry CUDA.

The model process receives only the existing clean environment plus exact
`CUDA_VISIBLE_DEVICES=UUID` and `CUDA_DEVICE_ORDER=PCI_BUS_ID`. The arguments add
`--fit off`, CUDA0, explicit offload count, flash attention on and both cache types.
Remaining CPU layers are intentional. The supervisor checks the physical identity
and actual process device FD before handing over its observation; the helper
independently checks these and the environment through the manifest-built verifier.
Driver, selection, process or listener drift refuses disclosure. Unsupported or
out-of-memory startup is a failure, never permission for automatic CPU fallback.
The reference is the [pinned llama.cpp server](https://github.com/ggml-org/llama.cpp/blob/9d25e0014c4b4b316873c01f4938c0e3c687287d/tools/server/README.md).

## Prepare private artifacts

Retain original packages and reports. Obtain publisher-pinned runtime archives
and CUDA runtime-library material into a new directory outside Git; verify hashes
before extraction/execution. The current candidate uses official llama.cpp
b12375 CUDA 12.8 x64 archives and the existing Qwen3-4B Q4_K_M weights. No system
driver/toolkit installation is required. CUDA components have [NVIDIA terms](https://docs.nvidia.com/cuda/archive/12.8.0/eula/index.html)
separate from llama.cpp MIT and the retained model license declarations. Operator
rights/library review and provenance attestations are not inferred by this tool.
Do not commit or redistribute the bundles as repository artifacts.

Build and pin matching owned helper/supervisor/CUDA-support sources. Inventory
actual loaded and bundled runtime libraries, including installed driver libraries;
resolve reviewed file pins and verify them before/after execution. At most 64
pins are admitted. CPU libraries retain a 512 MiB per-file bound; CUDA v3 allows
2 GiB for the vendor's larger cuBLASLt binary. Hashing remains streamed/bounded.

Generate the distinct `owned-explanation-cuda-v3` identity with
`run_explanation_owned.runtime_id(manifest)`; it additionally binds the CUDA
descriptor and support-source digest. Re-encode the canonical profile and manifest
and pin their bytes. A performance example and a qualification service helper
have different binary hashes and need separate profiles/manifests; never treat a
performance report as qualification. Keep all request settings, disclosure/grant
requirements and the frozen suite unchanged.

## Fit and qualification

First verify a bounded health-only startup and actual owned-process GPU memory,
then reap it. The local T1200's 4 GiB memory is constrained: the recorded 32k,
32-layer, q4 cache startup consumed 3,528 MiB. Other workloads and request buffers
can exhaust remaining headroom. Quantized KV caches change model execution, even
when weights and request settings are unchanged, so the exact profile requires
fresh frozen evaluation. A successful startup proves neither valid generation,
maximum-input memory fit, latency nor usefulness.

One exploratory invented P01 request through the owned CUDA supervisor/helper
returned a valid proposal in 76.3 seconds (60.4 prompt, 15.9 generation), with
cleanup complete. Repository builds ran concurrently; this is neither an idle
comparison nor a passing result against the unchanged 30-second production limit.
The private performance envelope retains the exact candidate identity.

Use the existing [capacity/profiling tools](explanation-profiling.md) only with
fresh matching packages and explicit opt-in. Busy-host observations are
exploratory. A full PR33 campaign still needs reviewed artifacts, an idle-host
measurement, all 30 reader cases and four adversarial probes, two independent
hosted reviews of an eligible collection, independent rescoring and owner
attestation. Issue #508 remains open. No approval or activation is installed by
this prerequisite; GPU OCI/GitOps scheduling is outside its scope.

## Verification

```text
python3 -m unittest discover -s tests -p test_cuda_explanation.py
cargo test --locked -p edgeagent-research --test owned_manifest
cargo test --locked -p edgeagent-research --lib explanation_cuda
cargo xtask verify --python python3
```

Default tests use synthetic hardware facts and require no CUDA device or weights.
Report actual optional hardware checks separately. Old CPU replay settings remain
frozen, and failed or incomplete GPU reports cannot enable generation.
