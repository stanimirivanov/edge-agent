# Synthetic cash option settlement

## TL;DR

- P10a calculates whole-position cash/European/index expiry with explicit rules.
- A synthetic final fixing is distinct from a quote, trade or ordinary index mark.
- Missing/future evidence never means zero payout or permission to close exposure.
- This pure API does not commit settlement; P10 account integration remains next.

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
Do not split a position to change rounding; future storage owns the whole-position
lock and identity. A repeated pure calculation repeats its result, not a durable
exactly-once guarantee.

## Integration and limits

No SQL, public endpoint, provider, source qualification, exercise discretion,
threshold/lapse policy, short options or delivery exists in this slice. All
fixtures are invented. Normalized finite input bounds and checked u128/u64
arithmetic reject amounts too large for the current accounting representation.
Only one explicitly declared final synthetic candidate is supported; corrections
and source finality protocols require later reviewed integration.

P10b must retain fixing artifacts in canonical run ordering, authenticate scope,
lock the account root, terminate affected outstanding orders/holds, close the
whole authoritative position once and commit account/run/risk continuations,
receipt, postings and outbox atomically. It must cover missing evidence,
duplicates/conflicts, rollback, process death, restart and lost ACK. Until then,
P09 option order expiry continues to leave its position open. Public activation,
provider/session qualification and all of M07 remain incomplete. Rollback of this
foundation is code-only: no schema or persisted account meaning changed.

## Verification

```powershell
cargo test --locked -p edgeagent-simulation-domain --test settlement
cargo test --locked -p edgeagent-simulation-domain --doc
cargo xtask verify
```

Default tests exercise deterministic financial behavior, redaction and compile-
time separation of marks from settlement. They claim no database or crash proof.
