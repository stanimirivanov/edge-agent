# Deployment Portability

## TL;DR

EdgeAgent builds each Rust deployable once as a signed OCI image and promotes the
same digest through local, Kubernetes, and cloud-managed deployment profiles.
Cloud portability is enforced through stable application contracts, workload
identity, infrastructure modules, and automated conformance tests. It does not
require active-active operation across providers or force every workload onto
the lowest common denominator.

## 1. Portability Objective

The platform must demonstrate that independently deployable components can run
on GCP, Azure, AWS, or a conformant Kubernetes stack without changing domain
logic. Provider integrations remain replaceable infrastructure adapters. The
portable contract covers:

- OCI image inputs, health probes, graceful shutdown, and resource limits;
- CloudEvents-compatible envelopes and at-least-once delivery semantics;
- PostgreSQL-compatible durable state and S3-compatible object storage;
- OpenTelemetry-compatible telemetry export;
- OIDC workload identity and external secret references; and
- repeatable provisioning, deployment, migration, rollback, and smoke tests.

The first implementation may support one managed-cloud profile plus the
portable Kubernetes profile. Additional provider profiles must pass the same
conformance suite before they are described as supported.

## 2. Deployment Profiles

### 2.1 Portable Kubernetes profile

The portable profile is the reference environment for local demonstrations,
CI integration tests, and Kubernetes deployments. It uses:

- Knative Serving or standard Deployments for request-driven services;
- Kubernetes CronJobs or KEDA ScaledJobs for finite and event-driven workers;
- NATS JetStream for durable commands, events, replay, and consumer state;
- CloudNativePG for PostgreSQL lifecycle management;
- SeaweedFS or another maintained S3-compatible object store for evidence and audit objects;
- Valkey for optional ephemeral caching, never as the system of record;
- an OpenTelemetry Collector with Prometheus and Grafana plus a compatible
  trace backend; and
- Vault and OIDC-compatible identity, with SPIFFE/SPIRE available when
  workload identity requires stronger cross-cluster federation.

### 2.2 Cloud-managed profiles

Managed profiles may replace infrastructure adapters with provider services to
reduce operational load. They must preserve observable behavior: event schema,
idempotency, ordering boundaries, retention, retry policy, dead-letter handling,
security claims, and recovery objectives. A provider feature that cannot meet
that contract requires an explicit exception decision; it must not silently
change application semantics.

### 2.3 Cross-cloud recovery

Multi-cloud support means deployability to any supported profile. It does not
initially mean synchronous cross-cloud writes, global active-active databases,
or automatic provider failover. Those mechanisms add material consistency,
latency, egress, and incident-response costs. A later warm-standby profile may
restore versioned infrastructure, immutable objects, database backups, and
event checkpoints in another provider after recovery drills validate the
process.

## 3. Artifact and Configuration Contract

CI produces one OCI image per deployable from the shared build contract in
`Dockerfile` and `deploy/images.toml`. Release automation publishes the
current Linux AMD64 image, attaches a CycloneDX SBOM and GitHub build provenance,
and signs the immutable digest through OIDC. Multi-architecture manifests,
database migrations, and versioned event-schema bundles remain later release
increments. Environments promote immutable digests; they never rebuild source.

Runtime configuration enters through validated environment variables and
mounted configuration. Secrets enter through workload identity and external
secret references, not image layers, repository files, or event payloads. Each
service fails closed when required identity, schema, or dependency checks fail.

Infrastructure is organized as reusable OpenTofu modules with thin provider
composition layers. Helm or Kustomize packages the portable Kubernetes profile.
Every profile exposes the same logical endpoints and resource names to the
application while retaining provider-specific security, availability, and cost
controls in infrastructure code.

## 4. Event Portability Contract

Event routing and durable messaging solve different problems and are modeled
separately:

- routing selects destinations from event attributes and integrates provider
  services;
- durable messaging retains work, applies acknowledgements, redelivers failed
  deliveries, and isolates poison messages; and
- retained streams preserve ordered partitions for replay and projections.

All domain messages use a CloudEvents 1.0 envelope with a versioned payload.
Delivery is at least once. Consumers use durable inbox records and idempotency
keys; producers that change durable state use a transactional outbox. Ordering
is guaranteed only within the declared partition key. Retries use bounded
exponential backoff and end in a quarantined dead-letter stream with operator
metadata and replay tooling.

The portable profile uses NATS JetStream because Core NATS alone provides
at-most-once delivery, while JetStream adds persistence, acknowledgements, and
redelivery. Managed profiles can use cloud messaging services through adapters,
but provider event routers do not substitute for a durable queue or stream.

## 5. Portability Conformance

A deployment profile is supported only when automation proves that it can:

1. deploy the same signed image digests and run forward-compatible migrations;
2. authenticate services without static cloud credentials;
3. publish, consume, redeliver, quarantine, and replay versioned events;
4. reject duplicate commands without duplicate state transitions or fills;
5. retain immutable audit objects and verify their checksums;
6. export correlated logs, metrics, and traces through OpenTelemetry;
7. survive a service restart and recover consumer position without data loss;
8. roll back an application release without rolling back committed schemas; and
9. restore persistent state within the profile's documented recovery targets.

CI runs contract tests against portable local dependencies. Scheduled or
release-gated tests exercise real provider infrastructure to detect IAM,
network, quota, retention, and service-behavior drift.

## 6. Operational Boundaries

Each profile documents region, data residency, encryption keys, backup policy,
retention, egress paths, quotas, and cost ceilings. Services expose readiness
only after critical dependencies and schema compatibility are confirmed. They
handle termination by stopping intake, completing or abandoning messages within
the visibility window, flushing telemetry, and releasing leases.

OpenTelemetry for Rust is currently not fully stable across all signal types,
so the platform pins compatible crate versions behind an internal telemetry
crate and validates upgrades before broad rollout. Database and broker adapters
receive the same treatment: domain crates never depend directly on a cloud SDK.

## 7. Multi-Cloud Deployment Matrix

| Capability | GCP | Azure | AWS | Portable Kubernetes / Rust contract |
| --- | --- | --- | --- | --- |
| Request-driven Rust services | Cloud Run services | Azure Container Apps | Amazon ECS on Fargate | Knative Serving or Kubernetes Deployments; OCI image, HTTP/gRPC health contract |
| Finite and scheduled workers | Cloud Run jobs | Azure Container Apps jobs | ECS tasks on Fargate, scheduled by EventBridge Scheduler | Kubernetes CronJobs or KEDA ScaledJobs; idempotent Rust binary |
| Function-style handlers | Cloud Run functions, where its runtime model fits | Azure Functions custom handlers | Lambda container image or custom runtime | Not the baseline; package the same handler as a short-lived OCI process |
| Event routing | Eventarc | Event Grid | EventBridge | Knative Eventing; CloudEvents attributes define routing behavior |
| Durable commands and work queues | Pub/Sub | Service Bus | SQS, with SNS for fan-out | NATS JetStream through a messaging port; at-least-once delivery |
| Retained event streams | Pub/Sub or Managed Service for Apache Kafka | Event Hubs | Kinesis Data Streams or MSK | NATS JetStream or Kafka; explicit partition and retention contract |
| Relational system of record | Cloud SQL for PostgreSQL | Azure Database for PostgreSQL Flexible Server | RDS for PostgreSQL or Aurora PostgreSQL | CloudNativePG; SQL migrations and PostgreSQL compatibility tests |
| Optional document/key-value state | Firestore | Cosmos DB | DynamoDB | Prefer PostgreSQL JSONB initially; CockroachDB only after a dedicated decision |
| Immutable audit analytics | Cloud Storage retention policy/Bucket Lock plus BigQuery | Immutable Blob Storage plus Synapse serverless SQL | S3 Object Lock plus Athena | S3-compatible object store through `object_store`, queried with DataFusion; checksum and retention contract |
| Ephemeral cache | Memorystore | Azure Managed Redis | ElastiCache | Valkey cluster; disposable and never authoritative |
| General object storage | Cloud Storage | Blob Storage | S3 | SeaweedFS, RustFS, or Ceph through the `object_store` crate |
| Scheduler | Cloud Scheduler invoking a job or service | Container Apps scheduled jobs | EventBridge Scheduler invoking an ECS task | Kubernetes CronJobs or KEDA; schedules carry idempotency windows |
| Metrics, logs, and traces | Cloud Monitoring and Cloud Trace | Azure Monitor and Application Insights | CloudWatch and X-Ray | OpenTelemetry Collector, Prometheus, Grafana, and Tempo or Jaeger |
| Secrets and encryption | Secret Manager and Cloud KMS | Key Vault | Secrets Manager and KMS | Vault plus external-secrets integration; envelope-encryption contract |
| Workload identity | IAM and Workload Identity Federation | Microsoft Entra ID and managed identities | IAM roles for workloads | OIDC federation; Keycloak for local identity and optional SPIFFE/SPIRE workload IDs |
| Infrastructure delivery | OpenTofu GCP modules | OpenTofu AzureRM modules | OpenTofu AWS modules | OpenTofu plus Helm/Kustomize; identical logical outputs and smoke tests |

The matrix maps capabilities, not exact product equivalence. For example,
Eventarc, Event Grid, and EventBridge primarily route events; Service Bus, SQS,
Pub/Sub, Event Hubs, Kinesis, and JetStream supply different queue or stream
semantics. The deployment adapter must prove the required behavior rather than
rely on a product-name mapping.

## References

- [CloudEvents specification](https://cloudevents.io/)
- [NATS JetStream concepts](https://docs.nats.io/concepts/jetstream)
- [Google Cloud Eventarc documentation](https://docs.cloud.google.com/eventarc/docs)
- [Microsoft messaging service comparison](https://learn.microsoft.com/en-us/azure/service-bus-messaging/compare-messaging-services)
- [AWS decision guide for SNS, SQS, and EventBridge](https://docs.aws.amazon.com/pdfs/decision-guides/latest/sns-or-sqs-or-eventbridge/sns-or-sqs-or-eventbridge.pdf)
- [KEDA ScaledJob specification](https://keda.sh/docs/2.21/concepts/scaling-jobs/)
- [CloudNativePG architecture](https://cloudnative-pg.io/documentation/1.24/architecture/)
- [OpenTelemetry Rust status](https://opentelemetry.io/docs/languages/rust/)
- [Microsoft Entra product-name change](https://learn.microsoft.com/en-us/entra/fundamentals/new-name)
- [Azure Cache for Redis migration guidance](https://learn.microsoft.com/en-us/azure/azure-cache-for-redis/cache-migration-guide)
