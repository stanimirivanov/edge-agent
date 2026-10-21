# Theoretical European option analytics

## TL;DR

- P11 computes model premiums, delta/gamma/vega/theta/rho and bounded IV.
- Theoretical quotation points are not quotes, cash, fixings or risk authority.
- Rates, yield, ACT/365F time, evidence and solver policy have no defaults.
- Independent references qualify a finite envelope; failures stay typed failures.

Read [ADR-0040](../decisions/0040-qualify-bounded-european-option-analytics.md)
and the [simulation profiles](simulation-profiles.md) before consuming this API.

## Inputs and units

`AnalyticsInputs::new` takes an exact `ReferenceVersion<OptionSpecification>`,
injected `PreciseInstant`, `IndexPoints`, explicit `CarryRates`, `YearBasis` and
evidence digest. Only cash-settled European index options are supported. The
specification must be effective, known and activated at the supplied instant.
Last trading alone is not the valuation deadline; actual expiration bounds
positive remaining lifetime. ACT/365F divides exact elapsed nanoseconds by
365 * 86400 seconds. This does not classify 0DTE, qualify a session or infer a
trading-year basis. At/after expiration, use the separate settlement boundary.

Carry rates/yield are continuously compounded fractional annual values in [-1,1],
including negative rates. Volatility is fractional annual standard deviation in
(0,10]. Remaining lifetime is (0,100] years; spot and strike are positive and at
most 1e12 quotation points. These are computational limits, not business defaults
or promises of uniform relative precision across the envelope. Exact source
decimals convert deliberately to binary64 only inside this statistical model.
Extreme tails or degenerate intermediates can still fail closed.

Evidence and rate-assumption digests identify retained artifacts. Callers must
retain actual bytes and qualify instrument linkage, freshness, skew, rights,
rate maturity and historical availability before using the estimate. The pure
calculator cannot establish those facts from a digest. No provider is selected.

`estimate(AnnualVolatility)` returns an immutable `EuropeanEstimate`:

| Output | Convention |
|---|---|
| Premium | Option quotation points; continuous and unrounded |
| Delta | Premium-point change per one underlying index point |
| Gamma | Delta change per one underlying index point |
| Vega | Premium points per 1.0 fractional annualized volatility |
| Theta | Premium points per elapsed year, holding other inputs fixed |
| Rho | Premium points per 1.0 fractional annualized risk-free rate |

Divide vega/rho by 100 for a one-percentage-point sensitivity. Multiplier and
whole-contract quantity are **not** silently applied. There is no conversion to
`OptionPoints`/money, tick rounding, executable quote, fill, valuation freshness,
final fixing or account mutation. Model outputs cannot enable a Greek-based risk
policy without its separate evidence/policy qualification.

## Implied volatility and failures

`IvSolverPolicy::new` requires explicit ordered `AnnualVolatility` endpoints,
positive finite absolute premium-point and volatility-width tolerances, positive
minimum vega, and 1–256 iterations. There is no default or automatic bracket
expansion. The target is an exact canonical `OptionPoints` value, not an
unacknowledged midpoint/last-price choice; source side/selection belongs in the
retained evidence policy. Its decimal precision also limits inferred accuracy.

Discounted no-arbitrage bounds are strict for a positive-volatility, positive-time
continuous BSM model. Invalid targets yield `PremiumBounds`; targets within the
price tolerance of an open boundary yield `Unidentifiable`. Invalid/numerically
collapsed endpoint prices fail, not turn into a fabricated bracket. Otherwise
an uncovered target returns `NotBracketed`. Bisection succeeds only when **both**
the surviving bracket width and actual price residual meet policy, and vega meets
the minimum. Exhaustion/stagnation returns `NonConvergence`, never the last guess.
The result exposes bracket, residual, iterations, volatility and its same-input
Greeks. A numerical failure is not a request to relax policy or choose another model.

## Reference evidence and reproducibility

The implemented model follows the explicit carry/time conventions in the
[MathWorks pricing reference](https://www.mathworks.com/help/finance/blsprice.html)
and [theta reference](https://www.mathworks.com/help/finance/blstheta.html).
The implementation is original; no upstream pricing code is copied. The normal
CDF uses the [libm erfc API](https://docs.rs/libm/0.2.16/libm/fn.erfc.html),
which evaluates small tails directly rather than subtracting from one.

`scripts/generate_option_analytics_references.py` independently computes the
checked-in synthetic call/put vectors with 65-digit standard-library Decimal
and a normal-integral power series, not Rust/libm. Regenerate to stdout with:

```text
python scripts/generate_option_analytics_references.py
cargo xtask verify --check option-analytics-contracts
cargo xtask verify
```

Reference assertions allow 1e-8 absolute plus 2e-11 relative error per component;
finite differences have separate explicit truncation tolerances in tests. Cases
include nonzero/negative carry and one minute to expiry; nanosecond/low-vega cases
test explicit boundaries, not universal accuracy. Published four-decimal prices
add independent coarse checks. IV round trips supplement, never replace, these
references. No default test uses network, a provider, wall time, model or database.

Canonical input/result identities include the complete exact specification,
provenance, time, ACT/365F convention, retained evidence/assumptions and
implementation; IV also binds the exact target and full solver policy. Signed
zero rates normalize. Replay is tolerance-based binary64, not cross-platform
bitwise equivalence, correctly rounded interval bounds or investment validity.
Public diagnostics redact input/output values. P08–P10 settlement/fill/risk paths
remain unchanged; P11 adds no runtime/public activation and does not complete M04.
