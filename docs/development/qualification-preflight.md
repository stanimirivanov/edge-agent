# Daily qualification profiles and offline preflight

## TL;DR

- PR15A supplies a library profile and read-only policy check, not a live command.
- No credential lookup, network call, policy installation or vendor activation occurs.
- Independent owner approval and current authority are required; config is not approval.
- Unresolved account calls stay blocked; usage is never reset, reserved or refunded.
- PR15B must implement authenticated runtime fencing; PR15C requires separate live opt-in.

## Exact profile

`QualificationProfileData` explicitly supplies format
`edgeagent.qualification-profile.v1`, provider, exact `AcquireEvidence`, original
configuration SHA-256, independent owner-review revision, full `DailyCreditPolicy`,
inclusive start and exclusive end. There are no defaults, credentials, URLs or secret
references. `QualificationProfile::new` produces sorted canonical JSON plus LF and
its SHA-256. `decode` caps input at 8 KiB before parsing and rejects unknown/duplicate
fields, wrong digest/version, noncanonical bytes and invalid timestamps.

The initial positive window is at most 300 seconds and remains inside the admission.
Only Capture, consolidated raw provider-day bars and discarded raw responses qualify.
Twelve Data requires native dates; Alpaca historical SIP preserves nanosecond source
timestamps. Do not convert either representation to fit the other. The configuration
pin names the existing complete frozen provider/selection/reference configuration;
it does not authenticate its contents or establish reviewed source provenance.

## Trusted owner boundary

Call `preflight_qualification` with authenticated `AuthorityCatalogs`, the original
sealed admission, an explicit trusted clock sample, cancellation and a
`QualificationAuthority`. No production authority implementation ships in PR15A.
The port must independently authenticate owner identity and current opt-in for the
exact profile digest/review revision. Unknown, withdrawn, terminal, durably denied
or unverified state must return None/Denied, never echo the supplied profile as approval.

Its fresh assessment binds the independently resolved account and installed credit
policy, original command budget, current daily usage and account-wide uncertainty.
Exactly one current review is required for each control:

| Control | Required owner verification |
|---|---|
| Data-only credential scope | Vendor principal has data rights without trading capability; no broker credentials. |
| Reference provenance | Full calendars, mappings, daily buckets, corporate actions and frozen schedule are authenticated. |
| Runtime fencing | Actual provider/write workers are fenced; lease expiry/timeouts alone do not establish quiescence. |
| Retention and cleanup | Scoped retention, denial, erasure and licensed-copy cleanup cover the original deletion ceiling. |
| Restore and consumers | Restore denial reconciliation and all consumers support the applicable accepted artifact format. |

Reviews must already be known, remain current and cover the original licensed deletion
deadline. Owner approval must cover that deadline too. These are trusted in-process
assessments, not deserializable approval files or cryptographic proof. Merely constructing
a DTO, possessing a credential or verifying a checksum establishes none of these facts.
PR15B owns real authentication, fresh durable reads and actual control qualification.

## Non-resetting accounting and report semantics

Preflight rejects unconfirmed, pending and generation-blocked accounts, including
unresolved work from other commands. Only owner-confirmed quiescence through existing
recovery mechanisms may clear uncertainty. Check durable and cooperative cancellation,
remaining command calls/bytes, original attempts and exact installed credit policy.
Require sufficient credits for **every remaining allowed call** at its explicit maximum
endpoint price. Reject stale UTC usage days, cross-midnight windows, mismatched policy
revisions/allocations/expiry and arithmetic overflow. Never infer public free-tier rates,
roll over usage, renew policy, reserve credits or refund uncertain calls here.

A pass returns the sealed, non-serializable `QualificationPreflight`, with status
`preflight_passed_execution_disabled`, provider and assessment/window times. No dispatch
API accepts this report. It is not a lease, reservation, readiness token or live result.
PR15B must revalidate under actual account/worker fences and atomically reserve through
the existing ledger immediately before each dispatch. Current errors remain typed
`BudgetError` kinds with redacted default diagnostics; trusted callers can inspect causes.

## Verification and remaining work

[PR15B1 owner control](qualification-owner.md) adds independent SQL approvals and
atomic mutation guards. [PR15B2 runtime](qualification-runtime.md) supplies the production
SQL assessment, authenticated complete input and separate finite vendor composition.
Constructing a SQL grant remains an owner assertion, not verification of external facts.

Run `cargo xtask verify --check qualification-preflight` for the six protected tests;
run `cargo xtask verify` for complete default acceptance. Tests use invented profiles,
fake independently controlled owner assessments and explicit times, without environment
mutation, credentials, provider calls or database setup. Both provider branches prove
policy behavior, not the truth of actual account/control reviews. Independent harness
censuses reject missing, renamed, duplicated or additional test identities.

PR15B must compose authenticated owner catalogs/configuration, durable per-dispatch
fencing/recovery, applicable accepted storage and atomic terminal publication, maintenance
and restore readiness. PR15C then needs separately authorized provider calls and cleanup
evidence. The offline worker remains unchanged and rejects vendor activation.
See [the acquisition plan](plans/m03-acquisition.md) and
[ADR-0061](../decisions/0061-separate-owner-reviewed-qualification-preflight-from-execution.md).
