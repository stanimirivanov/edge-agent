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

### Selecting the development layout

Issue #552 adds a bounded candidate response to the offline development path.
Use the comparison command above to get each case ID and its `choice_binding`.
Pass a candidate response on stdin, using the requested case as a separate
argument:

```text
cargo run --locked -p edgeagent-research --features performance-probe --example probe_explanation -- select-development-presentation D02 < choice.json
```

```json
{"kind":"research.explanation-development-choice.v1","case":"D02","input":"CHOICE_BINDING_FROM_EXPORT","layout":"reader_first"}
```

The selector accepts up to 1,024 bytes. The choice must match the requested
development case and the digest of its prepared input, reader task and both
eligible renderings. Supported selections are `standard` and `reader_first`.
Duplicate or extra fields, unsupported values and cross-case bindings refuse
without returning a presentation. The candidate supplies a layout choice only.
The existing renderer supplies the facts, citations, timestamps and disclosures.

Both choices were exercised on D01–D03 using synthetic responses. All six
selected texts matched their comparison exports, and three responses bound to
different cases were refused. The existing comparison outputs matched their
previous values apart from the new binding field. This command exercises
candidate selection in the development harness. Model transport, reviewer
preferences and frozen qualification remain separate.

### Owned CUDA development choices

Issue #554 connects the pinned model to the offline selector. Build the release
`probe_explanation` example with `--features performance-probe`. Prepare a new
private CUDA manifest with that helper and the exact checked-in
`development-layout-prompt.txt` file in the Research explanation fixtures.
Recompute the prompt, helper, deployment, profile and manifest pins. The
development package reuses the runtime, weights, libraries, device, context and
profile settings from the CUDA qualification candidate.

Run each of D01, D02 and D03 once with its own report path. Use the frozen
supervisor snapshot and substitute the exact manifest digest:

```text
python3 scripts/run_explanation_owned.py --manifest /private/development.manifest.v3.json --execute-manifest MANIFEST_SHA256 --development-layout-case D01 --output /private/D01.json
```

The example independently prepares the invented task and both verified
presentations. The transport rechecks the precise input bytes, anchor and
choice binding before using the existing loopback transport and owned deployment
verifier. A closed response schema admits only `standard` or `reader_first`
for that case. The offline selector independently verifies the response and
renders the choice. Each model wait is capped at 180 seconds and the report
stores only outcome, layout, digests, timing and closed diagnostics. This is a
development model selection experiment rather than a reader preference study.

The owned CUDA run attempted each case once on manifest
`9bc68784033ee315d0fc206e5edd037153963e7b108e3c02cc3b54a260698b37`.
The profile SHA-256 is
`67b6d4b7450ce4e9e3625da77c0f7045b299f1309159f4e742af2a74c07e0959`.
The private preparation record SHA-256 is
`e35762b3ae01f3c9a207c8e754d6373759453f6fd1799cc3aec7bd38225c616c`.

| Case | Validated choice | Request time |
| --- | --- | ---: |
| D01 | `standard` | 92.832 s |
| D02 | `standard` | 131.623 s |
| D03 | `standard` | 96.306 s |

Each rendered digest matched the selected offline presentation. The three
private report digests in case order are
`f7fbfac8db88051a97870c94c9b1cf62cf433c87fd3c128123c4ca0694c6790a`,
`629ecae09c6b7fe40e2abea757c36ad66ae0a4111779560b025e603e035dd6b5`, and
`04f0cb0e736e2879c802018e639d02cf1ee86be15be76d04296313673f7198e4`.
The wiring accepted real model choices on all three fixtures. The model
continued to select the standard layout even for the policy and source tasks.
This run leaves the usefulness blocker open on #508.

### Task-aware CUDA prompt

Issue #556 tests one prompt revision on the same three development tasks.
`development-layout-task-aware-v2.txt` directs the model to look for the
task's concrete answer and its evidence together in the reader-first opening.
It distinguishes evidence or policy questions from score comparisons. The
helper admits the new prompt bytes for development selection. The historical
run retains its matched helper and prompt. The development response schema,
case bindings and both renderings are identical to those from #554, as
verified by comparing the two helper exports before inference.

The private package reuses the previous runtime, weights, libraries, device,
context and settings. It re-pins the prompt, rebuilt helper and derived
deployment, profile and manifest. The manifest SHA-256 is
`c26b2fb25daf93020006f7c64d869e2525845f86ce34305e6336c40e11bc2a70`.
The prompt SHA-256 is
`80d1f36c741ac00e7b39e0bc399e647d3b11a56c5f1785dc04e023cd2373cc05`.
The private preparation SHA-256 is
`6c5bffeb297656caca8854d16cbca97aa04dd14a2bce601638f19391bb07f99c`.
The expected engineering choices are standard for D01 and reader-first for
D02 and D03. The prompt itself contains no fixture IDs or answer mapping.

| Case | Expected | Validated choice | Request time |
| --- | --- | --- | ---: |
| D01 | `standard` | `reader_first` | 93.323 s |
| D02 | `reader_first` | `reader_first` | 132.567 s |
| D03 | `reader_first` | `reader_first` | 96.118 s |

Each case ran once and returned a valid, fixture-bound choice. Every selected
presentation digest matched the offline export. The model selected the
intended opening on the policy and source tasks, but it also selected that
opening for the rank and score task, where it does not supply the requested
score comparison. This result shows that the prompt changes model choices.
It does not establish reliable task discrimination or reader preference.
Issue #508 remains open.

The private profile SHA-256 is
`b44887c4b2361a38c35f9092ce9ce3ee3a8f2db2f2067ce8aeb360ab2f37b521`.
The private report digests in case order are
`92429d4e29d8823bed3c48e5220781ece236a1860e5275c81e915cef80ea3e24`,
`6ef5234d8e9af5d1f6f34f214db73b561e9cca34b45128b9bbe77b73fc8c23cc`, and
`0af8a183d5e85c2fac65e30dbdef914ad15f89b410f8aff1154770595b6da3a9`.

### Deterministic reader-first eligibility

The development selector checks whether the verified opening covers the
fixture's task before asking the model to choose a layout. D01 asks for score
comparison and units, which the opening omits, so only `standard` is eligible.
D02 can offer `reader_first` because the opening contains an observed
below-floor instrument, its turnover and its applicable minimum. D03 can
offer it because the opening includes the leader's evidence and labeled
event, observation and availability times.

The request includes only eligible presentations, and its response schema
lists exactly those layouts. The independent selector also rejects an
ineligible choice with an otherwise valid binding. The new choice-input
binding includes eligibility, so historical responses remain tied to the
previous experiment. All offline renderings matched the previous export
byte for byte. The frozen baseline, census and rubric are unchanged.

The CUDA run reused the task-aware prompt and the same runtime, weights,
libraries, device, context and settings. The only manifest entries re-pinned
were the helper and its derived profile. The manifest SHA-256 is
`e4e52e65a0f8f6a7e44dd0785f56311778a2916e640e7a5b9df02ea72b0b5714`.
The profile SHA-256 is
`d57310dbf2e510f3847d3be8a49d79bb00aca737e50bcb9ae6cf514b810b93e9`.
The private preparation SHA-256 is
`bb0dbe882132fe0302b1d8e4c029171c66579927e209721bd6758c2a17603d30`.

| Case | Eligible layouts | Validated choice | Request time |
| --- | --- | --- | ---: |
| D01 | `standard` | `standard` | 56.251 s |
| D02 | `standard`, `reader_first` | `reader_first` | 132.328 s |
| D03 | `standard`, `reader_first` | `reader_first` | 95.900 s |

Each case ran once. All selected presentation digests matched the offline
exports. The private report digests in case order are
`13408b4a350fe6760dcd3fa135a36e61ed0b81d388fd8b2d3f6ea704d3aa5d21`,
`01788763c546ef22c4e5e80f581f08e360c757e26dbb0022a702b91b9c5b8250`, and
`9c9a9fceb7a50e98cd98bf77231b332ef8494684762276ff153aed9c28528cdb`.
This verifies layout eligibility and selection on the three development
tasks. Reader preference and frozen-suite qualification remain open on #508.

### Held-out usefulness comparison

Issue #560 compares `standard` with the layout actually returned by the
pinned selector. The six invented H01–H06 tasks have their own bindings.
H01 and H02 cover liquidity exclusions, H03 and H04 cover source and
chronology, and H05 and H06 control for score and horizon questions. The
development fixtures D01–D03 and all frozen qualification inputs stay intact.

Collect one report per case with the owned supervisor, using the same
`--development-layout-case` command as above and a fresh private report path.
With the exact manifest digest, prepare blinded packets and score the returned
judgments locally:

```text
python3 scripts/review_explanation_heldout.py packetize MANIFEST MANIFEST_SHA256 ATTEMPT_DIR PRIVATE_REVIEW_DIR
python3 scripts/review_explanation_heldout.py score PRIVATE_PLAN PLAN_SHA256 PRIVATE_RESULT JUDGE_1_JSON JUDGE_2_JSON
```

Share only individual packet files with each authorized reviewer, in isolated
sessions, one packet per judgment. Use the existing `PROMPT` preference rubric
in [review_explanation_hosted.py](../../scripts/review_explanation_hosted.py).
Add a separate check for factual or evidence regressions on either presentation.
Each reviewer returns a private object with their distinct authenticated
reviewer ID in `reviewer` and a `ratings` array. Each rating contains the
packet's `id` and `view`, a `preference` of `first`, `second` or `tie`,
a short `rationale` and a Boolean `regression`. The operator verifies reviewer
identity and independence before importing ratings. Retain the plan and
ratings privately. Hosted review of these new packets requires authorization
for this development material.

Before packetization the tool verifies the exact attempt and presentation
bindings. Packets omit the machine-only layout headers and role metadata.
Their view digests bind the exact original presentation digests and ordered
reader-visible text. Both orientations must favor the same side to earn a
preference. Identical target choices earn zero credit. To advance, each
reviewer must prefer the gated layout on at least three of four target tasks,
explicitly tie both identical controls in both orders, and report no factual
or evidence regressions. A missing reviewer leaves the experiment incomplete.
These results have no qualification or activation authority.

The local collection used the same weights, CUDA runtime, device, context,
task-aware prompt and inference settings as #558, with a rebuilt helper and
the supervisor's expanded development allowlist re-pinned. The old D01–D03
export matched byte for byte. The new manifest and preparation SHA-256
digests are
`dda7d228d0d36eb39e588f28978e4d3410ef9e10dab839bf11a718ac0c423aef` and
`9be776a62fdcebd187b13ea586c444a7e5b723c597ee475206bfd97123c37aa4`.
The H01–H06 export was pinned before collection as
`fc44fe64b34526fe917deb9167e2d2660476f450e0f903aa89f7bca2f4f2e715`.

| Cases | Task | Validated choices |
| --- | --- | --- |
| H01, H02 | Liquidity | `reader_first`, `reader_first` |
| H03, H04 | Source and chronology | `reader_first`, `reader_first` |
| H05, H06 | Score and horizon controls | `standard`, `standard` |

Each case ran once. Packet preparation verified all six selected presentation
digests against the pinned helper. The 12 blinded views and private mapping
were retained under plan SHA-256
`eefeae5033126494fbba0d6a927f5fc33e62c6a9ca562eaf397c7c9688bcd7a6`.
The four target pairs have different layouts and both controls have equal
text. The scorer returned `incomplete` with no independent judgments
submitted. This run verifies the candidate selections and comparison
preparation. Reader usefulness remains open on #508.

### Offline held-out judge runner

[ADR-0130](../decisions/0130-use-offline-models-for-explanation-review.md)
selects the two licensed local judges. The runner consumes the private
held-out plan and its twelve packet files. It uses the existing rubric
and scorer and adds the held-out factual/evidence regression flag to the
closed response grammar.

Prepare a private JSON configuration with `runtime` (the pinned b12375
`llama-server`), `libraries` (pinned shared libraries including the system
libraries mapped by the server), and a `judges` object with `ministral` and
`phi4`. Each judge has `weights`, `license` and `template` file pins.
Each file pin contains an absolute `path` and its `sha256`. Use the
publisher's exact weight files and keep the corresponding chat templates
and license notices with the private packages. Also specify integer
`threads`, `timeout_seconds` and `memory_headroom_mib`.

```text
python3 scripts/review_explanation_offline.py prepare --config PRIVATE_CONFIG --heldout PRIVATE_HELDOUT_PLAN --heldout-sha256 HELDOUT_PLAN_SHA256 --packets PRIVATE_PACKETS --plan PRIVATE_OFFLINE_PLAN
python3 scripts/review_explanation_offline.py run --plan PRIVATE_OFFLINE_PLAN --plan-sha256 OFFLINE_PLAN_SHA256 --output NEW_PRIVATE_OUTPUT
```

Review the printed plan digest and configuration before execution. On Linux
the runner uses Bubblewrap to put the driver and judge in a network namespace
with only loopback. It clears the environment and mounts only the code,
pinned artifacts, standard runtime paths and a private audit directory.
It uses one CPU model at a time. Each preflight counts every full templated
input and measures the longest input while reserving room for the
response. Swap activity or insufficient available memory blocks judgments.

Both preflights and all twelve calibration judgments must pass before any
scored view. The final assessment uses the existing held-out pass criteria.
The runner reserves the trial by plan digest next to the plan and writes
each spent slot before inference. It retains input counts, bounded numeric
preflight evidence, accepted judgments and the closing result in the
exclusive private output. If execution refuses or stops, preserve the audit
and review a fresh plan before starting another trial.
