# Engineering standards

## TL;DR

- Preserve domain boundaries instead of maximizing layers, interfaces, or patterns.
- Represent evidence, market time, units, risk, uncertainty, versions, and lifecycle explicitly.
- Keep deterministic analytics and policy outside generative models.
- Bound external work and define timeout, cancellation, retry, idempotency, and degradation behavior.
- Assume at-least-once message delivery and make durable state transitions replay-safe.
- Keep live execution structurally absent while testing dry-run orders and ledgers as first-class domain behavior.
- Test observable behavior and important failures deterministically without paid or mutable services in default CI.
- Treat contracts, prompts, schemas, persisted records, and evaluation fixtures as versioned compatibility boundaries.
- Pin tools and dependencies in repository configuration and run the same checks locally and in CI.

## Policy strength

Normative terms use the meanings defined in
[CONTRIBUTING.md](../../CONTRIBUTING.md#policy-language-and-sources-of-truth).
MUST and MUST NOT are requirements; SHOULD and SHOULD NOT are strong defaults
whose deviations require a recorded reason; MAY identifies an option.

These standards apply to production code, tests, scripts, generated bindings,
data transformations, evaluation, and operational tooling. Rust-specific
tooling becomes normative when the workspace ADR is accepted.

## Architecture

### Capability-oriented boundaries

Organize code around capabilities and ownership:

- **Domain code** owns research vocabulary, value objects, invariants, state
  transitions, scoring semantics, evidence rules, and policies. It imports no
  HTTP, SQL, UI, cloud, queue, model-provider, or market-data SDK types.
- **Application code** coordinates use cases, authorization, policy, units of
  work, and consumer-owned ports. It does not parse vendor wire formats.
- **Inbound adapters** validate protocol shape and translate requests, CLI
  input, events, or scheduled work into application commands.
- **Outbound adapters** implement application-owned ports and translate domain
  values to persistence, provider, model, or notification representations.

Keep transport DTOs, provider payloads, model output, persisted records, event
envelopes, and domain values distinct when their invariants or evolution differ.
A capability owns its writes. Other capabilities use its public application
surface or versioned events, not its tables or private types.

### Rust workspace and deployables

- The workspace pins a stable toolchain, uses Rust 2024, and commits `Cargo.lock`.
- Reusable crates own domain, application, contracts, infrastructure adapters,
  and test support. Deployable crates contain composition roots and runtime wiring.
- Workspace dependency declarations centralize versions; crate features remain
  additive and do not silently change domain behavior.
- Production code MUST NOT use `unsafe` without an accepted ADR, a documented
  invariant, focused tests, and reviewer approval.
- `unwrap`, `expect`, and `panic!` are prohibited on recoverable production
  paths. Process termination is reserved for invalid startup configuration or
  violated invariants that cannot be isolated safely.
- Public types use domain-specific newtypes rather than interchangeable strings
  or primitives for identifiers, money, quantity, timestamps, and versions.
- Binaries handle termination signals, stop intake, settle or release messages,
  close resources, and flush telemetry within the declared grace period.
- Each deployable builds into a minimal non-root OCI image from the same pinned
  workspace and exposes documented health behavior.

### Abstraction discipline

- Prefer cohesive modules, small stable APIs, composition, and explicit ownership.
- Introduce a port when a consumer requires substitution or isolation, not to mirror every class.
- Introduce a strategy abstraction only after at least two real behaviors establish the variation.
- Avoid generic repositories, global mutable state, service locators, boolean-driven state machines, and framework objects in the domain.
- Do not add queues, caches, databases, workers, or services without current behavior that needs them.
- Make invalid states difficult to construct with validated values and explicit state transitions.

### Data, time, and numeric meaning

- Model identifiers, revisions, evidence hashes, confidence, risk, durations,
  timestamps, quantities, prices, returns, and currencies explicitly.
- Decimal and floating-point choices MUST be deliberate. Exact monetary values
  use decimal semantics; statistical calculations document precision and tolerance.
- Store instants in UTC while preserving exchange timezone and market session.
  Distinguish event time, provider observation time, ingestion time, decision
  time, publication time, and persistence time.
- Preserve raw source identity and transformation revision for derived values.
- Inject clocks, identifiers, randomness, filesystem roots, and external clients
  whenever behavior or tests depend on them.
- Prefer immutable values. A published research artifact cannot be edited;
  invalidation, expiry, withdrawal, and correction are appended events.

### Errors and resource ownership

- Distinguish invalid input, unsupported requests, abstention, policy rejection,
  stale/conflicting evidence, unavailable dependency, timeout, cancellation,
  conflict, and internal defect.
- Public errors MUST NOT expose provider internals, credentials, stack traces,
  prompts, or licensed payloads.
- Preserve causes when wrapping failures.
- Bound input, output, collection size, symbols per request, lookback windows,
  concurrency, retries, model tokens, execution time, and memory-heavy work.
- External calls remain outside database transactions.
- Exact retry requires stable identity plus immutable-content comparison; a key
  match alone does not prove equivalent work.

### Concurrency and background work

- Keep domain logic synchronous and deterministic where practical; introduce
  asynchronous I/O and concurrency only at application and adapter boundaries.
- Child work belongs to a request, job, or service lifecycle and cancels with it.
- Never create unbounded tasks, threads, workers, queues, or retries.
- Document ordering, ownership transfer, cancellation priority, retry safety,
  delivery guarantees, deduplication, terminal failure, and operator recovery.
- Do not coordinate tests with sleeps; use controlled clocks, barriers, events,
  channels, or observable state.

### Event processing

- Commands name requested intent and have one owning consumer. Events use past
  tense and describe facts that have already committed.
- Messages use a CloudEvents-compatible envelope and a separately versioned
  payload with message, correlation, causation, aggregate, schema, producer,
  event-time, idempotency, and trace fields.
- Delivery is at least once. Never describe broker delivery or business state
  as exactly once.
- A producer uses a transactional outbox when a database transition and
  publication must agree. A stateful consumer records inbox identity and domain
  transition atomically.
- Ordering is guaranteed only for a declared partition key. Consumers reject,
  defer, rebuild, or quarantine unexpected aggregate versions explicitly.
- Retries are bounded, classified, and observable. Permanent, authorization,
  schema, and invariant failures enter a quarantined stream with operator-safe
  inspection and replay.
- Contract tests run against the portable broker adapter and every claimed
  managed-cloud adapter.

## Market-data and evidence boundary

- Provider adapters return one canonical schema with instrument identity,
  venue, currency, event time, observation time, source, freshness, adjustment
  state, entitlement class, and quality flags.
- Symbol strings are not stable identity. Corporate actions, venue changes,
  symbol reuse, mergers, and delistings require reference-data handling.
- Raw and adjusted price series are never mixed implicitly. The selected basis
  and adjustment revision are part of the calculation contract.
- Missing bars, market holidays, halts, crossed quotes, outliers, stale values,
  and provider disagreement are explicit outcomes.
- A fallback provider must satisfy the same semantic contract. Availability
  alone does not make two sources substitutable.
- Fixtures use synthetic or redistribution-safe data. Do not commit licensed
  payloads or imply that an unofficial source is production-safe.

## Model and retrieval boundary

- User input, retrieved text, tool output, provider metadata, and model output
  enter as untrusted data.
- System policy, capability grants, tool allowlists, and publication approval
  are deterministic and unavailable for model modification.
- Models do not calculate authoritative prices, indicators, ranks, risk, sizes,
  or evaluation metrics.
- Model output is parsed into a versioned schema and validated against approved
  evidence. Unknown fields and uncited numeric claims fail validation.
- Prompts, model settings, tool schemas, model identity, and validation policy
  are pinned for replay and evaluation.
- Fallback behavior is explicit: another qualified model, deterministic
  rendering, or abstention. A model outage never bypasses evidence policy.

## Strategy and evaluation integrity

- A strategy version declares eligibility, required features, parameters,
  ranking semantics, benchmark, expected horizon, expiry, and validation state.
- Research and evaluation code share audited calculations where practical but
  never share future information.
- Evaluation uses chronological partitions, point-in-time universes and
  fundamentals, delisting coverage, corporate actions, market calendars,
  declared fill rules, spreads, costs, slippage, and liquidity constraints.
- Freeze inputs and versions before evaluating a holdout. Multiple experiments
  and selection criteria are recorded; cherry-picked results are defects.
- Report sample size, uncertainty, regime breakdown, baseline comparison, and
  maximum drawdown. Win rate alone is not sufficient evidence.
- Live outcome tracking appends to the artifact ledger and does not rewrite the
  original request, thesis, levels, timestamp, or expiry.

## Dry-run execution integrity

- The simulator accepts only the literal mode `dry_run`; unknown or missing
  modes fail before a command reaches execution state.
- No execution-simulator crate, image, manifest, or runtime identity may contain
  a broker credential, live endpoint, live mode, or broker-adapter dependency.
- Order state transitions are explicit and append-only. Cancellation, expiry,
  rejection, partial fill, and fill races have deterministic resolution rules.
- Market and limit fills pin evidence plus versions for spread, slippage, fees,
  latency, liquidity participation, sessions, halts, gaps, and corporate actions.
- Order acceptance, fill, cash, position, and P&L changes preserve transactional
  invariants and are idempotent under command and observation redelivery.
- Monetary ledgers balance exactly. Statistical mark-to-market calculations
  declare precision, rounding, price source, and observation time.
- Simulation output is clearly labeled and cannot be presented as a live fill
  or broker confirmation.

## Contracts and compatibility

Treat HTTP schemas, event envelopes and payloads, CLI behavior, configuration, database migrations,
provider-normalization contracts, research artifacts, strategy definitions,
prompts, model/tool schemas, and evaluation fixtures as compatibility boundaries.

- Version semantics, not filenames alone. Never change a published field's meaning in place.
- Additive changes define default and old-reader behavior.
- Breaking changes require a new version or expand/migrate/contract plan,
  compatibility tests, and an ADR.
- Commands express requested intent; events describe completed facts in past tense.
- Explainable decisions pin exact source, contract, strategy, policy, model,
  adapter, environment, and evidence revisions.
- Generated output is reproducible and committed only when consumers cannot reasonably generate it.

## Testing

- Test behavior at the lowest boundary that proves it: pure domain tests,
  application tests with fakes, adapter integration tests, contract tests, and
  a small number of end-to-end tests.
- Test names state the condition and observable result.
- Defect fixes include a regression test that fails before the fix.
- Cover success plus relevant rejection, duplicate, stale/conflicting evidence,
  authorization, timeout, retry, cancellation, partial failure, and recovery.
- Use controlled clocks and synthetic fixtures. Default CI must not require a
  network, paid provider, credentials, or externally mutable market data.
- Gate live-provider tests explicitly and prevent them from publishing or trading.
- Inject duplicate, delayed, reordered, acknowledgement-loss, broker-restart,
  consumer-restart, and poison messages into event integration tests.
- Test dry-run execution with property-based ledger invariants and deterministic
  replay; assert that every non-dry-run mode is rejected.
- Never weaken assertions, add blind retries, or extend timeouts without diagnosing the cause.
- Coverage reveals unexamined code but does not prove correctness.

## Security, privacy, and operations

- Deny by default; authentication identifies while authorization controls each protected use case and data boundary.
- Secrets come from an approved secret mechanism and are never committed defaults.
- Do not log credentials, authorization headers, personal data, raw private prompts, or licensed payloads.
- Expose structured logs, traces, and bounded-cardinality metrics sufficient to replay and diagnose decisions.
- Correlation identifiers are not authorization grants.
- Dependencies need a current use, compatible license, maintained release,
  pinned resolution, proportionate review, and replacement path.
- New operational behavior documents healthy signals, failure classes,
  degradation, recovery, rollout, and rollback.

## Documentation

Public APIs document purpose, invariants, inputs, outputs, units, ownership,
side effects, concurrency, security expectations, and caller-actionable failures.
Comments explain decisions and constraints, not syntax. Keep tutorials and
operational procedures in documentation. Long documents contain a TL;DR under
the policy in [CONTRIBUTING.md](../../CONTRIBUTING.md#documentation).
