# ADR-0129: Configure qualified explanation deadlines

- Status: Accepted
- Date: 2026-11-21
- Milestone: M06 - Evidence-grounded explanation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Configure production inference with the immutable profile's `settings.timeout_ms`.
- Use profile v2 for 30,001 to 600,000 ms and retain v1 identities through 30,000 ms.
- Apply migration 0013 and qualify and approve the exact profile before use.

## Context

The pinned CUDA feasibility probe returned P01 in 57.528 seconds. P03 timed out
waiting for headers at the diagnostic ceiling of 180 seconds. The maintainer
requested a configurable production limit with substantially more headroom.
Ten minutes is a bounded operator option. P03 completion time remains unknown.

## Decision

Accept `settings.timeout_ms` from 1 to 600,000 as a canonical integer, with
exactly one representation per value. The v1 envelope covers 1 to 30,000 ms
and v2 covers 30,001 to 600,000 ms. The timeout participates in profile IDs,
qualification and disclosure approval. Operators choose a value by pinning
a new canonical profile in an owned manifest v2 or v3.

The qualified reservation verifies the profile and caps the deadline using its
timeout. The worker also caps it by artifact, grant and owner permit expiry.
The database requires a positive duration up to ten minutes. Unqualified
reservations keep the 30-second application bound. The owned supervisor scales
worker and collector budgets from the validated profile and retains the
calibration helper's 180-second observation ceiling. The historical manifest-v1
qualification tool continues to admit only profile v1.

## Alternatives considered

A mutable environment override would detach the timeout from qualification and
owner disclosure review. An unbounded timeout could retain a spent attempt
indefinitely. The finite profile revision keeps both guarantees explicit.

## Consequences

A longer timeout can retain a spent attempt longer after interruption. Recovery
still uses its recorded deadline and cannot issue a second send. At the configured
ceiling the collector can take roughly six hours if all 34 requests consume
their full budgets. Schedule collection separately from production execution.

## Compatibility and migration

Existing profile bytes and IDs are preserved. Apply Research migration 0013
before deploying extended profiles. SQL rows and their exact deadlines are
preserved. Rebuild the owned helper with the matching supervisor sources and
re-pin the private manifest. Collect and independently review fresh evidence and
secure approval and owner disclosure authorization for the exact new profile.

For rollback, stop using extended profiles and drain or recover their attempts
using their original deadlines. Leave the compatible widened SQL constraint in
place so historical longer reservations remain valid.

## Security and operations

A longer timeout grants no additional rights. Current owner permit, Research
grant, artifact expiry, result fences and approval revocation remain authoritative.
The feasibility report does not approve any deadline or model.

## Validation

Test canonical v1 and v2 identities, range and envelope mismatches, supervisor
budgets and qualified reservation in isolated PostgreSQL. The test exercises
migration 0013, rejection above the approved profile bound, and the legacy
reservation bound.
