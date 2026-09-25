# ADR-0004: Separate application, UI, GitOps, and substrate ownership

- Status: Accepted
- Date: 2026-09-25
- Milestone: M11 - Trusted user experience
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Keep Rust services, public contracts, deterministic policy, and release
  production in `edge-agent`.
- Build the human-facing research and dry-run experience in `edge-agent-ui` as
  an untrusted adapter to the gateway; it owns no market fact, policy decision,
  fill, position, cash balance, or P&L value.
- Keep environment desired state, immutable release selection, and Argo CD
  consumer resources in `edge-agent-gitops`.
- Use `k8s-infrastructure` only for project-neutral clusters, Argo CD,
  shared substrate capabilities, provider guardrails, and qualification.
- Exchange versioned contracts and immutable artifacts between repositories;
  never exchange credentials, infrastructure state, or private service data.

## Context

The Rust workspace deliberately separates five deployable components while
keeping their domain and application behavior in one source repository. The
project now also has dedicated `edge-agent-ui` and `edge-agent-gitops`
repositories and can consume Kubernetes substrates from `k8s-infrastructure`.

Without an explicit ownership decision, deployment configuration could drift
between application and infrastructure repositories, the UI could duplicate
domain policy, and release automation could mutate a cluster directly. Those
choices would weaken auditability, portability, least privilege, and the
build-once/promote-by-digest contract.

ADR-0001 intentionally left the web UI language and repository undecided. The
product vision already treats CLI, API, web, and conversational surfaces as
adapters to the same contracts. This decision resolves repository placement
and trust boundaries while leaving the UI technology stack to a focused ADR in
the UI repository.

## Decision

Use four repositories with disjoint authority:

| Repository | Owns | Must not own |
| --- | --- | --- |
| `edge-agent` | Rust domain and application code, gateway APIs, versioned schemas, deterministic policy, service images, SBOMs, provenance, and signatures | Environment release selection, browser presentation state, cluster lifecycle, cloud credentials |
| `edge-agent-ui` | Browser interaction and presentation, accessibility, generated API client integration, frontend tests, and its independently releasable artifact | Domain calculations, policy decisions, direct service/database/broker access, operator control-plane authority |
| `edge-agent-gitops` | Argo CD Projects and Applications, namespaces, environment overlays, immutable backend and UI digests, dependency instances, external secret references, promotion history, and product acceptance | Cluster provisioning, Argo CD installation, source builds, embedded credentials or Terraform/OpenTofu state |
| `k8s-infrastructure` | Project-neutral Kubernetes substrates, Argo CD installation, shared cluster capabilities, provider IAM, cost guardrails, and substrate qualification | EdgeAgent workloads, release selection, product credentials, migrations, or product acceptance |

`edge-agent` is the semantic source of truth. It publishes versioned API and
event schemas suitable for client generation. `edge-agent-ui` pins a reviewed
contract version and communicates only with the gateway over its public API.
It does not call internal services, NATS, PostgreSQL, object storage, model
providers, or market-data providers.

The initial UI serves a research and dry-run user. Administrative functions
such as Argo CD reconciliation, dead-letter replay, infrastructure mutation,
credential management, and emergency controls remain on separately authorized
operator surfaces. A future administrative UI requires its own threat model,
authorization design, and decision.

`edge-agent-gitops` records the exact signed image digests selected for each
environment. Promotion changes desired state through review; build pipelines do
not imperatively deploy to clusters. Argo CD reconciles the reviewed state.
Rollback selects a previously verified digest rather than rebuilding or moving
a release tag.

`k8s-infrastructure` may install and qualify shared cluster-wide controllers
such as an ingress, external-secrets, autoscaling, or database operator. The
consumer-specific resources handled by those controllers remain in
`edge-agent-gitops`.

Repository handoffs contain only versioned schemas, immutable artifact
identities, public source revisions, and non-secret capability information.
They never contain kubeconfig bytes, signing material, provider credentials,
Kubernetes Secret values, or infrastructure state.

## Alternatives considered

### Keep UI and deployment configuration in the Rust monorepo

- Benefits: one checkout and one issue tracker.
- Costs and risks: source builds and environment promotions share history and
  access; frontend dependencies enter the backend verification surface;
  environment-only changes can trigger unrelated builds.
- Reason not selected: repository separation makes the contract boundaries,
  artifact promotion, and independent release cadences observable.

### Put EdgeAgent desired state in `k8s-infrastructure`

- Benefits: one repository can bootstrap the cluster and its workloads.
- Costs and risks: the neutral substrate becomes coupled to one consumer,
  application contributors gain an infrastructure-adjacent change path, and
  the existing substrate/consumer contract is reversed.
- Reason not selected: infrastructure installs and qualifies Argo CD;
  consumers own what Argo CD reconciles.

### Allow the UI to call services or reproduce calculations directly

- Benefits: fewer gateway endpoints and potentially faster prototypes.
- Costs and risks: authorization fragments across services, browser code can
  disagree with audited domain logic, and untrusted content gains a wider
  attack surface.
- Reason not selected: the gateway is the only browser-facing application
  boundary and deterministic services remain authoritative.

## Consequences

### Positive

- Source, promotion, substrate, and presentation histories remain independently
  reviewable.
- The same signed backend and UI artifacts can be promoted across environments.
- Browser compromise cannot legitimately create facts, approve policy, or
  bypass the dry-run execution boundary.
- Kubernetes and managed-cloud profiles can share semantic contracts without
  forcing identical infrastructure implementations.

### Negative

- Cross-repository compatibility, release, and issue coordination become
  explicit engineering work.
- Local development needs a documented composition workflow across source, UI,
  dependencies, and eventually GitOps.
- Contract publication and generated-client compatibility need automation.

### Neutral or follow-up

- The UI language, framework, package manager, rendering architecture, and
  authentication library require a decision in `edge-agent-ui`.
- GitOps packaging, promotion automation, and the initial target substrate are
  selected in `edge-agent-gitops` under M09.
- M11 may begin contract and experience discovery early, but useful product
  slices depend on the corresponding backend milestones.

## Compatibility and migration

All three EdgeAgent repositories other than the Rust source repository are
empty foundations, so this decision moves no runtime resource or user data.
The `deploy/local` dependency profile and image build manifest remain in
`edge-agent`; future environment desired state belongs in
`edge-agent-gitops`.

Existing APIs and image releases are unchanged. When public schemas become
available, UI compatibility is proven against pinned generated clients and
contract fixtures rather than by copying Rust types or maintaining handwritten
lookalikes.

## Security and operations

- Treat the browser, user input, rendered model output, URLs, and retrieved
  content as untrusted.
- Enforce authentication, authorization, quotas, idempotency, policy, and
  `dry_run` mode at the gateway and deterministic services, never only in UI
  controls.
- Store no provider, broker, database, cluster, or signing credentials in the
  UI bundle or GitOps repository.
- Grant Argo CD Projects only the source repositories, destination namespaces,
  clusters, and resource kinds they require.
- Verify signatures and attestations before a digest becomes promotable;
  admission enforcement remains part of deployment-profile qualification.
- Route operationally privileged actions through separately authenticated and
  audited tooling.

## Validation

- Documentation and repository verifiers agree on the four ownership
  boundaries.
- Future UI contract tests prove that browser-visible values originate from
  gateway responses and deterministic fixtures.
- Future GitOps tests render exact immutable digests and reject mutable tags,
  embedded secrets, unsupported destinations, and unsigned promotion inputs.
- Deployment conformance proves that promoted artifacts are identical across
  claimed profiles.
- End-to-end acceptance rejects every execution mode other than `dry_run` even
  when a client bypasses UI validation.
