# ADR-0061: Separate owner-reviewed qualification preflight from execution

- Status: Accepted
- Date: 2026-10-25
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Bind a finite qualification profile to one admission, configuration and owner review.
- Require independent current authority, control reviews and account-wide quiescence.
- Read existing daily-credit usage without resetting, reserving or refunding it.
- A preflight pass never enables execution or counts as live qualification.
- Separate PR15A library checks, PR15B authenticated runtime and PR15C live proofs.

## Context

Offline provider adapters and native/precise publication paths do not establish
permission to use actual accounts or data. Credentials, checksums and supplied DTOs
cannot authenticate rights, reference provenance, operational controls or owner opt-in.
Unresolved calls remain charged and fenced; a new profile must not reset their state.

## Decision

Add application-owned, bounded canonical qualification profiles and a synchronous
read-only preflight. Profiles bind the exact admission/configuration, explicit provider,
daily-credit policy, owner-review revision and positive window of at most 300 seconds.
Decode checks size before deserialization, expected digest, strict fields and canonical
bytes. Reuse terminal canonicalization without changing its existing wire bytes.

Reuse current authenticated authority catalogs and admission verification. A separate
trusted owner port must authenticate exact-profile opt-in, current installed policy,
durable command/account snapshots and five reviews: data-only credential scope,
reference provenance, runtime fencing, retention/cleanup and restore/consumer safety.
No production adapter or default approval ships in PR15A. Constructing an assessment
does not authenticate it; PR15B must implement this boundary independently of config.

Reject unconfirmed, pending or generation-blocked accounts. Check current command
cancellation, budgets and unchanged credit allocation/revision/expiry. Require enough
credits for every remaining allowed call at the explicit endpoint maximum price.
Do not reset usage, infer free-tier prices, install policy or clear blocks. Initial
profiles use Capture, consolidated raw daily bars, discard raw responses, stay within
the admission and one already-current UTC credit day. They require native dates for
Twelve Data and precise nanoseconds for Alpaca historical SIP. Owner/control reviews
must cover the original licensed deletion ceiling, not only the shorter run window.

Return a sealed, non-serializable point-in-time report labeled
`preflight_passed_execution_disabled`. No dispatch API accepts it. PR15B must recheck
current authority, cancellation, blocks and credit reservation at actual dispatch under
durable fences; a preflight snapshot cannot close that race.

## Alternatives considered

- Enable the offline worker with vendor flags: rejected; its invented authority and
  local process lock do not establish authenticated vendor or distributed fencing.
- Treat profile digest/credentials as permission: rejected; identity is not approval.
- Add a parallel quota limiter: rejected; existing durable accounting owns uncertainty.
- Implement runtime and live qualification together: rejected; distinct trust boundaries
  require separate review and explicit authorization before credential/provider use.

## Consequences

PR15A proves fail-closed policy entirely offline. It cannot run acquisitions, reserve
credits or prove actual provider scope, cloud restore or lifecycle quiescence. Conservative
same-day and full-call credit requirements may deny otherwise feasible runs; relaxing
them needs an explicit reviewed policy rather than fallback behavior.

## Compatibility and migration

Additive library API only; no SQL migration, dependency, service flag, endpoint or
published event-schema change. Existing offline composition and canonical terminal
bytes remain unchanged. PR15B consumes profiles after implementing authenticated
owner composition; PR15C alone records actual opt-in live qualification.

## Security and operations

Profiles contain no credentials or secret references. Preflight never reads environment
variables, credentials, files or network clients. Diagnostics redact identities and
payloads; explicit error-source inspection remains available to trusted code. Owner
assessments are trusted in-process inputs, not cryptographic proofs or deserializable
approval files. Withdrawn, terminal, denied or unverified owner state must fail closed.
No broker, trading, model or provider call is enabled.

## Validation

Protect six deterministic preflight tests in the default harness with an independent
named-test census and mutation coverage. Test canonical tampering/bounds, both protocols,
missing/stale approval, changed bindings, revoked authority, cancellation, missing/expired
controls, unresolved account state and exact non-resetting credit exhaustion. Route
shared canonicalization to existing terminal/native contract proofs. Run the complete
default harness; passing it is not live-provider qualification.
