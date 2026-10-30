# Immutable strategy and policy configuration

## TL;DR

- `research.configuration.v1` binds explicit strategy requirements and independent policy constraints.
- Use `Configuration::decode` for exact bounded canonical bytes; a digest is identity, not approval.
- This foundation does not activate configuration loading or change M04 execution.

## Meaning and compatibility

The [contract](../../crates/contracts/src/strategy.rs) declares implementation digest,
formula/eligibility revisions, complete required features, lookback, holding horizon,
TTL, benchmark, regime and invalidation specifications. Lookback counts trading-session
intervals; horizon counts holding sessions; TTL counts milliseconds after decision.
They are distinct. Compatibility explicitly pins Research v1 and raw USD return ppm
ranking descending with instrument-ascending ties. No existing request/artifact changes.

Policy revision is a separate immutable specification digest. The policy selects
instruments, long-only direction, horizon ceiling, evidence age, liquidity/volatility/gap
specifications and limits, event windows and quality revision. Formula identifiers
refer to separately retained specifications; decoding does not establish that they
are implemented, licensed or approved. A future loader must resolve exact supported
revisions, authenticate configuration bytes and verify their relationship to the
compiled implementation. It must reject unknown revisions rather than infer formulas.

Every v1 policy requires closes, OHLC, volume and event coverage, listed once in
canonical enum order. Missing features reject. Thresholds have no defaults: explicit
zero is a real boundary, never a disable switch. Liquidity uses whole USD cents;
volatility and absolute gaps use dimensionless ppm under the pinned specifications.
Event windows are milliseconds before/after decision, requiring affirmative coverage;
no event assertion or benchmark observation is supplied by this configuration.
Fixture formula names are invented declarations, not qualified runtime profiles.

## Identity and trust

Canonical bytes use recursively sorted JSON keys and one final LF, limited to 16 KiB.
Unknown fields at every level, versions, enum values, duplicate keys, unsorted or
repeated selections, invalid bounds and alternate encodings reject. SHA-256 covers
all validated bytes and excludes mutable approval/lifecycle state. Constructed or
Serde-decoded DTOs must pass `canonical_bytes`; raw input uses `decode`.

Request configuration digests can later identify these exact bytes. This PR does not
reinterpret existing digests or install new configurations. Approval, evidence owner
rights, runtime formula qualification, policy enforcement, publication and account
risk remain independent obligations. See [ADR-0091](../decisions/0091-define-immutable-strategy-policy-bundles.md).

## Verification

Run `cargo test --locked -p edgeagent-contracts --test strategy` and `make verify
PYTHON=python3`. The frozen invented bundle tests identity, strict nested parsing,
compatibility, bounds, required features, missing explicit limits and safe errors.
No database, provider or migration is required; reverting adds no state transition.
