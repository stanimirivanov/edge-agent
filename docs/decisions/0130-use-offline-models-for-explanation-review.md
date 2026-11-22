# ADR-0130: Use offline models for explanation review

- Status: Accepted
- Date: 2026-11-22
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Use publisher-quantized Ministral 3 8B Instruct and Phi-4 as two offline judges. Run one at a time on the workstation CPU.
- Keep the existing preference rubric, blinding, calibration, both-order checks and separate reviewer thresholds.
- Judge the six held-out development pairs first. The frozen baseline, census, rubric and acceptance thresholds stay unchanged.

## Context

The held-out comparison on [#508](https://github.com/stanimirivanov/edge-agent/issues/508)
has six verified candidate and baseline pairs. The pinned
Qwen3-4B candidate selected the layouts. The maintainer chose two distinct
local judges and ruled out external inference APIs. This decision replaces
the hosted execution path in [ADR-0121](0121-define-hosted-ai-explanation-review.md)
for prospective reviews on #508. The published ADR-0121 bytes remain
available to reconstruct historical protocol digests.

## Decision

### Model and runtime

Provision only the following two text weight files from their publishers.
Keep the license notices and the exact source revisions with the private
package. Verify the publisher SHA-256 against the local file before loading.

| Judge | Exact publisher artifact | License | Weight file |
| --- | --- | --- | ---: |
| A | [Ministral 3 8B Instruct 2512 Q4_K_M](https://huggingface.co/mistralai/Ministral-3-8B-Instruct-2512-GGUF/blob/0102285ad796bd99af90f58de616092e5630e970/Ministral-3-8B-Instruct-2512-Q4_K_M.gguf) | Apache-2.0 | 5.20 GB |
| B | [Microsoft Phi-4 14B Q4_K](https://huggingface.co/microsoft/phi-4-gguf/blob/6edc2ef6664b739a8e11e62f2672ff6afe0c15ac/phi-4-Q4_K.gguf) | [MIT](https://huggingface.co/microsoft/phi-4-gguf/blob/6edc2ef6664b739a8e11e62f2672ff6afe0c15ac/LICENSE) | 9.05 GB |

The respective file SHA-256 values are
`33e7a72cf5e6e2cfc2f2847075acc013d68bba023e35310cef86b5cf8fdca761` and
`5652b9be0ea4ae2842130d04fe31bc869fcb99a2b7106c53b4e754a343fd688f`.
These are distinct model families and publishers, separate from the Qwen
candidate. Shared evaluation biases remain possible.

Use the installed [llama.cpp b12375 CPU build](https://github.com/ggml-org/llama.cpp/releases/tag/b12375)
as the first runtime to verify. The workstation has an Intel i7-12800H,
30 GiB of RAM, about 21 GiB available at inspection, and a 4 GiB NVIDIA T1200.
Neither file fits entirely in VRAM. The two files occupy about 14.25 GB
on disk, and loading one model at a time leaves room in system RAM for
context and runtime overhead. This is a sizing estimate. Require a real load,
memory and largest-packet preflight for each exact artifact before review.

Use one CPU slot with zero GPU layers and a 16,384-token context, within the
[Phi-4 context limit](https://huggingface.co/microsoft/phi-4). Count each
complete templated input locally with the judge's tokenizer, including the
rubric and packet, and retain the 12,000 input-token ceiling. Check the
output reservation fits the remaining context. Refuse an oversized packet
without truncating it. Pin the runtime, executable and libraries, model and
license bytes, chat template, context, thread count, sampling settings, finite
deadline, response grammar and review implementation. Record memory headroom,
swap activity and elapsed time in preflight. A failure requires a reviewed
plan revision before any judgment.

### Offline execution and review

The versioned offline plan binds the exact two reviewer packages, packet set,
unchanged preference rubric and response schema. The plan reserves all view
slots before execution. Give each judge only the literal task and two
reader-visible texts in a blinded view. Keep the exact presentation bindings,
layout identities, expected preferences and order map in the private scorer.
Use the closed preference vocabulary and validate response shape and view
binding locally even when the runtime constrains JSON.

Reuse ADR-0121's three calibration pairs in both orders for each model before
any graded views. Both judges must pass all six calibration views each.
Assess each graded pair once in each order, in independently presented
packets. Map preferences back to the private order. Agreement preserves the
vote and disagreement becomes a tie. Invalid, missing or ambiguous responses
leave the review incomplete. Retain spent slots and accepted responses.
Do not retry a selected slot or replace a reviewer during a trial.

### Gates and authority

Start with the H01–H06 held-out comparison from
[PR #561](https://github.com/stanimirivanov/edge-agent/pull/561) after that
tooling lands. There are 36 total judgments, including calibration. Each
judge must prefer the gated output on at least three of the four target
pairs, tie both controls in both orders, and flag no factual or evidence
regression. An unchanged target is a tie. This is an AI-assessed development
result.

A passed held-out comparison can inform the decision to rerun the frozen
census. A subsequent qualification uses the original 30 reader cases, four
adversarial probes, 47 deterministic safety checks and original rubric.
The two judges each need at least 21 of 30 candidate preferences. Reuse the
presentation-bound rating format and independently rescore under
[ADR-0118](0118-retain-reviewed-explanation-qualification.md). The owner
authenticates the two exact local model runs and attests their provenance
and separation. This decision grants neither qualification approval nor
production activation.

## Alternatives considered

Hosted judges require external inference and disclosure. Human reviewers
would measure human preference directly but require separate reviewers and
scheduling. Using the Qwen candidate for judging would allow it to assess
its own selections. The chosen pair can be calibrated locally and run on
the same CPU in sequence.

## Consequences

The two judges can run with the same pinned CPU runtime. Their ratings
measure AI-assessed usefulness. The owner remains accountable for
qualification and attestation. Runtime compatibility, resource headroom
and calibration are still to be measured.

## Compatibility and migration

This scoped execution decision keeps ADR-0121's rubric and both-order
procedure in force. Its historical hosted evidence remains reproducible.
The offline scorer preserves ADR-0118's presentation-bound rating format
for frozen qualification and the separate development review format for
held-out tasks. Retain historical files and evidence.

## Security and operations

Stage only public artifacts before review. For execution use verified local
paths, no external egress and no credentials. If using the existing
llama.cpp HTTP interface, bind only loopback and use an owned process with
one in-flight request. Do not pass the candidate service or profile
credentials. Keep the two model processes and result streams separate and
make each view a fresh request without conversation history or tools.

Keep packages, full review records and model artifacts private. Commit
sanitized decisions, status and digests.

## Validation

An offline runner and the two measured judge packages are required before
the held-out judgments can run. Test view blinding and identity, reversed
orders, calibration refusal, resource limits and incomplete results with
local fixtures before real inference. Verify each exact weight and license
pin and complete the largest-packet preflight before judging.
