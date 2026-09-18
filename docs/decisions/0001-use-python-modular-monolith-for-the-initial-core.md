# ADR-0001: Use a Python modular monolith for the initial core

- Status: Proposed
- Date: 2026-09-18
- Milestone: M01 - Engineering foundation
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Use CPython 3.13 as the minimum runtime for the initial deterministic research core.
- Package the core from `src/edge_agent` and keep tests in `tests`.
- Begin as one modular deployable with capability boundaries enforced in code.
- Keep the domain and application layers independent of web, database, model, and market-data frameworks.
- Introduce an API framework, persistence engine, queue, and UI only when a vertical slice requires them.
- This ADR remains **Proposed** until maintainers approve or revise it.

## Context

The next milestone needs an executable foundation for explicit domain values,
point-in-time evidence validation, deterministic analytics, strategy ranking,
policy, artifacts, and replay. The runtime must support numerical and data work,
strict typing, fast tests, broad contributor accessibility, and later integration
with market-data and model providers.

The system does not yet need a distributed topology, database, web framework,
queue, or model SDK. Selecting those now would create dependencies before their
failure, compatibility, and operational requirements are known.

Python 3.13 remains in its regular bugfix period through approximately October
2026 and receives security fixes through approximately October 2029, providing
a reasonable support window for an early-stage open-source project. The
scientific Python ecosystem also aligns with deterministic feature calculation,
evaluation, and market-data normalization. See the official
[Python 3.13 release schedule](https://peps.python.org/pep-0719/).

## Decision

If accepted, EdgeAgent will:

1. Use CPython 3.13 as the minimum supported application runtime for M02.
2. Use a standards-based `pyproject.toml` and a `src/edge_agent` package layout.
3. Build one modular monolith whose capabilities interact through ordinary
   typed application APIs before any network split.
4. Keep domain modules free of FastAPI, Pydantic transport models, ORM types,
   workflow engines, model SDKs, and market-data SDKs.
5. Use immutable dataclasses or equivalent validated value types in the domain,
   precise exceptions or tagged outcomes, and strict static type checking.
6. Add a CLI adapter for the first offline vertical slice.
7. Defer the API framework, database, migration runner, task queue, deployment
   platform, TypeScript UI, and model provider to separate ADRs when behavior
   creates an immediate need.
8. Keep default CI offline and independent of paid providers or credentials.

The exact formatter, linter, type checker, test runner, build backend, dependency
resolver, and security scanner will be selected and pinned in the M01 runtime
scaffold change. Tool selection must support Linux and Windows and one-command
local parity with CI.

## Alternatives considered

### Python 3.14 as the minimum

- Benefits: newest language/runtime features and a longer upstream support window.
- Costs and risks: narrower compatibility across numerical, typing, packaging,
  and provider dependencies at project inception.
- Reason not selected: 3.13 provides a more conservative baseline while still
  receiving regular bugfixes at the decision date. Support for 3.14 can be
  added after the pinned dependency set proves compatibility.

### TypeScript for the entire system

- Benefits: one language for a future web client and server; strong tooling for
  schemas and asynchronous I/O.
- Costs and risks: weaker default fit for quantitative and dataframe-heavy
  analysis; risk of coupling domain contracts to web/runtime conventions.
- Reason not selected: the deterministic research and evaluation core is the
  first system, while a web client is not yet required.

### Go for the control plane with Python analytics workers

- Benefits: strong concurrency, deployment, and operational characteristics;
  clear isolation of numerical workloads.
- Costs and risks: two runtimes, generated or duplicated contracts, distributed
  failure modes, and more operational surface before a measured need.
- Reason not selected: the initial vertical slice gains no value from a process
  boundary. This remains an option if later throughput or isolation evidence supports it.

### Separate services from the start

- Benefits: independent deployment and scaling boundaries.
- Costs and risks: premature network contracts, retries, deployment complexity,
  local setup cost, observability requirements, and cross-service consistency.
- Reason not selected: the first milestones need correctness and evidence
  integrity rather than independent scaling.

## Consequences

### Positive

- Contributors can implement and test the research spine in one runtime.
- Numerical and evaluation libraries are available when their need is proven.
- In-process capability calls simplify cancellation, replay, and diagnosis.
- Framework-free domain code can later move behind a service boundary without
  importing provider or transport semantics.
- A `src` layout helps ensure tests exercise the installed package instead of
  accidentally importing the repository working tree; see the Python Packaging
  Authority's [src-layout discussion](https://packaging.python.org/en/latest/discussions/src-layout-vs-flat-layout/).

### Negative

- CPU-bound scaling will eventually require process-level parallelism, native
  vectorized libraries, or separately deployed workers.
- A future TypeScript UI introduces a second language and contract-generation need.
- Python's dynamic runtime increases the importance of strict boundary parsing,
  static checking, and exhaustive tests.

### Neutral or follow-up

- This decision does not select a web framework, ORM, database, dataframe
  library, technical-analysis library, model provider, or market-data source.
- Modular monolith describes the initial deployment unit, not permission to
  create broad shared modules or bypass capability ownership.
- The repository may later support additional Python versions through CI after
  dependency and numerical reproducibility checks.

## Compatibility and migration

No application code or public contract exists, so accepting this ADR requires
no migration. The first scaffold will declare `requires-python`, package
metadata, supported platforms, exact verification commands, and lock or
constraint policy.

Changing the minimum runtime or splitting a capability into a service later
requires a superseding ADR with contract, state, rollout, and rollback plans.

## Security and operations

- The runtime and all tools/dependencies will be pinned through reviewed
  configuration and scanned in CI.
- Default tests cannot require secrets, network access, paid data, or model calls.
- Provider and model SDKs remain adapters and receive least-privilege credentials.
- Separate processes are introduced only with defined identity, authorization,
  timeout, retry, idempotency, observability, and recovery semantics.

## Validation

The M01 runtime scaffold will validate this decision by proving:

- clean setup and the same checks on Linux and Windows;
- formatting, linting, strict type checking, unit tests, packaging, and
  dependency/security review through one documented command;
- import of the installed `src` package rather than repository-root leakage;
- no runtime dependency in the domain package on a web, persistence, provider,
  or model framework; and
- an offline CLI smoke test with deterministic output.

M02 will validate the topology by delivering the complete synthetic-evidence
request-to-artifact slice without adding a network or process boundary.
