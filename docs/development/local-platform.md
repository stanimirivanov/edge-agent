# Local platform profile

## TL;DR

- The local profile supplies PostgreSQL, NATS JetStream, SeaweedFS S3 storage, and an OpenTelemetry Collector through Docker Compose.
- Every host port binds to `127.0.0.1`; the profile is not a production or shared-development deployment.
- Image versions, licenses, exposed ports, and readiness contracts are declared in `deploy/local/stack.toml` and checked against Compose.
- `make local-up` validates configuration, starts the profile, and waits for all four capabilities.
- `make local-down` stops containers without deleting their named volumes.
- The checked-in credentials are intentionally local and must never be reused outside an isolated workstation or CI runner.

## Purpose and boundary

The profile gives contributors one reproducible set of external capabilities for
event and persistence work. It validates application adapters against local
interfaces without making a production infrastructure choice. PostgreSQL, NATS,
S3, and OTLP contracts remain authoritative; container-specific administration,
on-disk formats, and vendor extensions do not.

[SeaweedFS](https://github.com/seaweedfs/seaweedfs) provides the local
S3-compatible endpoint. MinIO is not used because its
[open-source repository](https://github.com/minio/minio) was archived and its
legacy precompiled releases are no longer maintained. SeaweedFS remains a
development dependency, not an application-domain type or an implied production
requirement.

## Prerequisites and lifecycle

Install Docker Engine or Docker Desktop with Docker Compose v2. From the
repository root:

```text
make local-up
make local-status
make local-down
```

`local-up` uses `deploy/local/.env.example` directly. Do not add real secrets to
that file. To override values, invoke Compose with a separate ignored env file
and the same checked-in Compose definition.

Named volumes preserve database, stream, and object data across `local-down` and
subsequent starts. To erase all local platform state, run the following explicit
destructive command only when that loss is intended:

```text
docker compose --env-file deploy/local/.env.example -f deploy/local/compose.yaml down --volumes
```

The reset cannot be recovered unless the named volumes were backed up.

## Capability endpoints

| Capability | Local endpoint | Purpose |
| --- | --- | --- |
| PostgreSQL | `postgresql://edgeagent@127.0.0.1:5432/edgeagent` | Service-owned transactional state, inboxes, and outboxes |
| NATS client | `nats://127.0.0.1:4222` | Durable commands and events through JetStream |
| NATS monitoring | `http://127.0.0.1:8222` | Health, stream, consumer, and server diagnostics |
| S3-compatible API | `http://127.0.0.1:8333` | Evidence and immutable development objects |
| OTLP gRPC | `http://127.0.0.1:4317` | Trace, metric, and log ingestion |
| OTLP HTTP | `http://127.0.0.1:4318` | HTTP telemetry ingestion |
| Collector health | `http://127.0.0.1:13133` | Collector readiness |

The PostgreSQL URI omits the checked-in local password. Clients must read the
complete values from `deploy/local/.env.example` or an approved override. S3
clients use path-style addressing, the local access and secret keys, and the
declared local bucket.

## Readiness and validation

Run static validation without Docker:

```text
python scripts/verify_local_stack.py
```

The verifier checks exact service membership, non-floating image tags, declared
licenses, localhost-only published ports, credential naming, JetStream
persistence, OpenTelemetry pipelines, and agreement between the manifest and
Compose configuration.

After Compose starts, the same verifier waits for PostgreSQL TCP acceptance,
NATS JetStream health, an authenticated S3 response boundary, and collector
health:

```text
python scripts/verify_local_stack.py --running
```

Readiness proves that the capability endpoint is available. Later adapter
milestones must add behavioral conformance for SQL transactions, outbox/inbox
recovery, JetStream acknowledgement and replay, S3 object integrity, and OTLP
correlation.

## Security and operational limits

- All published ports bind to loopback and must remain unreachable from another host.
- Local credentials are public test fixtures with no value outside this profile.
- Services enable the container `no-new-privileges` security option.
- No cloud or broker credentials enter this environment.
- Named volumes may contain synthetic test artifacts and must not receive licensed market data or personal information.
- The OpenTelemetry debug exporter can print received telemetry; callers must not send secrets, licensed payloads, or personal data.
- This single-node profile does not demonstrate high availability, encryption at rest, TLS, workload identity, backup, restore, or production recovery behavior.

## Version and dependency policy

`deploy/local/stack.toml` records the selected image, upstream license, ports,
and readiness type. Version upgrades require a focused dependency review and a
passing CI startup test. Floating tags such as `latest`, `stable`, or `main` are
rejected. Release tags are repeatable at the version level but are not immutable
digests; release automation will add digest locking and signature verification
with the broader supply-chain milestone.

The current choices use actively maintained releases of PostgreSQL, NATS,
SeaweedFS, and the OpenTelemetry Collector. Application code must still target
the generalized PostgreSQL, NATS JetStream, S3, and OTLP contracts rather than
these local product versions.

## Troubleshooting

Use `make local-status` first. If a capability does not become ready, inspect
bounded service logs with:

```text
docker compose --env-file deploy/local/.env.example -f deploy/local/compose.yaml logs --tail=200 postgres
```

Replace `postgres` with `nats`, `object-store`, or `otel-collector`. Do not post
unredacted logs publicly if a local override contains real credentials or data.
Version conflicts, occupied ports, insufficient disk, or stale volumes should
be corrected explicitly; do not weaken readiness checks or silently choose a
different port in the checked-in profile.
