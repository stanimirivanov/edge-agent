# Security policy

## TL;DR

- Do not disclose suspected vulnerabilities in public issues, discussions, logs, or pull requests.
- Use GitHub private vulnerability reporting when available.
- Never include API keys, personal data, licensed market-data payloads, or private prompts in a report.
- EdgeAgent is pre-release; only the current default branch is supported.
- Treat user input, providers, retrieved content, plugins, and model output as untrusted.
- Do not test systems, accounts, datasets, or services without explicit authorization.

## Supported versions

EdgeAgent is in engineering foundation and has no supported production release.
Security fixes target the current default branch. When releases begin, this
section will be replaced by an explicit supported-version table and disclosure
schedule.

## Reporting a vulnerability

Use the repository Security page's **Report a vulnerability** action. This is
the preferred private channel. If private reporting is unavailable, request a
private contact channel without naming the vulnerable component, exploit,
affected data, or reproduction details in public.

Include, where applicable:

- affected revision, component, and configuration;
- impact and preconditions;
- minimal reproduction or proof of concept;
- redacted logs and evidence;
- known mitigations; and
- prior disclosure, if any.

Do not test infrastructure, repositories, accounts, data, model providers, or
market-data services you do not own or have permission to assess. Do not retain,
alter, trade on, or disclose accessed data beyond what is needed for safe
demonstration.

## Project threat model

EdgeAgent processes unusually adversarial and time-sensitive inputs. Security
review covers conventional application risk and these project-specific threats:

- **Prompt and retrieval injection:** news, filings, social content, provider
  metadata, and tool descriptions may contain instructions intended to alter
  system behavior.
- **Evidence poisoning:** bad ticks, manipulated sentiment, symbol collisions,
  stale prices, incorrect corporate actions, or compromised providers may
  corrupt research output.
- **Model overreach:** a model may fabricate facts, misstate calculations,
  attempt unauthorized tools, or present uncertainty as certainty.
- **Cross-tenant disclosure:** requests, preferences, artifacts, prompts, or
  evidence from one tenant may leak to another.
- **Credential and entitlement abuse:** provider keys may be stolen or used to
  retrieve, retain, or redistribute data outside contractual rights.
- **Unsafe capability expansion:** a research component may be connected to an
  execution path without the authorization and controls required by the product boundary.
- **Evaluation leakage:** future information or holdout data may enter strategy
  development and produce misleading performance evidence.
- **Supply-chain compromise:** models, packages, actions, containers, datasets,
  or generated artifacts may be replaced or tampered with.

## Required security properties

- Deny by default and grant the least privilege needed for one capability.
- Validate external input at adapters and enforce domain invariants after translation.
- Keep deterministic analytics and policy decisions outside generative models.
- Isolate retrieved content from system instructions and tool permissions.
- Use managed secret injection; never commit credentials or production defaults.
- Bound request size, tool calls, concurrency, retries, model tokens, output,
  and execution time.
- Preserve source, version, timestamps, transformation lineage, and integrity
  metadata for published research facts.
- Fail closed when critical evidence, entitlement, authorization, or policy is invalid.
- Redact credentials, authorization headers, personal data, private prompts,
  and licensed payloads from logs and test fixtures.
- Pin dependencies and CI actions to reviewed release lines; use read-only CI
  permissions unless a job documents why it needs more.

## Security expectations for contributions

A change affecting authentication, authorization, sensitive data, prompt or
retrieval processing, model/tool execution, provider credentials, public input,
artifact integrity, or execution boundaries MUST document:

- assets and trust boundaries;
- abuse and failure cases;
- validation, authorization, and resource limits;
- safe logging and evidence retention;
- rollout, rollback, and incident behavior; and
- focused security verification.

Contributors MUST follow [AGENTS.md](AGENTS.md),
[CONTRIBUTING.md](CONTRIBUTING.md), and the
[engineering standards](docs/development/engineering-standards.md).

## Triage and disclosure

Maintainers SHOULD acknowledge a private report, validate scope and severity,
and agree on a communication path before disclosure. Response times are
best-effort until a staffed process exists.

Reporter and maintainers SHOULD coordinate publication after a fix or effective
mitigation is available. Maintainers MAY publish a GitHub security advisory and
credit the reporter unless anonymity is requested. Good-faith reports SHOULD
be handled respectfully and without retaliation.
