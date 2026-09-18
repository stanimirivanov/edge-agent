# EdgeAgent

## TL;DR

- EdgeAgent is an open-source, evidence-grounded market research and decision-support platform.
- It turns a bounded research request into reproducible research artifacts for liquid U.S. equities and ETFs.
- Deterministic services own prices, indicators, rankings, simulations, and risk calculations; models may explain validated evidence but cannot create facts.
- Stale, conflicting, unlicensed, or insufficient evidence produces an explicit abstention.
- EdgeAgent does not execute, route, or stage orders and does not promise investment outcomes.
- The project is in **M01 - Engineering foundation**; application components have not been scaffolded yet.

## What EdgeAgent is

EdgeAgent aims to make market research more consistent, inspectable, and
reproducible. A request identifies a security universe, time horizon, strategy,
and constraints. The system acquires point-in-time evidence, computes features
and risk deterministically, applies policy, and emits a versioned research
artifact with sources, assumptions, uncertainty, expiry, and invalidation
conditions.

The project initially targets liquid U.S.-listed equities and ETFs over swing
horizons. Its reusable primitives—request contracts, provider adapters,
evidence lineage, versioned strategies, policy gates, evaluation, and artifact
lifecycle—are designed to support additional research modes without coupling
the domain to one data vendor, model provider, user interface, or framework.

## What EdgeAgent is not

EdgeAgent is not a brokerage, order-management system, custody product, or
profit guarantee. The initial scope excludes broker connectivity, autonomous
execution, options, leverage, short selling, micro-cap securities, and
individualized portfolio optimization. Research output is informational and
must not be represented as a certain outcome or substitute for professional
advice.

## Engineering principles

- **Evidence before narrative:** every time-sensitive or numeric claim is
  traceable to source data and deterministic transformations.
- **Policy before publication:** models cannot bypass data-quality, product,
  or risk controls.
- **Abstention over invention:** insufficient evidence is a valid result.
- **Point-in-time integrity:** evaluation excludes hindsight, look-ahead, and
  silent strategy revision.
- **Portable boundaries:** domain behavior does not depend on transport,
  storage, market-data, or model-provider types.
- **Open-source reproducibility:** default verification does not require paid
  services, private datasets, or credentials.

## Repository map

- [Product vision](docs/product/vision.md)
- [System architecture](docs/architecture/system-overview.md)
- [Implementation milestones](docs/roadmap/milestones.md)
- [Contributor workflow](CONTRIBUTING.md)
- [Engineering standards](docs/development/engineering-standards.md)
- [Architecture decisions](docs/decisions/README.md)
- [Security policy](SECURITY.md)
- [Code of conduct](CODE_OF_CONDUCT.md)

## Verify this foundation

The current harness uses only the Python standard library. With Python 3.11 or
newer installed, use the interpreter command available on your platform:

```text
python scripts/verify_repository.py --format-check
python scripts/verify_repository.py
python -m unittest discover -s tests -p "test_*.py"
```

The examples use `python`; `python3` on POSIX or `py -3` on Windows are
equivalent. On systems with `make`, `make verify PYTHON=python3` runs the same
checks (use the appropriate interpreter command). GitHub Actions runs them on
Linux and Windows. Application build commands will be added with the first
executable scaffold and must remain consistent with these entry points.

## Contributing

Read [AGENTS.md](AGENTS.md) and [CONTRIBUTING.md](CONTRIBUTING.md) before making
changes. The project favors one coherent, independently reviewable capability
per issue and pull request. Never commit API credentials, personal data,
licensed market-data payloads, raw private prompts, or local model artifacts.

## License

EdgeAgent is licensed under the [Apache License 2.0](LICENSE).
