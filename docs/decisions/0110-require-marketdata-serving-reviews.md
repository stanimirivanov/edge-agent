# ADR-0110: Require MarketData reviews for explanation serving

- Status: Accepted
- Date: 2026-11-09
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- MarketData independently reviews the right to serve derived explanations.
- Require a current review for the sealed acquisition rights revision, feed
  scope and exact reader audience. Acquisition permission alone is insufficient.
- Reconcile the retained acquisition and current retention before answering.
  Research separately checks its grant, companion and artifact lifecycle.

## Context

[ADR-0109](0109-bound-evidence-grounded-explanations.md) requires evidence-owner
rights and Research authorization at serving time. Acquisition rights address
non-display processing and storage. They do not establish that a derived
explanation can be served to a particular audience.

## Decision

MarketData records append-only serving reviews against the exact rights pin
and reviewed feed scope of a sealed admission. A stable, owner-reviewed reader
audience identifies the redistribution boundary. The schema owner appends a
consecutive revision containing the authenticated reviewer, opaque agreement
reference, effective interval and allow, deny or revoke decision. Select the
latest revision before testing validity. Missing, denied, revoked, future
or expired reviews refuse. A new rights pin requires a separate review.

A check reconciles the exact retained owner publication, acquisition and object.
It requires current underlying owner rights, intact retention and the explicit
serving review. The check returns a decision without evidence bytes, a bearer
grant or a caller-selected policy. It applies only to derived explanations.
Quoted documents and other independently licensed inputs require their own
authority. Where several evidence objects contribute, each must pass.

Research resolves the companion and evidence binding, checks its own current
serving grant and artifact lifecycle, then obtains a fresh MarketData decision.
Research rechecks its state before replying. The projector uses that answer for
one read. Revocation applies to subsequent checks and cannot retract bytes sent.

## Alternatives considered

Expanding the meaning of acquisition grants would implicitly authorize existing
records without review. Keeping the serving decision entirely in Research would
bypass MarketData's owner authority and retention state.

## Consequences

A missing serving review leaves the original acquisition and Research outcome
available under their existing policies. MarketData must be reachable for each
explanation read. Review history retains policy identities and reviewer attribution
without storing evidence object digests or explanation content.

## Compatibility and migration

Apply the additive MarketData migration before enabling owner authorization.
Existing acquisitions need explicit serving reviews. The installed local
owner process exercises the same ledger on invented data. Live feed support
requires its own installed current-rights adapter and reviewed records.

Stop callers before rolling back the authorization command. Keep the review
ledger so that a later rollout still sees the latest denial or revocation.

## Security and operations

The local operator runs `offline-explanation-review ACQUISITION_ID` as the
MarketData schema owner. Provide one JSON object on stdin with `audience` and
`agreement` as stable 32-character evidence IDs, `revision` as a positive
integer, `decision` as `allow`, `deny` or `revoke`, and integer
`valid_from_ms` and `expires_at_ms`. The interval is inclusive/exclusive.
An exact retry by the same reviewer is idempotent. No data is written to stdout.

The trusted local caller invokes
`offline-explanation-authorize ACQUISITION_ID AUDIENCE_ID` with the exact
retained Research evidence event on stdin. This uses the existing owner
installation and fixed Research scope settings. Exit 0 permits, 77 denies,
75 means unavailable, and 65 means invalid input or integrity failure. Only
exit 0 with empty stdout is an allow. Treat all other results as refusal.


## Validation

Run `cargo xtask verify --check research-evidence-processes --python python3`
against the isolated local PostgreSQL profile. The real owner process covers
missing and active reviews, wrong audiences and tampered rights pins, expiry,
revocation, unauthorized appends, unavailable storage, and retired evidence.
