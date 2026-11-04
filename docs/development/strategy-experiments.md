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
## Experiment and selection ledger

Apply migration 0006 after the existing Research migrations in the private schema.
Run `edgeagent-research offline-experiment-ledger` with the loopback PostgreSQL URL,
Research schema and scope. Use authenticated schema-owner credentials. Supply one
canonical `research.experiment-ledger.v1` command on stdin, at most 262144 bytes.
The `data` object contains `scope`, `family` and `action`. The action object has
its own `action` discriminator. Use the contract's `Command::bytes` encoder for
sorted keys and a final newline.

| Action | Fields and result |
|---|---|
| `register` | `protocol` is the SHA-256 of the retained evaluation protocol. `trials` lists one to 64 attempts sorted by unique request ID. Each has the complete `request` and expected `evidence` census sorted by instrument and digest. |
| `finish` | `trial` names the registered request ID. `outcome` is either `status: result` with the entire experimental envelope in `result`, or `status: failed` with a `reason` of `invalid_input`, `unavailable` or `cancelled`. |
| `select` | `trial` names a retained artifact result after every declared attempt terminates. An abstention is terminal and cannot be selected. |
| `read` | Returns the family's canonical records as JSON lines in commit order. |

Writes echo the canonical record after commit. Retry the same bytes after losing a
response. Use a new family for a changed protocol or attempt list. Each declared
attempt has one terminal slot. Register separate IDs for separate runs, even when
testing the same parameters. The selected trial points to its exact retained result
within the sealed family.

This census is scoped to declared attempts and imported records. The operator
retains the protocol and accounts for trials performed outside the ledger.
Database commit order establishes registration before import. Run chronology
and statistical validation require separate evidence. See
[ADR-0104](../decisions/0104-retain-experiment-selection-evidence.md).

## Verification and rollback

Run `cargo test --locked -p edgeagent-research-application -p edgeagent-research` and
the full default gate. Pure tests compare normal and experimental results, reject
incompatible bindings/expiry and preserve evidence abstention. Actual command tests
use the existing invented owner-process fixture, with invalid Research database,
schema and broker settings, and prove output separation and safe failure. They do
not claim qualification of a live provider. A compile-fail doctest protects publication
type separation. Run the Research persistence and worker PostgreSQL gates for ledger concurrency,
rollback and CLI export. Preserve retained rows when rolling back the ledger command.
