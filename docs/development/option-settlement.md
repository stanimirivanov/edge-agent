# Synthetic cash option settlement

## TL;DR

- P10a calculates whole-position cash/European/index expiry with explicit rules.
- A synthetic final fixing is distinct from a quote, trade or ordinary index mark.
- Missing/future evidence never means zero payout or permission to close exposure.
- The pure API proposes effects; the account adapter commits them atomically.

Read [ADR-0038](../decisions/0038-bind-synthetic-cash-settlement-to-explicit-policy.md),
[simulation profiles](simulation-profiles.md) and [P09 accounting](evidence-execution.md).

## Inputs and results

`SettlementPolicy::new` requires an exact `ReferenceVersion<OptionSpecification>`,
semantic revision, synthetic source identity, exact fixing instant, inclusive
latest evidence-receipt instant, `SettlementRounding` and `SettlementCharges`.
There are no defaults. The fixing lies between activation and settlement; it need
not equal last trading or expiration. These structural checks do not establish
calendar membership or source rights. Cash/American, physical and non-index shapes
are rejected, not reinterpreted as the supported profile.

`SyntheticFinalFixing::new` records invented final evidence under that complete
policy, a logical observation ID, actual knowledge time and final index points.
Receipt must follow fixing and meet the explicit deadline. This artifact cannot
certify a provider; there is intentionally no constructor from an ordinary mark.
Retain canonical bytes, not redacted Debug output. Identity includes the exact
specification, reference provenance, timezone revision, policy and knowledge time.

`calculate` accepts injected evaluation time, authoritative whole-position
`PositionCost`, available **unreserved** cash and the complete candidate set.
Zero positions, ambiguity and policy mismatches fail. Before settlement it returns
`NotDue`; no fixing returns `MissingEvidence`; future-known evidence/reference
returns `NotKnown`. No such result contains a financial effect. Passing a later
evaluation time cannot renew evidence or silently change which fixing was chosen.

Calls and puts use exact intrinsic points times USD/point times whole contracts.
Round once across the whole position to USD cents using explicit Down, Up or
NearestEven (half-cent ties to an even cent). Fees remain separate: explicit
cents per contract apply to EveryContract or only InTheMoney before rounding.
Fee rates, including zero, are mandatory inputs, not suggested product settings.
If proceeds cannot fund charges, use the supplied unreserved cash or fail Funding.
Overflow fails closed rather than wrapping, saturating money or returning zero.

The calculated result exposes contracts closed, gross/fees, remaining available
cash, policy/evidence digests and balanced signed postings in P09 order: cash,
position cost, realized P/L expense, fee expense. It releases all supplied book
cost in candidate effects. It never changes the supplied position or commits cash.
Do not split a position to change rounding; the account adapter owns the whole-position
lock and identity. A repeated pure calculation repeats its result, not a durable
exactly-once guarantee.

## Integration and limits

The pure domain layer contains no SQL. This integration adds no public endpoint,
provider, source qualification, exercise discretion, threshold/lapse policy,
short options or physical delivery. All
fixtures are invented. Normalized finite input bounds and checked u128/u64
arithmetic reject amounts too large for the current accounting representation.
Only one explicitly declared final synthetic candidate is supported; corrections
and source finality protocols require later reviewed integration.

P10b implements the transaction in
[ADR-0039](../decisions/0039-commit-account-settlement-as-a-canonical-postlude.md).
`SettlementContext::new` appends a bounded canonical postlude to `ExecutionContext`.
Supply explicit order/allocation `SettlementLimits` (each 1..=32), contiguous steps,
exact policies and complete synthetic candidate sets. Provision using
`provision_settlement` after execution/book costs but before any admission. Retain
the full configuration; reconstructed contexts must match its bytes after restart.

The postlude is owner-authored input, not a new market feed or scheduler. It follows
the original finite schedule and frozen market inputs. P05 time/IDs continue from
the account checkpoint. Complete replay identity includes manifest, admission/
execution configuration and the postlude. Combined slots stay within 4096 and
postlude bytes within 1 MiB. Commands carry only account/plan/slot/revision.

Under the existing inbox transaction, acquire the root lock, check grants and
retained configuration, then return exact receipts before new revision checks.
Deferred operations advance account/run and risk observation without changing cash,
positions, order journals or holds. Successful settlement expires affected active
orders and releases their holds, closing that instrument across all strategies.
Other reservations protect cash. `calculate_account` requires known final evidence
even for an empty lot, without weakening the positive-position `calculate` API.

Round the account/instrument payoff once. The pure `SettlementCalculation::allocate`
attributes gross/fees by cumulative quantity proportion in strategy-ID order,
using differences of cumulative floors; the last attribution consumes the residue.
Release actual strategy costs, not a second rounded payoff. Commit the balanced
attributions, expiry revisions, receipt, terminal instrument seal, account/run/risk
continuations and outbox together. Later sealed-instrument steps record zero-effect
already-settled facts. Valuation timestamps/gross carry and risk latches are not reset.

Apply expand-only migration 0003 with the owner role. Runtime may SELECT the new
configuration and INSERT receipts/seals, not modify them, policies, funding or grants.
Facts have a 16 KiB serialized limit. Missing/corrupt costs or legacy journals,
unsupported units, exceeded work and unfunded fees fail closed. Rollback pauses
ingress and retains committed cash, journals, receipts and seals, never drops them.
P09 order expiry alone still leaves positions open. These proofs qualify finite
synthetic P10 correctness, not public/provider/session activation or all of M07.

## Verification

```bash
cargo test --locked -p edgeagent-simulation-domain --test settlement
cargo test --locked -p edgeagent-simulation-domain --doc
cargo xtask verify
cargo xtask verify --check account-admission-processes --check simulation-schema-contracts
cargo xtask verify --check sqlx-metadata
```

Default tests cover arithmetic, attributions, wire/schema checks, redaction and
compile-time mark/fixing separation. The protected process gate uses the documented
disposable PostgreSQL profile for concurrency, real worker death and replay.
Post-commit worker-result loss is not a new broker-wire ACK fault; no proof claims
provider finality, HA, power-loss durability or market fidelity.
