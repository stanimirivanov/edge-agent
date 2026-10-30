# Isolated local strategy experiments

## TL;DR

- `offline-experiment` runs existing momentum calculations over owner-authorized evidence.
- Output is `research.experiment.v1`, explicitly `publication_eligible:false`.
- It opens no Research database, approval store or broker connection.

## Local command

Run the Research binary with `offline-experiment`, supplying an existing correlated
owner-ready Research evidence envelope on stdin and the existing owner-loader profile
settings described in [Research worker](research-worker.md). Input is capped at 262145
bytes and owner loading/evaluation at 25 seconds; the existing owner adapter additionally
bounds its child process and retained bytes. Scope, implementation/configuration,
rights, evidence integrity and chronology checks remain mandatory. Invalid, unavailable,
unauthorized or non-ready owner input fails with no stdout result.

Research database/schema and NATS settings are unused by this command. It needs no
installed or approved strategy row. Compiled availability and local execution grant
no approval, publication, simulation or market-data authority. The current slice uses
exact M04 profile bindings, consistent with [local governance](strategy-governance.md);
full PR2 bundle loading and another strategy remain later work.

## Result and publication boundary

The [pure application function](../../crates/research-application/src/experiment.rs)
uses the same deterministic evaluator and compiled momentum entry as normal Research.
For identical inputs its nested `result` record has the same numbers, ranking, evidence
and rendering. The outer envelope contains `kind`, `publication_eligible` and `result`,
with recursively canonical JSON keys and one final newline. The whole envelope is
bounded by the existing maximum Research result size plus its fixed wrapper.
Evidence-policy abstentions remain nested abstentions, never approvals.

`ExperimentalOutcome` exposes only the experimental bytes. It has no conversion to
`RenderedOutcome`, and `PreparedCompletion::new` cannot accept it. The entire output
also fails ordinary ResearchRecord and message-envelope decoding. A caller can inspect
or extract nested JSON for analysis; that does not grant publication authority. Keep
the envelope when saving results so their non-approved status remains visible.
No experiment identifier, ledger, promotion or durable trial registration is added;
M05-PR15 supplies retained experiment/selection records. See
[ADR-0093](../decisions/0093-isolate-experimental-research-results.md).

## Verification and rollback

Run `cargo test --locked -p edgeagent-research-application -p edgeagent-research` and
the full default gate. Pure tests compare normal and experimental results, reject
incompatible bindings/expiry and preserve evidence abstention. Actual command tests
use the existing invented owner-process fixture, with invalid Research database,
schema and broker settings, and prove output separation and safe failure. They do
not claim qualification of a live provider. A compile-fail doctest protects publication
type separation. No database migration, new dependency or deployment change is needed;
rollback removes the additive local command without affecting existing artifacts.
