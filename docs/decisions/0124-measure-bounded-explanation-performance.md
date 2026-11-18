# ADR-0124: Measure bounded explanation performance

- Status: Accepted
- Date: 2026-11-18
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Observe nine invented capacity attempts through production preparation, HTTP exchange and validation, with an independent 180-second observation ceiling.
- Require explicit opt-in, an idle host, exact private file pins, a fresh owned runtime per attempt and a 90-minute whole-run bound.
- Retain only metrics and closed labels. No model reply, grading result, approval or publication authority leaves the measurement path.

## Context

The PR33 diagnostic replay timed out before response headers at the unchanged
30-second production deadline. It did not measure complete-response capacity or
host contention. The maintainer approved an idle CPU benchmark followed by a
separate bounded end-to-end performance probe before deciding a new deadline.
ADR-0122 freezes diagnostic replay settings; increasing that mode's deadline
would change its meaning. Performance therefore needs a separate protocol.

## Decision

Add an opt-in `performance-probe` build feature and dedicated Research example.
Ordinary service modes cannot select it. Reuse the exact production HttpModel
exchange, deployment verification, response parser, typed proposal acceptance,
shared preparation and deterministic renderer. The measurement method accepts
only the capacity-calibration namespace and discards Reply and presentation
bytes, returning only closed status/stage labels and elapsed milliseconds.
The normal model port and canonical profile deadline remain unchanged.

Use three separately namespaced invented calibration sizes (one, four and twelve
instruments), three attempts each. Share deterministic fixture construction and
production preparation with the existing harness; preserve every frozen grading
input and pin. Do not select reader tasks, ask hosted judges, score usefulness or
use owner/private inputs. Report actual input bytes/digests rather than claiming
a token-length envelope.

The private launcher requires the exact manifest digest as execution consent,
uses existing file/hardware/profile checks and waits for sustained idle admission.
Each attempt starts and reaps a fresh owned server. A timeout cannot leave an
abandoned generation ahead of the next request. Retain all nine observations,
including timeouts and invalid output, with no selective retry. Abort the census
on a supervisor/identity/cleanup failure. A whole-process 90-minute timer bounds
idle waiting, startup and execution; owned cleanup runs before returning.

The request observation ceiling is 180 seconds, not a new production setting.
The supervisor allows 190 seconds after model startup for preparation, request,
validation and reporting, preserving its normal worker/collection bounds. Model
startup retains the existing 120-second readiness bound and helper control the
existing 90-second bound. Persist an exclusive mode-0600 JSONL journal with
manifest/launcher/profile/calibration digests, input sizes, fixed census,
start/finish markers, request and orchestration durations, and numeric host/load
observations. Never retain prompts, responses, rendered text, raw logs or errors.

## Alternatives considered

A second HTTP client would not measure production framing/validation. Changing
profile-v1 timeout bounds would mix measurement with production policy. Reusing
the frozen grading census would permit performance tuning against evaluation
inputs. Keeping one server after timeouts could measure abandoned-request queueing.
These approaches are rejected for this probe.

## Consequences

Valid output beyond 30 seconds becomes observable without permitting publication.
Timeouts remain censored observations, not measured completion latencies. Fresh
processes measure no shared-KV warm-call behavior; file caches and thermal drift
remain possible. Aggregate external CPU estimates and optional thermal telemetry
are observations, not proof of exclusive CPU ownership or a percentile SLA.

## Compatibility and migration

No profile/schema/deadline, persistence, grant, frozen census or production port
changes. Use a new private package with the dedicated helper and updated exact
helper/launcher identity; preserve historical profiles/manifests/reports. Existing
worker and collection modes still require the service helper. No approval is
installed. Choosing a production deadline and any representation/authority-budget
changes require a later decision after measurements.

## Security and operations

Use only operator-reviewed local weights/runtime/license/prompt files. The model
sees invented inputs, never retained owner evidence. Closed performance envelopes
are not qualification collections and grant no permission. Normal CLI modes
cannot select the longer observation ceiling. The owned launcher retains
parent-death handling, listener/process fencing and cleanup. Cancellation and
budget expiry abort further attempts; a missing completion marker is incomplete.
Rollback removes the feature/example and launcher without changing stored state.

## Validation

Loopback tests cover late valid responses with unchanged profile timeout,
invalid proposals, observation bounds, lease release and collection rejection.
Calibration tests compare all frozen pins and require distinct increasing input
sizes. Python tests cover consent/idle refusal, fixed census, exclusive reports,
payload rejection, stop-on-failure and actual owned-runtime cleanup. Default
verification tests and lints the opt-in feature. No real model run is claimed by
these checks; the operator must request that separately on an idle workstation.
