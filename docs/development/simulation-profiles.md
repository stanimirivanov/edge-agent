# Research and simulation profiles

## TL;DR

- Preserve `us-equity-daily-v1`; add separately versioned intraday and options
  profiles without changing retained daily evidence.
- Vendor selection is deferred. Synthetic fixtures are the default; live market
  data requires capability conformance and verified rights, not live execution.
- Model cash/physical settlement and European/American exercise. The first
  options simulator will accept only fully funded long cash-settled European
  index-style options under a qualified specification.
- ExecutionSimulator will serialize account-wide and per-strategy risk in one
  PostgreSQL transaction boundary. Broker partitioning is not that guarantee.
- These are approved requirements, not implemented or qualified capabilities.

## Authority and implementation status

[ADR-0032](../decisions/0032-define-vendor-neutral-simulation-profiles.md) replaces
the earlier vendor choice and options exclusion while retaining their safety and
evidence requirements. [Product vision](../product/vision.md) owns scope;
[milestones](../roadmap/milestones.md) own outcomes and the
[delivery plan](plans/simulation-expansion.md) owns P01-P13 sequencing.
Normative terms follow [CONTRIBUTING](../../CONTRIBUTING.md).

The current domain implements checked daily values and versioned mappings,
calendar days and action annotations. Product service binaries remain bootstrap
entry points. No profile below claims a working acquisition, risk, fill, pricing
or settlement runtime. Profile identifiers are reserved design vocabulary, not
new CLI/configuration values or published wire schemas.

## Evidence profiles and vendor boundaries

| Profile | Intended evidence | Boundary and acceptance prerequisite |
|---|---|---|
| `us-equity-daily-v1` | Existing raw USD equity/ETF provider-day OHLCV | Keep the [daily profile](market-data.md)'s universe, exact numeric bounds, causal timestamps, point-in-time references, quality, rights, retention and acquisition ceilings. Intraday/options remain excluded from this profile. |
| `us-underlying-intraday-v1` | Tradable underlying bars and trades; separately identified index-level observations where the underlying is a cash index | Preserve event and receipt/availability time, source precision, venue/session, condition/correction semantics and source sequence where meaningful. An index value is not a trade; ETF/futures proxies need an explicit separate basis and cannot silently substitute. |
| `us-option-quotes-v1` | Bid/ask quote snapshots linked to versioned option and underlying specifications | Pin both sides' timestamps, units and optional displayed sizes. Reject crossed, stale, inconsistent or unqualified snapshots. Missing size does not mean unlimited liquidity; snapshots do not establish queue position or depth. |

Provider-neutral application ports declare capabilities, not vendor names. A
provider may satisfy only a subset; different qualified providers may supply
underlying, option or settlement observations. A substitute must meet the same
semantic and rights contract. No automatic cross-provider failover is implied.
Vendor SDKs and DTOs stay in adapters; source IDs remain in evidence/manifests.

Before intraday/options activation, the profile MUST pin explicit finite bounds
for quote age, cross-input skew, refresh/calls, bytes, concurrency, recovery work
and retention, with applicable rights and account limits. They are unresolved
qualification inputs, not permissive defaults: do not inherit the daily
12-hour grace or silently relax its request budget. Unknown limits deny use.
Underlying identity and authoritative index/settlement feeds also remain required
product/provider qualification decisions. No provider, subscription or access
right is selected by P01.

## Instrument modeling versus simulation eligibility

P02 must distinguish stable underlying identity, call/put, strike, currency,
contract quantity, multiplier, tick rules and specification revision. Shares and
contracts are different units. Zero quotes/payoffs and signed ledger movements
must not be forced into a positive-only price type. Model last trading, expiry,
exercise cutoff and settlement timing separately under versioned session rules;
0DTE is a contract-local date relationship, not a fixed UTC duration or market-close guess.
Physical settlement requires a versioned deliverable specification, not an
assumption that every contract delivers 100 shares.

| Settlement | Exercise | Modeling requirement | First options simulation |
|---|---|---|---|
| Cash | European | Explicit specification and settlement observation | Eligible only after qualification; fully funded long positions |
| Cash | American | Represent the exercise style without reinterpreting it | Reject until exercise behavior is separately implemented |
| Physical | European | Represent deliverables and settlement timing | Reject until delivery/accounting is separately implemented |
| Physical | American | Represent exercise, deliverables and timing | Reject until exercise/assignment/delivery is separately implemented |

The planned `equity-long-dry-run-v1` retains the first equity/ETF market/limit
scenario. The planned `index-option-cash-european-dry-run-v1` adds the qualified
cash-settled European case, including 0DTE. An enum/specification being modeled
does not grant simulation eligibility. Short options, borrowing, leverage and
portfolio-margin simulation remain excluded; indicative margin metadata is not
an implemented margin engine. All other modes remain unsupported.

## Deterministic execution and account consistency

ExecutionSimulator remains the sole authoritative account writer. P08 will use
the existing inbox transaction, lock the account root before authoritative risk
reads, and hold that lock through account/strategy checks, reservations, order
state, balanced ledger updates, account revision and outbox commit. Every
account mutation uses that boundary; ACK follows commit. Account-wide limits
aggregate all strategies and replicas, with strategy limits checked on top.
Pending cancel retains reservations until the corresponding terminal effect.
Exact retries return recorded outcomes; identity/content conflicts fail.

This requires one fenced writable PostgreSQL primary, not active-active
multi-cloud account writes. Projections are revisioned read models, never risk
authority. NATS partition keys and row-lock arrival order establish neither
FIFO processing nor deterministic simulation order. P05/P08 must persist a
canonical availability-time/source-sequence/tie-break input order, reject or
defer gaps, and pin clock, policies, evidence, deterministic IDs and RNG state.
Replay cannot reveal evidence before its recorded knowledge time.

P07-P10 must introduce compatible run/account/strategy/order identities,
account/input/order revisions, reservation outcomes, risk reasons, session IDs
and evidence/specification/model references through the existing schema registry.
Do not invent these as current wire fields. Provider/venue capability seams may
support substitution, but no broker adapter, SDK, credential, endpoint, generic
live command or hidden live mode belongs in this implementation.

Fill/latency models use available bars, trades and quotes with explicit bounded
liquidity assumptions; they cannot infer an intrabar path or queue position.
Persist seeds and randomness continuation. Theoretical valuation cannot replace
an executable quote. Cash settlement requires a separately identified,
authoritative or explicitly synthetic settlement observation and exact rounding
policy; missing evidence defers/fails settlement without inventing a zero payoff
or releasing exposure. American exercise and physical delivery never fall back
to the European cash algorithm.

## Source reuse and qualification gates

The reviewed [Nautilus LICENSE](https://github.com/nautechsystems/nautilus_trader/blob/8705495b380f9b1569cf048d1c0f664707a67a72/LICENSE)
is LGPL-3.0: use design/ideas only, no code copy in this plan. The reviewed
[rust-finance LICENSE](https://github.com/Ashutosh0x/rust-finance/blob/df13eb8dd8e18481790f3417f570d4380cee7764/LICENSE)
is MIT: future reuse requires notice retention, per-file provenance and
dependency/license review. Permission is not quality certification. Reimplement
[`DailyLossLimit`](https://github.com/Ashutosh0x/rust-finance/blob/df13eb8dd8e18481790f3417f570d4380cee7764/crates/risk/src/daily_loss_limit.rs)
with session identity/exact money; do not copy
[`bsm::implied_vol`](https://github.com/Ashutosh0x/rust-finance/blob/df13eb8dd8e18481790f3417f570d4380cee7764/crates/pricing/src/bsm.rs)'s
non-convergence-as-success behavior. No upstream code or new dependency is included here.

| Gate | Required observable acceptance | Owning work |
|---|---|---|
| Evidence | Identical canonical semantics across synthetic and qualified adapters; stale/crossed/missing or required-but-unknown continuity fails explicitly. Snapshot-only capability does not promise a continuous feed | M03, P02-P04 |
| Reproducibility | Same retained inputs, availability order, versions and random state reproduce IDs and domain results | M04, P05 |
| Account safety | Competing strategies/processes cannot overspend; duplicate, rollback, restart and ACK-loss cases preserve one effect | M05/M07, P06-P09 |
| Options lifecycle | Exact ITM/ATM/OTM cash flows, including zero payoff; missing settlement evidence and unsupported combinations cannot fabricate completion | M07, P10 |
| Analytics | Finite-input, convergence and near-expiry reference cases pass before any Greek/IV-dependent policy is enabled | M04, P11 |
| Audit/evaluation | Rebuild projections without duplicate effects; publish frozen manifests and cost/latency/liquidity sensitivity, not return guarantees | M08, P12-P13 |

P01 runs `cargo xtask verify` for document/governance checks only. Later PRs must
add executable domain, adapter and protected account-process proofs to the
existing harness; M02 proof discovery is not account qualification. M09/M10
must qualify primary fencing/failover, restore and resource limits. Default CI
uses invented evidence and no secrets. M03's existing qualified-live-daily-data
completion requirement remains; it does not qualify intraday/options profiles.
Live data access never authorizes live order execution.
