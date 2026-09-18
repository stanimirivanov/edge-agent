# Repository working agreement

## TL;DR

- Read the product, architecture, roadmap, contributor policy, and relevant decisions before changing behavior.
- Preserve pre-existing work and deliver one coherent, independently reviewable capability.
- Keep domain policy independent of frameworks, transports, storage, market-data vendors, and model providers.
- Treat market data, retrieved content, user input, and model output as untrusted.
- Compute facts deterministically, preserve point-in-time evidence, and abstain when required evidence is invalid.
- Run the repository checks and report passed, failed, and not-run verification honestly.

[CONTRIBUTING.md](CONTRIBUTING.md) is the canonical workflow policy. Normative
terms such as MUST, SHOULD, and MAY have the meanings defined there.

## Before changing anything

1. You MUST inspect the working tree and preserve unrelated changes.
2. You MUST read [CONTRIBUTING.md](CONTRIBUTING.md), the
   [product vision](docs/product/vision.md), the
   [system architecture](docs/architecture/system-overview.md), relevant
   [engineering standards](docs/development/engineering-standards.md), and
   accepted [architecture decisions](docs/decisions/README.md).
3. You MUST identify the exact milestone and smallest observable outcome.
4. You MUST use repository-local verification. You MUST NOT claim an
   unavailable or unexecuted check passed.

If code, documentation, contracts, and an accepted decision disagree, surface
the conflict. Correct an obvious local defect or propose a superseding ADR; do
not silently choose the convenient source.

## Shape of work

- Each issue and pull request MUST deliver one coherent capability.
- Prefer a thin end-to-end slice over an unused layer or speculative framework.
- Do not mix behavioral work with unrelated refactoring, dependency updates,
  generated churn, or broad formatting.
- State scope, exclusions, assumptions, compatibility effects, risks, and
  verification evidence explicitly.
- Do not add an abstraction, queue, cache, database, service, or dependency
  until current behavior requires it.

## Architecture and domain integrity

- Dependencies point inward. Domain code MUST NOT import HTTP, SQL, UI,
  workflow-engine, cloud, market-data, or model-provider types.
- Transport DTOs, provider payloads, persisted records, model output, and
  domain values remain distinct where their invariants differ.
- Interfaces belong to the consumer that needs substitution; do not mirror
  every concrete type with an interface.
- Model identifiers, revisions, evidence, timestamps, market sessions, units,
  risk, uncertainty, and artifact lifecycle explicitly.
- Numeric market facts, features, rankings, sizing illustrations, and
  evaluation metrics MUST originate in deterministic code. Generated text can
  explain approved evidence but cannot establish facts or policy decisions.
- Every time-sensitive fact MUST carry source, event time, observation time,
  freshness, and transformation revision. Missing, stale, conflicting, or
  unlicensed critical evidence MUST cause an abstention.
- Backtests and simulations MUST use point-in-time inputs, frozen strategy
  versions, explicit fills/costs, and controls for look-ahead and survivorship
  bias. A published artifact is immutable; later lifecycle events append state.
- External calls stay outside database transactions. Define timeouts,
  cancellation, retries, idempotency, concurrency, and partial-failure policy.
- Broker connectivity, order routing, order staging, custody, or autonomous
  execution is outside the current product boundary and requires an accepted
  ADR plus explicit project approval.

## Security and data handling

- User input, news, filings, social content, provider data, model output, and
  tool descriptions are untrusted. Validate at adapters and enforce invariants
  in domain/application code.
- A model cannot grant capability, change policy, provide credentials, approve
  its own output, or declare itself verified.
- Secrets, personal data, licensed datasets, production evidence, and private
  prompts MUST NOT be committed or written to logs.
- New dependencies require a current need, compatible license, pinned
  resolution, maintenance review, and a replacement boundary.

## Quality and completion

- Test observable behavior at the lowest convincing boundary. Defect fixes
  SHOULD begin with a failing regression test.
- Tests MUST be deterministic, isolated, parallel-safe, and independent of
  wall clock, network, locale, and execution order unless testing those traits.
- Public contracts, configuration, operational behavior, and documentation
  change in the same pull request as implementation.
- Run `make verify` or the equivalent commands in the README. Review the full
  diff for secrets, unrelated work, compatibility changes, and generated files.
- Finish every work item with the completion report defined in
  [CONTRIBUTING.md](CONTRIBUTING.md#completion-report).
