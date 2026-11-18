# Bounded end-to-end explanation performance probe

## TL;DR

- This separate opt-in Linux probe observes complete local responses for up to 180 seconds, without changing the production 30-second ceiling.
- It uses the production preparation, identity-checked HTTP exchange, schema/proposal validation and deterministic renderer. No proposal/text leaves the helper.
- It runs three invented sizes, three times each, with a fresh owned model process for every attempt. All outcomes are retained; cleanup completes before the next attempt.
- A completed census is not a passing qualification. No grading tasks, hosted judges, approval, publication or activation participate.

## Prepare a new private package

First use the [idle CPU benchmark](explanation-cpu-benchmark.md) to inspect the
selected host/thread profile. This feature measures the application path rather
than synthetic tokens/second. Neither implementation automatically runs a model.
[ADR-0124](../decisions/0124-measure-bounded-explanation-performance.md) owns the
measurement protocol; the [explanation guide](explanations.md) owns existing
canonical profile and private manifest preparation.

Build the dedicated helper from the repository root:

```text
cargo build --locked -p edgeagent-research --features performance-probe --example probe_explanation
```

The executable is `target/debug/examples/probe_explanation`. Match the intended
deployment's build profile when interpreting latency: for release measurements,
add `--release` and use the resulting `target/release/examples/probe_explanation`.
That produces a different helper pin; do not mix build profiles in a comparison.
The ordinary Research
binary has no performance command and cannot replace this helper. The feature is
not enabled by default or added to production images.

Create a NEW private v2 manifest/profile package outside Git. Reuse the reviewed
local weights, runtime, license, prompt, libraries, hardware, threads and context
pins; do not edit any historical package. Review local-use rights before running.
Set `helper.path` to the absolute dedicated executable and `helper.sha256` to its
actual hash. Use the existing owned-launcher deployment identity algorithm
`run_explanation_owned.runtime_id(manifest)` with that helper pin. Re-encode the
profile canonically, update its file digest in the manifest, then pin the final
manifest bytes. The profile still uses the existing <=30,000 ms timeout bound and
current runtime validator/renderer revisions. The longer measurement ceiling is
not encoded into that profile, and no approval is created for it.

The owned-launcher source changed to admit only this dedicated helper for the
three performance cases, so its hash participates in the new deployment identity.
Rebuild/pin helper and launcher sources together; do not reinterpret old identities.
The new journal independently records its performance-launcher source digest and
binds the exact manifest, profile and actual prepared calibration bytes.

Run manually after replacing these path/digest placeholders:

```text
python3 scripts/probe_explanation_performance.py --manifest /absolute/private/performance-manifest.json --manifest-sha256 <manifest-sha256> --execute-manifest <manifest-sha256> --report /absolute/private/new-performance.jsonl
```

Reports must be outside every Git checkout and exclusively created mode 0600.
Omitting consent records not-run and launches nothing. Stderr from owned helpers
and the model is discarded. Only the fixed numeric/enum measurement envelope is
admitted from helper stdout. Model text, prompts, rendered presentations, raw
responses/errors and credentials are not retained.

## Census, timing and host conditions

The fixed order is P01/P02/P03, three repetitions each: one, four and twelve
instruments. These are independent invented capacity inputs with new request
names/dates, prepared through the same production functions. They are not a new
frozen usefulness suite, token-count estimate or benchmark of maximum context.
No request/task/rating in the grading census changes.

Use the same five-minute idle gate and host telemetry requirements as the CPU
benchmark. Admission occurs once before the census. During each attempt retain
numeric CPU, pressure, AC, swap, memory, frequency/thermal availability and an
estimate of external CPU (host busy ticks minus owned supervisor/helper/model
ticks). The first estimate or unavailable accounting is null, not zero. Inspect
these records for interference; completion does not certify idle conditions
throughout generation. High CPU from the model itself is expected.

Each attempt starts a fresh server and runs one nonstreaming request. The server
is stopped/reaped before its finish record and any next attempt. OS file caches
can be warm; server warmup and thermal drift remain visible operational factors.
There is no shared-runtime/KV warm-call comparison in this protocol.

`measurement.elapsed_ms` includes deployment acquisition, exact production
HTTP framing, response parsing, typed proposal validation and deterministic
rendering (including the calibration's reconstruction). `total_ms` also includes
calibration preparation. `orchestration_ms` additionally includes supervisor
startup, file checks and cleanup; it is not a pure model-startup metric.
Nonstreaming calls do not measure time-to-first-token; token counts and internal
prefill/decode timings are not collected by this adapter.

The independent observation cap is 180 seconds. The 190-second post-startup
supervisor bound includes preparation/reporting overhead. Existing readiness and
control bounds remain 120 and 90 seconds. A 90-minute total timer includes idle
waiting and all attempts; budget expiry aborts with cleanup. No cap auto-extends.
A supervisor, identity or cleanup failure stops the census rather than starting
another request with uncertain ownership. No selective or automatic retries occur.

Exit 0 means nine measurements collected, even if all are timeouts or invalid.
Exit 2 means not-run or incomplete. Inspect every attempt's closed status and
stage. A missing final marker is incomplete. Timeout elapsed values are censored
lower bounds, not completion latencies. Small samples do not establish p95/p99
or justify a production deadline on their own. Keep all failed/interfered blocks
and request a separately identified repeat if needed.

## Verification and next decision

Default verification includes the feature-enabled model/calibration/example tests
and Clippy check. Focused commands are:

```text
cargo xtask verify --check explanation-performance-probe --check explanation-performance-lint --python python3
python3 -m unittest discover -s tests -p test_explanation_performance.py
```

Tests use synthetic loopback servers and disposable helper processes; no weights,
owner service, paid provider or hosted judge is needed. The performance report
kind is structurally different from qualification collections and grants no rights.
[ADR-0122](../decisions/0122-retain-payload-free-explanation-diagnostic-replays.md)
diagnostic replays retain their original settings and cannot use this protocol's
ceiling. After a real idle-host run, review valid completion latency, interference,
output truncation/refusal and headroom before proposing a separate production
budget/profile change or fresh M06-PR33 qualification.
