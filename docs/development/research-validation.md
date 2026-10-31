# Independent proposal reconciliation

## TL;DR

- Validate bounded canonical proposals against a fresh governed evaluation of trusted inputs.
- Every authoritative field must match; proposals provide no facts, policy or approval.
- Only the private-constructor `ValidatedOutcome` enters the new typed preparation path.

## Inputs and guarantees

The [validator](../../crates/research-application/src/validation.rs) receives the original
admitted request, authenticated retained evidence, selected policy/intent, explicit expiry
and all required constraint inputs separately from untrusted proposal bytes. It decodes
the proposal with the existing size/canonical contract boundary, rejects request/lifecycle
records, then independently rebuilds the expected terminal outcome using existing qualified
kernels. It never extracts policy, evidence locations, expiry authority or approvals from
the proposal. Callers must authenticate those separate inputs and immutable configuration.

Compare exact canonical bytes, including request identities/revisions, ranking and tie-break,
scores, evidence identities/availability/census, expiry, renderer text and terminal kind.
An altered integer score is not accepted within a new ±1 ppm band: the authoritative
integer already contains qualified ties-even rounding and its half-ppm calculation bound.
Reconciliation neither repairs nor silently normalizes a proposal. Independent evaluation
reuses qualified formulas rather than maintaining a second drifting numerical implementation.
This does not independently authenticate or reimplement the owner's calculations.

Normal, narrowed and correct abstention proposals can validate. Missing selected constraint
inputs and failed evidence cannot certify a proposed artifact: rebuilding produces the
appropriate abstention or an error. A validated abstention remains an abstention, not an
approval. Evidence abstentions expose no apparent eligibility decision. Errors preserve
safe typed causes and never print proposal bytes or expected/actual diffs.

## Publication boundary and staging

A successful check returns a private-constructor `ValidatedOutcome`, exposing immutable
record, bytes, digest and recomputed decision. Its internal rendered result is available
only inside the application crate. `PreparedCompletion::from_validated` accepts this type
and retains existing exact delivery/request/route checks. Ordinary records and experimental
results cannot enter that typed constructor. Validator revision is
`reconcile-governed-research-exact-v1`, for later provenance binding.

The legacy constructor remains for compatibility/receipt handling but cannot prepare
a fresh store commit. [Validated publication](validated-publication.md) requires the
complete selected configuration, sealed ValidatedExecution, mandatory decision retention
and current owner approval. Experimental output still cannot enter that typed path.

## Verification and rollback

Run `cargo test --locked -p edgeagent-research-application` and `make verify PYTHON=python3`.
Mutation tests use canonical but incorrect records to distinguish reconciliation from
shape validation, plus malformed/oversize/unknown-field and reordered-tie cases. Tests
cover narrowed outcomes, real evidence abstentions, missing selected inputs, safe typed
causes, exact bytes/digest and foreign-delivery refusal. A compile-fail doctest protects
the typed constructor. No dependency, schema, store, provider or runtime deployment changes;
rollback removes the additive API without modifying retained records.

## M05 mandatory execution

[Validated publication](validated-publication.md) supersedes the staged activation
boundary: new completions require complete canonical configuration, sealed validation,
current exact approval and atomic companion retention/publication. Historical receipts
keep explicit legacy absence. Upgrade projector/Gateway metadata readers and add the
decision consumer subject before activation; retained outcomes remain byte-identical.
