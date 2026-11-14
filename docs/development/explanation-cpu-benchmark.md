# Idle-gated explanation CPU benchmark

## TL;DR

- This opt-in Linux tool measures synthetic local CPU throughput, not explanation quality, HTTP latency or qualification.
- Retain the original candidate and reports. Production's 30-second deadline stays unchanged.
- Prepare a separate pinned manifest, then explicitly repeat its digest to execute. No model runs during ordinary tests or CI.
- Wait for five consecutive idle minutes before each thread block; preserve all measurements, including interference and failures.

## Preparation

Use the already installed official `llama-bench` from the same pinned llama.cpp
release as the candidate runtime. No downloads, packages or model selection occur
inside this tool. Retain the runtime/model license materials and review the local
execution rights before starting. Use only local weights, never real research data.

Create a private manifest outside Git with exactly this structure. The paths and
hashes below are placeholders; replace them with existing absolute regular files
and their actual SHA-256 digests. Include every required runtime shared library in
`libraries`; the launcher sets only PATH, LANG and the pinned library directories.
Do not reuse or modify the qualification manifest.

```json
{
  "kind": "research.explanation-cpu-benchmark.v1",
  "bench": {"path": "/absolute/path/to/llama-bench", "sha256": "<64 hex characters>"},
  "weights": {"path": "/absolute/path/to/model.gguf", "sha256": "<64 hex characters>"},
  "libraries": [{"path": "/absolute/path/to/libggml.so", "sha256": "<64 hex characters>"}]
}
```

Choose a fresh report path outside every Git checkout. Reports are exclusively
created mode 0600 and flushed/fsynced per record; an existing report is never
replaced. Keep the manifest alongside the report so its digest remains resolvable.
The output is `research.explanation-cpu-benchmark.v1`, not an importable explanation
collection, diagnostic replay, assessment, approval or disclosure grant.

From the repository root, after replacing the placeholders:

```text
python3 scripts/benchmark_explanation_cpu.py --manifest /absolute/private/benchmark.json --manifest-sha256 <manifest-sha256> --report /absolute/private/new-report.jsonl --execute-manifest <manifest-sha256>
```

Omitting `--execute-manifest` records not-run and launches nothing. This command
is a manual operator action, not a scheduled job. Exit 0 means a complete clean
throughput census; 2 means not-run/incomplete/inconclusive; 130 means interrupted.
A missing terminal record means interrupted/incomplete, never a passing run.

## Idle gate and bounded execution

Before every block, require 300 consecutive seconds with aggregate busy CPU at
most 5%, every online logical CPU at most 10%, no swapping, at least weights-file
size plus 2 GiB available memory, AC power where a battery is exposed, and no more
than 10 ms of additional CPU/memory/I/O pressure stall time per one-second sample.
Counter resets and topology changes reset the idle window. Missing required Linux
telemetry fails closed. Desktop hosts without an exposed battery treat AC as not
applicable; operators must confirm the actual power source when running in a VM
or a system that hides laptop power telemetry.

Waiting is bounded to one hour per block within a two-hour total measurement
budget. Each child is bounded to 900 seconds or the remaining total budget.
Pins are checked before waiting, so hashing does not interrupt the accepted idle
window. Do not modify pinned artifacts while waiting/running. No unrelated work
is killed, no governor/priority/affinity is changed, and no cache is forcibly dropped.
The fixed 6/10/14/20-thread census requires at least 20 CPUs in the process affinity
mask; unsupported hosts report not-run rather than silently changing the census.

Each block uses CPU execution (`-ngl 0 -dev none`), prompt lengths 512 and 2048, generation
lengths 128 and 512, three repetitions and the tool's default warmup. It retains
all four prompt-only/generation-only rows and their individual throughput samples.
The default llama-bench batch/cache behavior applies; no 16k HTTP context claim
is made. File hashing and normal warmup can warm the OS/model caches, so these
measurements are not cold-start latency. Fixed thread order is retained in the
report; thermal drift can confound comparisons and should be inspected.

During work, high benchmark CPU usage is expected. Record host counters, available
memory, AC, swap, affinity/topology, governor, available frequencies/temperatures,
and estimated external busy CPU (host busy ticks minus owned process CPU ticks).
More than 5% aggregate external busy CPU, swapping, insufficient memory, loss of
AC, or unavailable process accounting marks a block contaminated. Unknown optional
frequency/thermal metadata appears as empty lists/null values. Do not infer idle
external cores or thermal safety from aggregate accounting alone.

Raw stderr is discarded. Temporary stdout is checked against a 1 MiB limit at
one-second intervals; only validated numeric throughput fields are retained.
Timeout, failure and interrupted execution kill/reap the owned process group.
There are no automatic retries or selective sample deletion. Interrupted or
contaminated runs remain evidence; a separately requested repeat uses a new report.

## Interpretation and verification

Compare individual samples, variation and extrema alongside load/thermal records.
A synthetic tokens-per-second improvement is not proof that nonstreaming HTTP
requests complete within 30 seconds: tokenization, sampling, structured decoding,
context depth and transport/validation costs require the separately planned
end-to-end probe. Small samples do not establish a production percentile SLA.
Do not change deadlines, prompt, output limit or qualification tasks from this
report alone. See the [retained diagnosis](explanation-qualification-m06-pr33.md)
and [frozen explanation protocol](explanations.md).

Deterministic tests require no model weights or external services:

```text
python3 -m unittest discover -s tests -p test_benchmark_explanation_cpu.py
```

The implementation adapts the existing private report and file-pin helpers. It
adds no dependency; [llama-bench](https://github.com/ggml-org/llama.cpp/tree/master/tools/llama-bench)
is part of the already selected MIT-licensed runtime distribution.
