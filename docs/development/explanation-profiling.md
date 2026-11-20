# Explanation profiling and one bounded candidate

## TL;DR

- Build/test profiling on a busy CPU; no real model is needed for default checks.
- Optional real measurements retain numeric timings, progress, finish labels and thermal/power observations, never payloads.
- The comparator runs exactly one P01 per profile. Idle is the default; `--exploratory` explicitly permits provisional under-load measurements.
- No result changes the 30-second production limit, qualification or approval.

## Observations

[ADR-0126](../decisions/0126-bound-exploratory-explanation-profiling.md) extends
[the capacity probe](explanation-performance.md) with performance/attempt v3.
The adapter reuses the production exchange and acceptance path. Optional
`response.timings` admits only `cache_n`, `prompt_n`, `predicted_n`, `prompt_ms`
and `predicted_ms`. `response.finish_reason` admits stop, length, content_filter
or other. Malformed/missing timings stay null without changing proposal validity.
A length stop still fails existing proposal acceptance; diagnostics do not grant
permission. Counts and times are reported by the runtime, not independently
verified facts. Nonstreaming calls do not measure time to first token.

The parent samples the owned loopback `/slots` endpoint once per second with an
absolute 250 ms deadline and 65,536-byte wire cap. Only slot zero's processing
state and prompt/processed/cache/decoded counts survive. Unknown fields and raw
replies are discarded. A positive decoded count shows decoding was observed;
zero alone does not identify the bottleneck. Unavailable polling remains null.
Polling adds overhead, so compare profiles with the same instrumented sources.
The reference is the [pinned llama.cpp API](https://github.com/ggml-org/llama.cpp/blob/9d25e0014c4b4b316873c01f4938c0e3c687287d/tools/server/README.md)
and its [slot implementation](https://github.com/ggml-org/llama.cpp/blob/9d25e0014c4b4b316873c01f4938c0e3c687287d/tools/server/server-context.cpp).

Host snapshots retain existing external CPU estimates, frequency/governor/thermal
values, plus optional per-CPU throttle counters and RAPL energy/range/power limits.
Missing or permission-denied values are unknown. Throttle arrays follow lexical
`cpuN` directory order; compare corresponding entries across snapshots. Package
counters can be duplicated across CPUs and must not be summed as
independent events. Energy counters can wrap at their reported range. No governor,
affinity, power limit or unrelated process is modified. These observations do not
establish causality or certify an uncontended host.

## Prepare exactly one pair

Build the release helper as documented in the capacity guide, then create NEW
private packages with matching v3 probe/profiling/owned-launcher sources. Rebind
the rebuilt helper and launcher into each canonical deployment/profile/manifest.
Never edit historical packages. The maintainer's first pair uses six baseline
threads versus four candidate threads, with the same Qwen weights, llama.cpp
runtime, 32k context, prompt, seed, temperature, 1,024 output tokens and 30,000 ms
canonical timeout. No new weights, license or dependency is required.

Create an exclusive private JSON pair manifest with exactly:

```json
{"kind":"research.explanation-profile-comparison.v1","baseline":{"path":"/absolute/private/baseline.json","sha256":"BASELINE_MANIFEST_SHA256"},"candidate":{"path":"/absolute/private/candidate.json","sha256":"CANDIDATE_MANIFEST_SHA256"}}
```

Pin its actual bytes. Replace the following placeholders before running:

```text
python3 scripts/compare_explanation_profiles.py --pair /absolute/private/pair.json --pair-sha256 PAIR_SHA256 --execute-pair PAIR_SHA256 --report /absolute/private/new-comparison.jsonl
```

Default admission requires five consecutive idle minutes and waits at most ten
minutes. Add `--exploratory` only to explicitly skip that admission and retain
provisional under-load observations. Every run has a 20-minute whole-process
budget, including preflight/startup, and the existing 180-second per-request cap.
Budget expiry aborts work and bounded owned cleanup follows, including when
expiry interrupts shutdown. A supervisor/identity/cleanup failure stops before any next
request. Reports must be new mode-0600 files outside Git. Omitting matching
execution consent launches nothing. Source digests, pair and per-profile pins,
input identities, host conditions and all outcomes are retained.

Order is fixed: P01 baseline once, then P01 candidate once, with fresh owned
runtimes. This is a narrow diagnostic sanity comparison, not a sweep, warm-KV
experiment or statistical ranking. No automatic retry or winner selection occurs.
Exit zero means two observations collected, even if they are failures/timeouts;
it never means the latency target or qualification passed. Review status and
validity before interpreting timing. A later idle repetition is necessary before
performance conclusions; qualification still requires its separate frozen suite,
independent reviewers and owner authority.

## Verification

```text
python3 -m unittest discover -s tests -p test_explanation_profiling.py
python3 -m unittest discover -s tests -p test_explanation_performance.py
cargo xtask verify --check explanation-performance-probe --check explanation-performance-lint --python python3
```

Synthetic loopback and process tests run without an idle host or real weights.
Use `cargo xtask verify --python python3` for the complete required default gate.
No optional real trial, paid judging, approval or activation is implied by tests.
