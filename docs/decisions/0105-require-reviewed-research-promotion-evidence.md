# ADR-0105: Require reviewed Research promotion evidence

- Status: Accepted
- Date: 2026-11-04
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Require reviewed promotion evidence for each new approval.
- Bind the installed profile to a sealed experiment selection and a later declared holdout.
- Retain evidence on the approval row in the same transaction.

## Context

The experiment ledger retains the complete declared trial family and the selected
artifact. The approval transition needs to identify which experimental evidence and
holdout report the schema owner reviewed for an installed profile.

## Decision

The canonical promotion command carries the exact profile, family, selected trial,
canonical artifact digest, reviewed holdout report digest and holdout interval.
The schema owner submits it to the existing approval command. Under the existing
approval lock, reconstruct the sealed family from a single database snapshot.
Require the selected artifact to match scope, implementation, configuration,
lookback and result digest. The holdout start must be strictly later than every
declared trial decision. Its end must be strictly later than its start.

Store canonical evidence on the immutable approval row. Exact retries by the
same actor recover the original action. A changed report or selection conflicts.
Approval remains a separate owner decision from experiment selection.

## Alternatives considered

A separate evidence write would introduce an interval between retention and
approval. Storing both on the approval row makes the evidence and authorization
commit together. Applying statistical thresholds inside this command would require
a verified report format and evaluation methodology beyond the current ledger.

## Consequences

The report digest identifies an external report reviewed by the owner. It carries
an attestation to the declared holdout interval. Statistical validity, actual
experiment execution chronology, and the report's contents require separate
evidence and review.

## Compatibility and migration

Apply migration 0007 after the experiment ledger. Its check constraint preserves
existing approval rows with absent evidence and applies to subsequent inserts.
Older binaries can continue to read approvals and cannot insert new evidence-free
approvals. Keep the constraint and immutable rows on rollback. Historical receipt
recovery retains its previous approval interpretation.

## Security and operations

The authenticated private schema owner remains the approval authority. Keep
approval and ledger write grants restricted to that owner. The runtime validates
evidence locally without fetching the report or granting market-data, simulation,
or account authority.

## Validation

The PostgreSQL proofs cover missing, unsealed, mismatched and overlapping
evidence, rollback, exact retries and pre-migration approvals. The process proof
uses the existing command to deny a bare fresh approval and accept the versioned
promotion envelope before validating publication.
