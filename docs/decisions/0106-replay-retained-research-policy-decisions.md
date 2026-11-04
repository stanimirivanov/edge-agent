# ADR-0106: Replay retained Research policy decisions

- Status: Accepted
- Date: 2026-11-04
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Resolve validated replay from the exact configuration retained with the decision.
- Recompute the terminal outcome and full policy decision from currently authorized original input.
- Keep the original read-only replay path for receipts without a decision companion.

## Context

The original replay path calculated momentum from close-only inputs. Validated
publication now retains a complete selected policy and its canonical decision.
A policy-derived abstention or narrowed ranking requires those historical rules
to be reproducible.

## Decision

Return the checked selection alongside its receipt from the existing read-only
owner lookup. The lookup validates the original inbox delivery, immutable
outbox proofs, exact historical approval and retained control references before
releasing the transaction. The CLI uses retained implementation, configuration,
age and expiry settings and the existing operator-supplied owner connection.

Ready input goes through the complete owner policy evidence read and the
compiled selected execution validator. Compare exact canonical terminal and
decision bytes, including the evaluated eligibility set and reason codes.
Owner-declared and direct-input abstentions derive their outcome and provenance
from the original delivery. A current owner read failure refuses replay.

## Alternatives considered

Requiring the worker configuration file would let operational configuration
changes prevent replay of intact historical decisions. Comparing only terminal
bytes would miss a changed policy reason when two decisions yield the same
abstention.

## Consequences

Replay can reproduce a historical policy decision after profile retirement or
a later operational stop. Reproduction establishes byte equality under the
recorded settings. Continued evidence access depends on current owner rights,
and historical binaries must still be supplied by a compatible release.

## Compatibility and migration

The command keeps its existing input, explicit compiled revision pins and
terminal stdout format. Receipts without a decision companion keep the legacy
owner profile and close-only replay. Existing schema and checked SQL statements
support both paths. Rollback restores the previous replay binary.

## Security and operations

Replay obtains history in a read-only transaction and releases it before owner
I/O. The verifier has no publication or database write capability. Original
delivery identity, current owner authorization and safe refusal diagnostics
remain part of the boundary.

## Validation

Process proofs reproduce a policy denial after removing the active configuration,
refuse a changed policy reason, and compare retained state before and after
replay. Existing process and pure proofs cover original delivery checks, exact
artifacts and owner-declared abstentions, bad code pins and owner read failures.
