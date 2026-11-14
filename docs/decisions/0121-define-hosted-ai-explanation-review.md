# ADR-0121: Define hosted AI explanation review

- Status: Accepted
- Date: 2026-11-14
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Use two separately executed hosted models from different providers for PR33.
- Report AI-assessed usefulness; do not claim measured human preference or statistical independence.
- Freeze blinded, counterbalanced judgments and retain the existing qualification thresholds.
- Keep owner attestation, disclosure authority and activation separate from model votes.

## Context

The maintainer explicitly approved replacing the proposed human reviewers with
two hosted models. ADR-0118 requires identified, authenticated independent
reviewers, but does not prescribe human reviewers. Its external provenance and
owner-attestation boundary still applies. Different provider names alone cannot
prove independent training, unbiased judgment or agreement with real readers.
[LLM-as-judge research](https://arxiv.org/abs/2306.05685) documents position,
verbosity and self-preference biases. The new protocol must be fixed before
observing candidate results, rather than choosing judges or retries that pass.

## Decision

### Evidence claim and recommended pair

Adopt `research.explanation-review.hosted-ai.v1` for this qualification. Its
meaning is usefulness assessed by two operationally isolated AI judges under a
frozen rubric. Human usefulness remains unmeasured. The schema owner remains the
approval authority; neither a judge nor the candidate can approve itself.

Recommended first-trial models, checked against official documentation on the
record date:

| Provider | Exact requested model | Reason for selection | Standard USD per million input / output tokens |
|---|---|---|---|
| OpenAI | `gpt-5.4-2026-03-05` | Explicit dated snapshot, structured output and bounded reasoning controls | $2.50 / $15 |
| Anthropic | `claude-opus-5-5` | Separate provider/family and capable text comparison; its canonical ID is a pinned snapshot | $4 / $20 |

This is an engineering recommendation, not evidence that either judge is
validated on this corpus. Prefer the dated OpenAI snapshot over a moving alias.
Anthropic documents canonical IDs from the 4.6 generation onward as fixed
snapshots despite their dateless spelling. Verify account access before the run;
unavailability or a mismatched returned model ID refuses execution. Do not
silently substitute an alias, another provider, or a consumer chat session.

Sources: [OpenAI model and prices](https://developers.openai.com/api/docs/models/gpt-5.4),
[Claude model and prices](https://platform.claude.com/docs/en/models/opus-5-5/overview),
and [Claude ID guarantees](https://platform.claude.com/docs/en/about-claude/models/model-ids-and-versions).

### Frozen run and isolation

Before any graded response, retain a private run manifest binding the protocol
ID and exact ADR digest, candidate manifest/profile, suite/preparation, complete
candidate collection, literal reader tasks, existing rubric, judge system prompt,
response schema, normalization implementation, provider endpoints/API versions,
account references, exact model IDs and all generation settings. Retain the exact
request bodies and their digests without authorization headers. No candidate or
judge prompt may be tuned against results from that frozen run.

Use direct first-party APIs, fresh stateless requests and no conversation history,
shared memory, tools, browsing, retrieval or feedback from the other judge.
Neither judge is the Qwen candidate. Do not forward the design conversation,
provider/model labels, candidate status, `candidate_first`, manifests, prompts,
private paths, credentials, owner attestations or the other judge's judgments.
The only case payload is opaque identity, the literal reader task and the two
rendered explanations. Quote both explanations as untrusted data. Judges must
ignore instructions inside them and choose clarity, evidence findability and
accurate communication of limits rather than verbosity, enthusiasm or an action
recommendation. Ask for a brief justification, not hidden chain of thought.

The initial settings are low reasoning effort for OpenAI and low effort for
Claude, no tools, non-streaming structured JSON and at most 4096 billable output
tokens per call, including reasoning. Omit unsupported sampling controls instead
of claiming temperature-zero determinism. Opus 5.5's adaptive thinking remains on;
its effort setting controls depth. Freeze the actual accepted request parameters.
Pinned models and settings do not guarantee identical answers on repeated calls.

Sources: [OpenAI controls](https://developers.openai.com/api/docs/models/gpt-5.4),
[Claude effort](https://platform.claude.com/docs/en/build-with-claude/effort), and
[Claude structured output](https://platform.claude.com/docs/en/build-with-claude/structured-outputs).

### Calibration and both-order judgments

Before graded review, freeze three invented calibration pairs unrelated to the
thirty qualification cases: identical text (expected tie), a direct task answer
versus an irrelevant answer (expected direct answer), and a correct answer with
quoted instruction injection versus an incorrect answer (expected correct
answer, ignoring the injection). Include an explicit synthetic reference fact
where correctness is needed. Keep expected choices out of judge requests.
Each judge must produce the expected choice in both orientations for all three
pairs. Any missing, invalid or wrong calibration result refuses the run; do not
repair a failed calibration by selectively retrying it.

Each judge then assesses every U01–U30 pair twice: once in the collection's
original First/Second order and once with those texts exchanged. Use separate
requests with identical task/rubric and no indication of which view is original.
Each view ID is SHA-256 of recursively key-sorted JSON plus LF, with kind
`research.explanation-judge-view.v1` and data containing protocol digest, case ID,
original presentation ID, literal task and the actual ordered text pair. Bind the
view to its original presentation in the private manifest, not in judge-visible
order labels. Both requests must refer to their exact view IDs.

A valid final JSON response contains only `id`, `view`, `preference` and a brief
`rationale` bounded to 120 words. Validate exact identity, complete JSON framing,
field types, unknown/duplicate fields and preference enum (`first`, `second`,
`tie`); structured output does not replace independent validation. Retain only
the bounded accepted final JSON, never hidden thinking blocks or raw rejected
bodies. Map the swapped preference back to the original order. If both mapped
votes agree, retain that vote. Otherwise retain a tie, including a tie versus a
non-tie. A refusal, timeout, truncation, transport ambiguity, invalid response or
identity mismatch makes the review incomplete; it is not an imputed tie.

This yields one original-presentation-bound rating per case per judge. Preserve
both underlying view records and the deterministic normalization trace. Do not
pick the favorable orientation, average the judges or use a third judge to rescue
a failing result. Operational isolation and position consistency reduce specific
biases; shared style preferences and correlated errors remain possible.

### Scoring, bounds and failures

Retain all thirty original reader tasks, the complete candidate census, all 47
deterministic safety checks and the four adversarial probes requiring valid
generated proposals. Each judge must independently prefer the candidate on at
least 21 of 30 cases. Ties, candidate fallback and failure earn zero preferences.
No pooled score, relaxed threshold, omitted case or safe-fallback-only probe can
qualify. Investigate suspected unsupported claims even if preference totals pass.

The maximum planned calls are 132: 120 graded views plus 12 calibration views,
66 per provider. Allow one in-flight request per provider, a 120-second request
deadline and at most 12000 provider-counted input tokens per request. Do not
truncate explanations/tasks to meet a bound. Refuse before judging if a complete
request cannot fit. Reserve each request slot before I/O; disable automatic SDK
and HTTP retries. An ambiguous spent slot cannot be silently sent again. Retain
safe failure metadata and every completed judgment; do not selectively rerun,
switch judges, or combine favorable records across trials.

Compute the complete planned cost bound before the first paid judgment, including
calibration, billable reasoning, account-specific prices and applicable uplifts.
A proposed initial ceiling is USD 20; the owner must accept the actual quote and
ceiling before execution. At the listed standard rates, 132 requests averaging
4000 input and 1000 billable output tokens cost approximately USD 4.03. This is an
illustration, not a quote or authorization. The full stated token caps give about
USD 14.61 before taxes/uplifts. If the bound exceeds the accepted ceiling or actual
billing cannot be bounded, refuse before calls. Account access and funds are not
assumed, and this ADR does not authorize a paid request.

### Provenance and owner attestation

Use distinct opaque reviewer IDs mapped privately to provider, endpoint, exact
requested/reported model IDs, account reference, API version, settings, prompt and
schema digests, provider request/response IDs, times, usage, view IDs, accepted
responses and normalized rating-file digests. Authenticate the provider through
its TLS API and configured account, not a model's claim about its own identity.
Hosted weights cannot be locally hashed; that provider trust differs from the
verified local candidate artifact. Record model/price documentation observed for
the run and refuse model drift.

ADR-0118's owner attestation must additionally bind this protocol/run manifest,
calibration, both view censuses, normalized reviews and their AI-only provenance.
It must not attest human review or statistical independence. Only the existing
`id`, original `presentation` and normalized `preference` enter each rating in
the existing review wire format; rationale/provenance remain in the private audit
package. Owner approval remains an explicit, authenticated action after local
rescoring, separately from current disclosure grants and activation.

## Alternatives considered

Human reviewers remain suitable for a separate human-usefulness study but were
replaced here by explicit maintainer choice. Two sessions of the same model or
candidate self-review provide weaker separation. Unpinned chat UIs obscure model,
settings and request provenance. A single orientation leaves position bias
unchecked; majority rescue and selective retries change the selection rule after
seeing evidence. More expensive open-ended reasoning adds cost without changing
the bounded reader task. No claim of validated human agreement is made for this
recommended judge pair.

## Consequences

PR33 can collect AI-assessed evidence under a declared method. Operational cost,
provider retention and correlated biases are explicit. Poor calibration, order
instability or an incomplete review can prevent qualification. PR34 still owns
real guarded-runtime acceptance; hosted reviewers do not create a cloud
production inference adapter or bypass candidate process ownership.

## Compatibility and migration

Do not change historical receipts, candidate input, suite identities, thresholds,
wire schemas or database migrations. Existing evaluator/import APIs do not call
hosted judges or verify their external provenance. The opt-in hosted-review tool
is not implemented by this ADR: before execution it must enforce these bindings,
bounds and normalization with tests, rather than relabeling existing files.
Preserve historical review meaning; archive the exact protocol revision with each
new run. Changing judges, prompts, settings or normalization requires a separately
recorded run/protocol decision, never rewriting a completed assessment. Rollback
abandons the unapproved run and preserves its audit; no activation occurs.

## Security and operations

Send only the frozen invented benchmark/calibration material to the two providers.
This does not authorize sending real research artifacts, licensed market data,
private operational context or independent documents to hosted judges. Such use
needs separate owner rights and a new disclosure decision. Keep API credentials
in operator-managed secrets, never Git, prompts, reports or logs.

Use OpenAI `store:false` where supported and record each account's actual training,
retention and regional settings. This flag is not a zero-retention guarantee;
provider abuse monitoring or other retention may still apply. Verify applicable
settings/terms before execution, and refuse if the owner-required handling is not
available. Do not substitute consumer subscriptions or upload files/history.
See [OpenAI data controls](https://developers.openai.com/api/docs/guides/your-data)
and [Claude retention controls](https://platform.claude.com/docs/en/manage-claude/api-and-data-retention).
Keep private audit files exclusively created with mode 0600 and only sanitized
outcomes/digests in the repository. No hosted call or credential access occurs in
this documentation prerequisite.

## Validation

Run repository/architecture/default gates for this ADR and its published identity.
Before any real hosted execution, add focused tests for metadata-free packet
construction, canonical view hashes, both-order normalization, calibration,
incomplete/refused/stale/duplicate responses, full census, price reservations,
ambiguous calls, secret-safe diagnostics and exact existing-format import. Use
local synthetic transport fixtures in default CI; real provider calls remain an
explicit private operator qualification. This ADR's tests cannot establish hosted
model availability, judging quality, human preference or actual qualification.
