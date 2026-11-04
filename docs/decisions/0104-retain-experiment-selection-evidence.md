# ADR-0104: Retain experiment selection evidence

- Status: Accepted
- Date: 2026-11-04
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Register a bounded trial family before importing terminal experiment results.
- Seal one explicit selection after accounting for every registered attempt.
- Retain these facts in the Research owner schema for later promotion review.

## Context

Isolated Research experiments produce deterministic results that carry no
publication authority. A saved winning result alone hides the alternatives,
abstentions and failed attempts considered during selection.

## Decision

The schema owner registers a family with a protocol digest and one to 64
attempts. Every attempt pins a complete Research request and expected evidence
census. The owner then records a canonical experimental result or a bounded
failure category per attempt. Artifact results must match both bindings.
Abstentions match the request and count as terminal without claiming an
observed evidence census.

Selection names one retained artifact result after all declared attempts
terminate. The ordered immutable history identifies the exact selected result
and the full comparison set. The selection represents an operator decision.
The protocol digest identifies the operator's external evaluation methodology.

Serialize appends per scope and family through commit. Rebuild and validate
the bounded history before accepting a command. Return canonical bytes after
commit and accept exact retries against their original slots. Record the
authenticated database actor and transaction time on new rows.

## Alternatives considered

An experiment runner with mandatory persistence would couple isolated
evaluation to the Research database and require handling dispatch and crash
recovery. Importing only winning results would lose the registered denominator
and unsuccessful attempts. Explicit registration and import preserve isolated
execution and give the owner a reviewable record of the declared trials.

## Consequences

Registration precedes terminal imports in the database. This establishes the
census of declared trials. The operator remains responsible for declaring all
attempts and retaining the referenced protocol. Imported bytes are validated
for shape and binding. The ledger cannot authenticate when an external run
happened or certify the statistical validity of its outcome. Selection evidence
can inform later promotion review.

## Compatibility and migration

Apply additive migration 0006 in the private Research schema. Existing
experiment output remains the canonical input to the import command.
A rollback can remove the command while retaining the ledger table for audit.

## Security and operations

Use the authenticated schema owner and configured Research scope for the
local command. The ledger has separate storage and has no path into approval,
publication or the broker outbox. Keep imported artifacts under the same
data handling rules as other Research artifacts.

## Validation

Pure tests cover exact binding, incomplete families, unsuccessful candidates
and immutable selection. PostgreSQL tests exercise concurrent retries,
deferred commit rollback and reconstruction after reconnecting. Process
tests exercise the local command and verify an empty publication outbox.
