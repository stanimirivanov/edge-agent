# ADR-0123: Bound the Chrono maintenance exception

- Status: Accepted
- Date: 2026-11-18
- Milestone: M01 - Rust engineering foundation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Accept only the three maintenance records below for their existing crates.io versions and scopes until 2026-12-18T00:00:00Z.
- EdgeAgent maintainers own the risk and [migration issue #518](https://github.com/stanimirivanov/edge-agent/issues/518).
- Enforce scope and expiry before every supported audit; other advisories remain blocking, including future Chrono vulnerability records.

## Context

RustSec marks every Chrono version unmaintained and lists no patched release in
[RUSTSEC-2026-0543](https://rustsec.org/advisories/RUSTSEC-2026-0543).
The durable-execution proof resolves it through duroxide-pg 0.1.38 and SQLx 0.8.6;
the latest published provider and its upstream manifest still require Chrono.
Production, event-spine-proof and temporal-proof also resolve Chrono 0.4.45.
Removing only the proof's direct dependency cannot eliminate these transitive uses.
The root audit additionally found chrono-tz 0.10.4
([RUSTSEC-2026-0555](https://rustsec.org/advisories/RUSTSEC-2026-0555)) and
humantime 2.4.0 ([RUSTSEC-2026-0542](https://rustsec.org/advisories/RUSTSEC-2026-0542)).
Both records mark all versions unmaintained with no patched release. During the
same unmerged PR #513 review on 2026-11-18, the maintainer explicitly extended the
approved exception to these two records, retaining the original expiry and
migration owner. This record captures that final agreed scope.

## Decision

Keep the existing dependency resolutions unchanged. Permit only the exact
advisory IDs and reviewed reason in the applicable cargo-deny policy:

| Package | Version | Advisory | Approved lock scope |
| --- | --- | --- | --- |
| chrono | 0.4.45 | RUSTSEC-2026-0543 | Root, durable-execution, event-spine-proof, temporal-proof |
| chrono-tz | 0.10.4 | RUSTSEC-2026-0555 | Root only |
| humantime | 2.4.0 | RUSTSEC-2026-0542 | Root only |

Root means `Cargo.lock`; proof locks live under `experiments/<proof>/Cargo.lock`.
All packages must resolve from crates.io. Root deny.toml carries all three IDs;
experiments/deny.toml retains only Chrono because the two additional packages are
absent from proof resolutions. No new scope, version, source, duplicate record or
unreviewed reason is covered. Removing any retired ignore entry remains allowed.

The exception expires exclusively at 2026-12-18T00:00:00Z. The existing Python
supply-chain verifier checks the trusted current UTC date, exact exception fields,
locked scope/version/source and strict advisory settings. Tests inject dates;
operator commands cannot override the clock. The canonical runner requires this
preflight even when a single dependency-policy check is selected. Each CI
cargo-deny action has the same preflight before execution, including scheduled
audits and M08 jobs. The verifier rejects removed or reordered CI preflights.

Do not extend expiry automatically. The owning migration issue must remove Chrono, chrono-tz and humantime
and their ignore entries, or obtain a new explicit reviewed decision before expiry.
An unfinished migration leaves checks blocked. Removing the exception after the
migration is allowed; an empty ignore list is always stricter.

## Alternatives considered

An upgrade cannot resolve an all-versions maintenance advisory. Replacing only
duroxide-pg would leave production and other proof users. Immediately forking
multiple upstreams or changing timestamp/calendar implementations is a larger
maintenance and compatibility commitment, requiring separate review. Globally
allowing unmaintained dependencies or suppressing all advisories for these packages would
accept unreviewed risk and is rejected.

## Consequences

CI can run current proofs during the migration window without changing their
storage backend, crash/cancellation assertions or decision-evidence meaning.
The unmaintained dependencies remain present: unknown or newly discovered defects
and upstream migration delays remain risks. This acceptance does not declare
these packages safe, maintained or suitable for expanded use. No license exception is made.

## Compatibility and migration

No runtime, wire/schema, timestamp, timezone, persistence or lockfile change occurs.
Issue #518 owns phased replacement, upstream coordination and compatibility tests.
Preserve historical research and qualification evidence and keep the Duroxide
PostgreSQL proof boundary intact. This policy is a CI prerequisite for the current
M06 work, not an expansion of explanation runtime authority.

## Security and operations

`cargo-deny` 0.20.2 supports an ID/reason ignore but no native expiry. A raw
cargo-deny command alone is therefore not the aggregate acceptance gate: use
`cargo xtask verify dependency-audit` or a named dependency-policy check, which
always prepends the Python preflight. CI clocks are the time source. Continue
refreshing the advisory database normally; all other IDs, unsound/yanked checks,
license/source/bans rules remain unchanged. New vulnerabilities are not waived.
Rollback removes the reviewed ignore entries, restoring their advisory failures.

## Validation

Mutation tests accept the last valid UTC date and reject the expiry day and later,
other IDs/reasons, relaxed advisory settings, changed package versions/sources and new
workspace scopes, including attempts to add root-only records to a proof policy. Workflow mutations require the preflight before the native audit.
Runner tests prove explicit audit selection cannot omit the preflight. Run the
current six-workspace dependency audit, focused verifier tests, ADR policy tests
and complete default repository gate. These checks do not prove migration done,
absence of unknown vulnerabilities, or fresh real-model qualification.
