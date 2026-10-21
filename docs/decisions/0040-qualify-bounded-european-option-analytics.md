# ADR-0040: Qualify bounded European option analytics

- Status: Accepted
- Date: 2026-10-21
- Milestone: M04 - Deterministic research
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

P11 supplies pure theoretical BSM premiums/Greeks and bounded implied volatility
for cash-settled European index options. Explicit inputs, point-in-time references,
independent numerical proofs and failure results precede any policy activation.
These estimates cannot substitute for quotes, exact ledger money or final fixings.

## Context

Quote-driven fills and cash settlement already have distinct exact accounting
boundaries. Research needs numerical sensitivities without importing a trading
runtime or allowing approximate theory to become financial authority. An IV
iteration limit must not silently return the last guess as a successful solve.

## Decision

Add synchronous analytics inside simulation-domain. Accept only the initial
cash/European/index shape, an effective and already-known exact specification,
injected evaluation time, canonical index points and retained evidence/assumption
digests. Derive positive remaining lifetime to contractual expiration using the
explicit ACT/365F convention, including subsecond time. This is not a trading
calendar, evidence freshness, entitlement or source-qualification gate.

Use Black–Scholes–Merton with explicit continuously compounded annual risk-free
rate, continuous yield and fractional annualized volatility. Premiums and Greeks
use quotation points, never USD contract totals or tick-rounded executable prices.
Theta measures elapsed years; vega and rho measure changes per 1.0 fractional
volatility/rate. Keep all values finite and reject lost time value at open model
bounds rather than clamping numerical failure to zero. Document the tested
computational envelope and tolerance; it is not a market-policy default.

Pin MIT libm 0.2.16 with default features disabled for erfc/exp/log/sqrt. The crate
is maintained in rust-lang/compiler-builtins; the earlier libm repository is
archived because development moved. Stable 1.98.1 lacks stable erfc. No upstream
pricing implementation is copied; the numerical library has no public API exposure.
Replacement belongs to the analytics module and requires rerunning its independent
qualification; do not enable nightly or architecture-specific library features.

IV uses an explicit fixed bracket, price tolerance, volatility-width tolerance,
minimum vega and 1–256 bisection iterations. Require both convergence criteria
and sufficient identification. Reject invalid premiums, unbracketed targets,
flat/boundary cases, numerical collapse and exhaustion. Do not expand brackets,
guess tolerances, invoke another model, or return an unqualified last iterate.

Canonical identities bind implementation, full specification/provenance, exact
input decimals, injected time, year basis, rates, evidence/assumption digests and,
for IV, exact target plus complete solver policy. Retain original artifact bytes
separately. A digest proves neither availability nor authority. Normalize signed
zero rates. Binary64 results replay within declared tolerance, not a promise of
cross-platform bitwise identity or certified interval arithmetic.

## Alternatives considered

- Handwritten normal-CDF approximations add avoidable numerical maintenance.
- An upstream trading/pricing runtime adds capabilities and licensing footprint
  beyond a pure calculation, and does not replace independent qualification.
- Newton-only inversion can leave the bracket or mistake low vega for convergence.
- Round-trip IV tests alone can reproduce a shared pricing defect.
- Reusing theoretical values as quotes or fixings weakens existing authority.

## Consequences

Independent Decimal/integral-series vectors, published reference prices, parity
and finite differences qualify finite normal/near-expiry cases. Extreme tails,
underflow and ill-conditioned prices may explicitly reject. No numerical success
enables Greek-dependent risk, calibrates a volatility surface, proves source
quality, or completes all M04. P12/P13 remain separate downstream work.

## Compatibility and migration

Add an API/module and one private numerical dependency; existing wire formats,
financial APIs, tables and stored account meaning are unchanged. No migration,
network call, public route or activation flag. Revert the API/dependency as one
unit before consumers adopt it; retained analytics identities must not be
reinterpreted under another implementation revision.

## Security and operations

Inputs/outputs and errors use payload-safe diagnostics. Work is synchronous and
bounded, with no clock, random state, external effects or credentials. Model
estimates do not grant market-data rights or permission to trade. Preserve all
P08–P10 financial invariants and the dry-run-only boundary.

## Validation

The default protected option-analytics-contracts check discovers all six named
proofs before executing them. Census mutation fixtures reject missing/renamed,
duplicate or extra proofs. Routes connect analytics and reference-generation
changes to that check. Run the default gate and applicable supply-chain checks.
Reference regeneration uses Python standard-library Decimal, not runtime libm.
