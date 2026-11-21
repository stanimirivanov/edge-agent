# M06-PR33: First local qualification trial

## TL;DR

- On 2026-11-14, the owned runtime completed all 30 reader cases and four adversarial probes.
- Every observation was `failed`; all reader pairs contained identical fallback text.
- This trial cannot qualify. Hosted judging, independent rescoring and owner approval were not performed.
- A separate diagnostic replay reproduced 34 response-header timeouts at the maximum permitted deadline; a fresh qualification remains pending.

## Candidate and execution

The prepared candidate was Qwen3-4B-Instruct-2507, Unsloth Q4_K_M GGUF, through
official llama.cpp CPU build b12375, commit
`9d25e0014c4b4b316873c01f4938c0e3c687287d`. License/provenance materials and local
artifact pins remain private; their presence is not an owner rights attestation.
The release Research helper and frozen supervisor came from merged PR507,
commit `2fa7203`. The profile used six CPU threads, a 16384-token context, seed
42, temperature zero, 1024 maximum output tokens and a 30000 ms request deadline.

The first supervisor invocation used an incorrect manifest digest and refused
before spawning any child. Its empty output and separate refusal record were
preserved. One subsequent invocation used the verified pin and collected the
entire fixed census without selective model-case retries. The supervisor exited
successfully and reaped the owned runtime. The completed private collection is
319484 bytes; its envelope grants no permission.

| Binding | SHA-256 |
|---|---|
| Exact completed collection bytes | `4327adb76b4a9933b0050bf26d6a580d835afc32b29325e2072079d1bdf3a63e` |
| Exact v2 manifest bytes | `80c28cde8955a30f30deec0b8c1aa220e06c0eb05f0e3c1a2fa44895c4189331` |
| Exact profile bytes | `96ffa674b8466bd66210a7ce681ef717051eb98e727e37c4653a6f1d7ec3c84c` |
| Research helper executable | `c4d9d20121166c82e73f434717585ea3b90ceb66ef54c79f62b2ed46b02d59f4` |
| Frozen runtime suite | `b28ce7cb889d64e8b20f397c0b2a56f66ab3630128c8f5c888d49f527346d994` |
| Frozen runtime preparation | `3f9aa8d7bbc26fea7e1dda7de7e9e7a40bca080c9e27595346393ffb6530d1b2` |

## Observed result and limits

The collected case IDs/input identities match the frozen `usefulness-v3.tsv`
census. A separate local `verify-runtime` invocation also reconstructed all pair
texts and presentation identities from the retained typed observations using the
frozen renderer; it made no model call. All thirty reader observations and all four adversarial observations
have outcome `failed`, not `generated` or `fallback`. Their displayed reader
texts are the deterministic fallback, so all thirty blinded pairs are identical.
Failed observations earn no candidate preferences; the maximum possible eligible
total is zero, below the required 21 per reviewer. None of the four required
valid generated adversarial proposals was obtained.

The collector maps model-port errors to `failed` without retaining their typed
categories in this envelope. The adapter distinguishes timeout, availability,
deployment mismatch, refusal and invalid response, but this record cannot say
which occurred. Neither the 30-second setting nor the overall runtime proves a
timeout diagnosis. No rejected response or proposed speculative fix is published.

No hosted request, provider token-count request or paid judgment was made.
The two AI reviews, the 47-check qualification assessment, exact-profile owner
approval and production activation remain **not run**. Default repository tests
are separate software verification and cannot establish those missing results.
ADR-0111 remains Proposed; this negative collection does not resolve operator
review of runtime/model rights or authorize disclosure/activation.

## Initial continuation requirements

Keep issue #508 open. First retain bounded, payload-safe failure categories in
the owned collector under tests, preserving this trial unchanged. Diagnose the
failure before proposing a separately recorded candidate/profile trial; do not
silently increase budgets, retune prompts, replay selected cases or replace this
result with a more favorable run. The hosted runner refuses already-ineligible
collections before any external disclosure. When a fresh eligible collection
exists, follow [ADR-0121](../decisions/0121-define-hosted-ai-explanation-review.md),
obtain account/handling/quote acceptance, and retain both complete AI reviews
before independent rescoring and authenticated owner attestation.

## Diagnostic replay — 2026-11-14

The maintainer requested safe failure diagnostics on issue #508/PR #511. Under
[ADR-0122](../decisions/0122-retain-payload-free-explanation-diagnostic-replays.md),
a separate instrumented helper replayed the complete fixed census once. Its
launcher refused any change to the original runtime, weights, libraries, license,
prompt, hardware, thread/context settings, sampling, output limit, deadline,
validator, renderer or tools. Only the helper/launcher identity and corresponding
profile identity changed. All 34 ordered input identities matched the original
30 reader cases and four probes. The supervisor exited zero and cleaned up the
owned processes; the original collection's exact-byte digest remains unchanged.

Every replay attempt returned `timeout` at transport stage `waiting_response`.
Measured model-port durations ranged from 30000 to 30008 ms. This means request
serialization and deployment verification completed, but the HTTP send did not
return response headers before the deadline. No response reached parsing or
proposal validation. An independently bounded, read-only health observation
returned HTTP 200/`status: ok` while probe S01 remained pending; it sent no
additional inference request and retained no response body.

The reproduced immediate failure is deadline exhaustion while awaiting the local
non-streaming response, rather than deterministic proposal rejection. It does
not establish server-internal prefill, generation, queue or cancellation timing,
nor prove that a longer deadline would qualify the model. Missing historical
categories cannot be retroactively assigned to the first trial. Actual model
output/schema compatibility remains unproven because no response reached parsing.

| Diagnostic binding | SHA-256 |
|---|---|
| Complete private JSONL journal | `4f950fbcb28d94dc05b3589e68009304b3b71e578013eafba27a7a746b81a403` |
| Diagnostic completion envelope | `747efe487ed4d7431476788e3f2351052782e8938fb033c5bcb7c62cfda724da` |
| Diagnostic v2 manifest bytes | `dfa0fe0364cffe1377226ef49f80a4c42bc61ba5667b459180d2a90fbd07affd` |
| Instrumented helper executable | `9f5cf3dd4af91185d0ba107e05402963f1086358f6f0e927e77b792fff78bc05` |

## Remaining decision before fresh qualification

The 30000 ms value already equals the profile contract's maximum; it cannot be
raised through a manifest edit. Review a CPU runtime/model/resource configuration
that meets the existing bound, or make a separate versioned contract decision
before changing that bound. Preserve this diagnosis and both private packages.
No deadline, prompt, output budget, corpus, qualification threshold or production
setting was changed. No fresh qualification, hosted judgment, owner approval or
activation was performed. Diagnostic formats are rejected by hosted preparation;
the campaign and issue #508 remain incomplete.

## CUDA frozen census

The candidate from #546 uses 1,600 output tokens. This collection pins a fresh
`research.explanation.profile.v2` with a 600,000 ms per-request deadline and
keeps the exact model weights, runtime, prompt, library pins, CUDA configuration
and other inference settings from the feasibility package. The release
`edgeagent-research` collector and independent `qualify_explanation` verifier
were built from merged source `31962fc2d484960bfb03ad17a9d4d1b712bf8599`.
The canonical profile and deployment identity bind the new collector.

The manifest SHA-256 is
`104fcdf61b971cb4240147d73d0e0aa696ac4b834a61fa0ddbb253746a40f368`.
The profile SHA-256 is
`f4c6c7727d0f09e67af5768bafbef0391d9d63a42c6db21bc5658c61e92b0c7e`.
The private preparation record SHA-256 is
`12fd90ad589301d34eb099a6ccc64806880fb697cfa9f72e1016fc4025139804`.
The Python preflight and Rust binding handshake accepted the exact package.
The preflight helper was reaped before model startup.

The owned collector runs the existing `Suite::runtime()` census of 30 reader
cases and four adversarial probes. The collection stays private and carries
no permission grant. Earlier CPU and CUDA records remain available as separate
evidence.

The supervisor exited successfully after about 27 minutes. All 30 reader
cases and all four adversarial probes had outcome `generated`. The independent
local `verify-runtime` check validated every ordered case and probe identity,
rendered every generated proposal, and reconstructed all 30 presentation
bindings and paired texts. Cleanup left no GPU compute client.

All 30 generated reader presentations were byte-identical to the deterministic
baseline. Under [ADR-0121](../decisions/0121-define-hosted-ai-explanation-review.md),
identical presentations are ties. This leaves zero possible preferences under
the review rubric, below the required 21 per judge. The collection establishes
complete local generation and validation for the pinned profile. The usefulness
requirement remains unmet. No hosted token-count or judgment request was made,
and owner attestation, qualification approval and activation remain pending.
Issue #508 stays open.

The collection SHA-256 is
`a442ec5e1c461a59e151d6735fb0b7d23d70d96806880de16118fea7d41ca8d2`.
The independent verifier SHA-256 is
`e93781d77aa0152068c08d3fe124693beb86c43ac32e323bd19cfc137f21f934`.

## Offline presentation comparison

Issue #548 compares the existing `standard` and `plain_language` choices on
three invented development cases with separate identities. Run the JSON exporter
locally:

```text
cargo run --locked -p edgeagent-research --features performance-probe --example probe_explanation -- compare-development-presentations
```

Both proposals pass the production preparation, validation and rendering path.
The plain-language proposal changes only the variant of ranked selections in
the validated standard proposal. The same facts, claim order, citations,
chronology and disclosures are present in both renders. Each report row holds
the reader task, both complete renders and counts of changed rank and other
lines.

| Case | Reader task | Observed difference |
| --- | --- | --- |
| D01 | Find the leader, compare positive and negative scores, and identify the unit | Two rank lines become sentences. The unit is spelled out as parts per million for the 20,000 and −10,000 scores. |
| D02 | Find the excluded instrument, observed turnover, and floor | The rank lines for the eligible instruments change. The 10,025-cent turnover, inclusive 500,000-cent floor, reason and source remain in their original positions. |
| D03 | Trace the leader to its source and distinguish the timestamps | The rank lines become sentences. Citation suffixes and the source row containing event time 91, observation time 95, and availability time 100 are identical. |

The observed total is six changed rank lines and zero changed lines elsewhere.
The explicit unit is a concrete readability improvement for score tasks.
The alternative does not move or rephrase the policy or chronology answers,
or shorten the path from a claim to its source. These are engineering
observations about the rendered output. The comparison collected no reader
preferences and provides no estimate of the frozen 21-of-30 pass rate. Keep the
frozen census and rubric for qualification; development work on policy or
source findability needs a presentation choice that actually changes those
parts of the render.

### Reader-first policy and source layout

Issue #550 adds `reader_first` to the same offline export. The opening
`research-explanation-reader-first.v1` layout places the liquidity finding and
the leading instrument's latest selected source ahead of the complete standard
presentation. It uses the policy renderer's reconciled measurements and source
observations directly. The bounded summary describes the liquidity floor and
the latest selected close. The full details retain the other policy checks,
the remaining sources, the rankings, chronology, and required disclosures.

On D02, the observed turnover of 10,025 USD cents, the inclusive minimum of
500,000 USD cents, and the source digest appear together on lines 4–5. The
standard presentation first shows the below-floor measurement on line 16.
On D03, the leading instrument and source digest are followed by explicitly
labeled event, observation and availability times on line 9. The corresponding
chronology row first appears on line 14 in the standard presentation. The
opening adds 11 lines on D01 and D03 and 12 on D02. This trades a longer
total document for earlier access to the two answers. The complete standard
render is byte-identical after the detail divider.

This layout is scoped to the invented development cases. The export is a
structural comparison. Reader preference, the frozen census, and qualification
remain separate from this experiment.
