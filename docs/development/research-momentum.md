# Deterministic raw-price momentum

## TL;DR

- M04-PR4 computes endpoint price return only; no ranking or strategy approval.
- Accept only the sealed raw USD series from the research evidence policy.
- Revision `raw-usd-roc-ppm-half-even-v1` rounds exact return to integer ppm.
- Run `cargo xtask verify --check research-momentum --python python3`.

## Formula and units

For a selected lookback of L trading-session intervals and L+1 complete closes:

`score_ppm = round_half_even((last_close / first_close - 1) * 1_000_000)`

The score is a dimensionless raw price return. A score of 100000 means 10 percent,
not 100000 percent. Negative and zero scores are valid results, not evidence-policy
abstentions. This is neither total return nor a dividend/split-adjusted calculation.
The qualified basis is raw USD per share; changing basis requires new qualification.
Intermediate closes establish complete lookback coverage but do not enter this
endpoint formula. The oldest/latest sessions and lookback are retained in output.

## Exact numerical behavior

The implementation reuses `UsdPrice`, which supports eight decimal places and
28 normalized coefficient digits. It aligns normalized coefficients back to exact
eight-place units with checked multiplication. Division uses six decimal long-
division steps, rather than multiplying the potentially large delta by one million.
Every remainder is less than the positive denominator. Even its tenfold value
fits the existing price bound. No floating point, decimal library or intermediate
rounding is used. The final remainder chooses nearest, ties to even; sign is applied
after rounding the magnitude so positive and negative ties behave symmetrically.

Maximum successful-score error is exactly half a ppm (0.0000005 return); many
ratios are exact. The error bound is absolute, not relative. A score outside signed
64-bit range or checked arithmetic capacity returns a payload-safe error, never
clamps, wraps, saturates or fabricates an abstention reason. The application will
map calculation failure separately from evidence eligibility in a later slice.
No numeric defaults are read from environment or locale. Configuration must pin
the formula revision before publication/replay; changing rounding requires review.

## Boundary and verification

[`momentum`](../../crates/research-domain/src/momentum.rs) accepts an
`EligibleSeries`, not raw DTOs or arbitrary prices. Owner authentication, retained
byte verification, rights and reference resolution remain application obligations;
see [research evidence](research-evidence.md). [ADR-0079](../decisions/0079-version-raw-price-momentum.md)
records the formula decision. No service, wire schema, database or provider changes.

The default protected `research-momentum` gate tests exact positive/zero/negative
vectors, symmetric half-ppm ties, equivalent scales, large valid intermediates,
wire range boundaries, session order/lookback and revision stability. An integer
rational oracle checks the half-ppm inequality for 18721 small price pairs without
floats, clocks, random data, dependencies or external sources. These are numerical
correctness proofs, not investment performance, backtest or strategy validation.
