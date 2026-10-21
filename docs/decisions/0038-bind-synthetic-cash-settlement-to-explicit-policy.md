# ADR-0038: Bind synthetic cash settlement to explicit policy

- Status: Accepted
- Date: 2026-10-21
- Milestone: M07 - Dry-run execution
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- P10 begins with a pure calculation contract, not an activated account writer.
- Require explicit synthetic final evidence, fixing/receipt bounds, fees and rounding.
- Preserve exposure on missing evidence; calculate balanced whole-position effects.
- Durable P10 integration and source/session qualification remain outstanding.

## Context

Order expiry in P09 closes an order, not its option position. An ordinary index
mark or quote cannot establish the final settlement value. Product-specific
fixing times, receipt allowances, charges and rounding are not known defaults.
Calculation and durable commitment have different ownership and recovery duties.

## Decision

Add a synchronous `simulation-domain::settlement` module for the cash/European/
index shape already checked by P02. A policy owns the full exact option reference,
source, semantic revision, explicit fixing instant, inclusive latest receipt,
whole-position cent rounding and per-contract charges. No `Default` exists.

Only `SyntheticFinalFixing` can enter calculation. Its versioned artifact binds
the complete policy, logical identity, positive final index level and actual
knowledge time. This is an invented scenario declaration, not provider finality,
rights, continuity or session certification. Ordinary canonical index observations
and option quotes do not implicitly convert. Multiple candidates fail closed,
even equal ones: no hidden revision selection, fallback or deduplication occurs.

Before contractual settlement return `NotDue`; absent evidence returns
`MissingEvidence`; unavailable evidence/reference returns `NotKnown`. These
results and errors carry no postings. A late evaluation can use a retained final
fixing received by its explicit deadline; replay does not renew a market price.

Calculate `max(index - strike, 0)` for calls and `max(strike - index, 0)` for puts.
Multiply exact points, contractual USD/point and whole contract count; retain
10^-14-cent precision and round once over the entire supplied position. Require
explicit Down, Up or NearestEven rounding. Fees are exact per-contract cents
under either EveryContract or InTheMoney (positive intrinsic before rounding).
Zero fees must be supplied explicitly. Unsupported numeric magnitude fails.

Release the entire supplied weighted-average book cost in the calculated result.
Cash, position-cost, realized-P/L-expense and fee-expense postings balance exactly,
using P09 conventions. If charges exceed proceeds, require sufficient supplied
unreserved cash; reject overflow and unfunded charges without mutation. There is
no discretionary exercise, threshold/lapse rule, short position or partial-lot
settlement. The caller must provide the complete authoritative position.

## Alternatives considered

- Reusing a quote/ordinary index mark would invent settlement-source authority.
- Per-contract rounding changes payout when the same position is split.
- Assumed free exercise or implicit rounding embeds unapproved product rules.
- Implementing storage and source activation in this foundation would obscure
  the financial algorithm and exceed a focused review boundary.

## Consequences

This reusable calculation serves the P10 durable integration and P13 frozen-run
qualification. It neither mutates positions nor releases reservations, creates
receipts or guarantees exactly one effect. A calculated value is not a settlement
confirmation. P10 remains incomplete until the transaction path is qualified.

## Compatibility and migration

Additive Rust APIs only; no dependency, wire schema, SQL migration or deployable
change. Canonical artifacts have explicit synthetic/versioned prefixes and bind
all specification, timezone, policy and evidence fields. Future semantic changes
need versioning rather than silent reinterpretation. Existing ADRs remain intact.

## Security and operations

No network, clock, provider/model calls, entropy, broker or live mode exists.
Financial/evidence diagnostics are redacted. Future durable composition must bind
authenticated scope, consume retained fixing/policy artifacts through canonical
availability order, lock the P08 account root, resolve all outstanding order holds,
close each position once, and commit balanced postings, account/run/risk state,
receipt and outbox together before ACK. It must not reduce risk from guessed marks
or use calculation alone as idempotency. Source corrections need a separately
reviewed policy; this foundation selects no revision and rewrites no history.

## Validation

Run `cargo xtask verify` and the focused domain settlement tests. Tests cover
calls/puts, ITM/ATM/OTM, aggregate/tie rounding, fee funding, knowledge/contract
deadlines, ambiguity, unsupported profiles, overflow, canonical bindings and
redaction. A compile-fail doctest rejects ordinary index marks as final evidence.
The independent ADR high-water sensor retains this decision. No process crash,
database atomicity or provider qualification is claimed by pure tests.
