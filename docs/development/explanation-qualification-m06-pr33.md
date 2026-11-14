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
