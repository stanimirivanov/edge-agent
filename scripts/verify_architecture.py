#!/usr/bin/env python3
"""Verify EdgeAgent's Cargo workspace structure and manifest boundaries."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import sys
import tomllib


@dataclass(frozen=True)
class PackageRule:
    """Reviewed canonical package identities, independently for each kind."""

    name: str
    target: str
    dependencies: frozenset[str]
    dev_dependencies: frozenset[str] = frozenset()
    build_dependencies: frozenset[str] = frozenset()


@dataclass(frozen=True)
class DependencyDeclaration:
    """A declared capability, including inactive targets and optional features."""

    table: str
    kind: str
    alias: str
    package: str
    optional: bool
    inherited: bool


@dataclass(frozen=True, order=True)
class Problem:
    """A deterministic architecture verification failure."""

    path: str
    message: str

    def render(self) -> str:
        """Return a compiler-style failure description."""

        return f"{self.path}: {self.message}"


CONTRACTS = "edgeagent-contracts"
CONFORMANCE = "edgeagent-event-spine-conformance"
INBOX_HANDLER = "edgeagent-inbox-handler"
INBOX_POSTGRES = "edgeagent-inbox-postgres"
MESSAGING = "edgeagent-messaging"
MESSAGING_NATS = "edgeagent-messaging-nats"
OUTBOX_POSTGRES = "edgeagent-outbox-postgres"
OUTBOX_RELAY = "edgeagent-outbox-relay"
SERVICE_RUNTIME = "edgeagent-service-runtime"
TELEMETRY = "edgeagent-telemetry"
XTASK = "edgeagent-xtask"
CONTRACT_DEPENDENCIES = frozenset({"chrono", "sha2", "schemars", "cloudevents-sdk", "serde", "serde_json", "url"})
INBOX_POSTGRES_DEPENDENCIES = frozenset({CONTRACTS, MESSAGING, "sqlx"})
INBOX_POSTGRES_DEV_DEPENDENCIES = frozenset({"serde_json", "tokio"})
INBOX_HANDLER_DEPENDENCIES = frozenset({CONTRACTS, MESSAGING, TELEMETRY})
INBOX_HANDLER_DEV_DEPENDENCIES = frozenset(
    {
        "async-nats",
        INBOX_POSTGRES,
        MESSAGING_NATS,
        OUTBOX_POSTGRES,
        OUTBOX_RELAY,
        "serde_json",
        "sqlx",
        "tokio",
    }
)
MESSAGING_DEPENDENCIES = frozenset({"bytes", CONTRACTS, "tracing"})
MESSAGING_NATS_DEPENDENCIES = frozenset(
    {"async-nats", CONTRACTS, "futures-util", MESSAGING}
)
MESSAGING_NATS_DEV_DEPENDENCIES = frozenset({"serde_json", "tokio"})
OUTBOX_POSTGRES_DEPENDENCIES = frozenset({CONTRACTS, MESSAGING, "sqlx"})
OUTBOX_POSTGRES_DEV_DEPENDENCIES = frozenset({"serde_json", "tokio"})
OUTBOX_RELAY_DEPENDENCIES = frozenset({CONTRACTS, MESSAGING, TELEMETRY})
OUTBOX_RELAY_DEV_DEPENDENCIES = frozenset(
    {OUTBOX_POSTGRES, "serde_json", "sqlx", "tokio", "tracing"}
)
TELEMETRY_DEPENDENCIES = frozenset({CONTRACTS, MESSAGING, "metrics", "tracing"})
TELEMETRY_DEV_DEPENDENCIES = frozenset({"serde_json"})
SERVICE_DEPENDENCIES = frozenset({CONTRACTS, SERVICE_RUNTIME})
XTASK_DEPENDENCIES = frozenset(
    {"cargo_metadata", "percent-encoding", "pulldown-cmark", "yaml-rust2"}
)

PACKAGE_RULES = {
    "crates/market-data-replay": PackageRule(
        "edgeagent-market-data-replay", "lib", frozenset({
            "edgeagent-market-data-application", "edgeagent-market-data-domain", "edgeagent-simulation-domain",
        }),
    ),
    "crates/market-data-postgres": PackageRule(
        "edgeagent-market-data-postgres", "lib", frozenset({CONTRACTS, "edgeagent-market-data-application", "edgeagent-market-data-domain", "edgeagent-inbox-postgres", "edgeagent-outbox-postgres", "serde_json", "sqlx"}),
        frozenset({"futures-util", "tokio"}),
    ),
    "crates/market-data-object-store": PackageRule(
        "edgeagent-market-data-object-store", "lib", frozenset({
            CONTRACTS, "edgeagent-market-data-application", "bytes", "futures-util", "object_store", "tokio",
        }), frozenset({"edgeagent-market-data-domain", "serde_json", "tokio"}),
    ),
    "crates/market-data-http": PackageRule(
        "edgeagent-market-data-http", "lib", frozenset({
            CONTRACTS, "edgeagent-market-data-application", "edgeagent-market-data-domain", "metrics",
            "chrono", "httpdate", "reqwest", "rustls", "rustls-native-certs", "serde", "serde_json", "tokio", "url",
            "base64", "bytes", "fastwebsockets", "http-body-util", "hyper", "hyper-util", "sha1", "tokio-rustls",
        }), frozenset({"tokio", "rcgen"}),
    ),
    "crates/market-data-application": PackageRule(
        "edgeagent-market-data-application", "lib", frozenset({CONTRACTS, "edgeagent-market-data-domain", "serde", "serde_json"}),
    ),
    "crates/audit-domain": PackageRule(
        "edgeagent-audit-domain", "lib",
        frozenset({CONTRACTS, "edgeagent-research-domain", "edgeagent-market-data-domain", "edgeagent-simulation-domain", "serde", "serde_json", "sha2"}),
    ),
    "crates/simulation-postgres": PackageRule(
        "edgeagent-simulation-postgres", "lib",
        frozenset({CONTRACTS, INBOX_POSTGRES, "edgeagent-market-data-domain", MESSAGING,
                   OUTBOX_POSTGRES, "edgeagent-simulation-domain", "chrono", "serde_json", "sqlx"}),
    ),
    "crates/simulation-domain": PackageRule(
        "edgeagent-simulation-domain", "lib",
        frozenset({"edgeagent-market-data-domain", "libm", "rand_chacha", "sha2"}),
    ),
    "crates/contracts": PackageRule(CONTRACTS, "lib", CONTRACT_DEPENDENCIES),
    "crates/event-spine-conformance": PackageRule(
        CONFORMANCE,
        "lib",
        frozenset({
            CONTRACTS, INBOX_HANDLER, INBOX_POSTGRES, MESSAGING, MESSAGING_NATS,
            OUTBOX_POSTGRES, OUTBOX_RELAY, TELEMETRY, "async-nats", "jsonschema", "schemars",
            "serde", "serde_json", "sqlx", "tokio", "tracing", "tracing-subscriber",
            "opentelemetry", "opentelemetry-otlp", "opentelemetry_sdk", "reqwest",
            "edgeagent-market-data-domain", "edgeagent-simulation-domain", "edgeagent-simulation-postgres", "edgeagent-audit-domain",
        }),
        frozenset({"futures-util", "axum", "opentelemetry-proto", "prost"}),
    ),
    "crates/inbox-handler": PackageRule(
        INBOX_HANDLER,
        "lib",
        INBOX_HANDLER_DEPENDENCIES,
        INBOX_HANDLER_DEV_DEPENDENCIES,
    ),
    "crates/inbox-postgres": PackageRule(
        INBOX_POSTGRES,
        "lib",
        INBOX_POSTGRES_DEPENDENCIES,
        INBOX_POSTGRES_DEV_DEPENDENCIES,
    ),
    "crates/messaging": PackageRule(
        MESSAGING,
        "lib",
        MESSAGING_DEPENDENCIES,
    ),
    "crates/research-postgres": PackageRule(
        "edgeagent-research-postgres", "lib", frozenset({CONTRACTS, "edgeagent-research-application", INBOX_POSTGRES, OUTBOX_POSTGRES, "sqlx"}),
        frozenset({"edgeagent-research-domain", "edgeagent-market-data-domain", "futures-util", "tokio"}),
    ),
    "crates/research-application": PackageRule(
        "edgeagent-research-application", "lib", frozenset({CONTRACTS, "edgeagent-research-domain"}),
        frozenset({"edgeagent-market-data-domain", "serde_json"}),
    ),
    "crates/research-domain": PackageRule(
        "edgeagent-research-domain", "lib", frozenset({"edgeagent-market-data-domain"}),
    ),
    "crates/market-data-domain": PackageRule(
        "edgeagent-market-data-domain", "lib", frozenset({"chrono", "chrono-tz"}),
    ),
    "crates/messaging-nats": PackageRule(
        MESSAGING_NATS,
        "lib",
        MESSAGING_NATS_DEPENDENCIES,
        MESSAGING_NATS_DEV_DEPENDENCIES,
    ),
    "crates/outbox-postgres": PackageRule(
        OUTBOX_POSTGRES,
        "lib",
        OUTBOX_POSTGRES_DEPENDENCIES,
        OUTBOX_POSTGRES_DEV_DEPENDENCIES,
    ),
    "crates/outbox-relay": PackageRule(
        OUTBOX_RELAY,
        "lib",
        OUTBOX_RELAY_DEPENDENCIES,
        OUTBOX_RELAY_DEV_DEPENDENCIES,
    ),
    "crates/service-runtime": PackageRule(
        SERVICE_RUNTIME,
        "lib",
        frozenset({CONTRACTS}),
    ),
    "crates/telemetry": PackageRule(
        TELEMETRY,
        "lib",
        TELEMETRY_DEPENDENCIES,
        TELEMETRY_DEV_DEPENDENCIES,
    ),
    "crates/xtask": PackageRule(
        XTASK,
        "bin",
        XTASK_DEPENDENCIES,
    ),
    "services/audit-projector": PackageRule(
        "edgeagent-audit-projector", "bin",
        frozenset({"edgeagent-contracts","edgeagent-service-runtime","edgeagent-audit-domain","edgeagent-research-application","edgeagent-inbox-postgres","edgeagent-messaging","edgeagent-messaging-nats","axum","async-nats","serde","serde_json","sqlx","tokio","url"}),
        frozenset({"futures-util"}),
    ),
    "services/execution-simulator": PackageRule(
        "edgeagent-execution-simulator",
        "bin",
        SERVICE_DEPENDENCIES,
    ),
    "services/gateway": PackageRule(
        "edgeagent-gateway",
        "bin",
        SERVICE_DEPENDENCIES | frozenset({
            "edgeagent-outbox-postgres", "edgeagent-research-application",
            "axum", "reqwest", "rustls", "serde", "serde_json", "sqlx", "tokio", "url",
        }),
        frozenset({"futures-util"}),
    ),
    "services/market-data": PackageRule(
        "edgeagent-market-data",
        "bin",
        SERVICE_DEPENDENCIES | frozenset({
            "edgeagent-market-data-application", "edgeagent-market-data-domain",
            "edgeagent-market-data-replay", "edgeagent-market-data-postgres",
            "edgeagent-market-data-object-store", "edgeagent-simulation-domain",
            "edgeagent-market-data-http",
            "edgeagent-inbox-postgres", "edgeagent-outbox-postgres", "edgeagent-outbox-relay",
            "edgeagent-messaging", "edgeagent-messaging-nats", "async-nats", "chrono",
            "object_store", "serde_json", "sqlx", "tokio", "url", "futures-util",
        }),
        frozenset({"tokio", "edgeagent-event-spine-conformance"}),
    ),
    "services/research": PackageRule(
        "edgeagent-research", "bin",
        frozenset({"edgeagent-contracts","edgeagent-service-runtime","edgeagent-research-application","edgeagent-research-domain","edgeagent-research-postgres","edgeagent-market-data-domain","edgeagent-inbox-postgres","edgeagent-outbox-postgres","edgeagent-outbox-relay","edgeagent-messaging","edgeagent-messaging-nats","async-nats","chrono","serde_json","sqlx","tokio","url"}),
        frozenset({"futures-util"}),
    ),
}

INHERITED_PACKAGE_FIELDS = {
    "edition",
    "license",
    "repository",
    "rust-version",
    "version",
}

DEPENDENCY_KINDS = ("dependencies", "dev-dependencies", "build-dependencies")
INHERITED_DEPENDENCY_KEYS = {"workspace", "features", "optional", "default-features"}


def _read_toml(path: Path) -> dict[str, object]:
    with path.open("rb") as source:
        return tomllib.load(source)


def check_dependencies(
    relative: str,
    dependencies: set[str],
    allowed: frozenset[str],
) -> list[Problem]:
    """Require an explicit direct-dependency allowlist during foundation work."""

    problems = []
    for dependency in sorted(dependencies - allowed):
        problems.append(Problem(relative, f"undeclared dependency boundary: {dependency}"))
    for dependency in sorted(allowed - dependencies):
        problems.append(Problem(relative, f"required dependency is missing: {dependency}"))
    return problems


def check_workspace_members(members: set[str]) -> list[Problem]:
    """Require the documented foundation packages and no implicit members."""

    expected = set(PACKAGE_RULES)
    problems = []
    for member in sorted(members - expected):
        problems.append(Problem("Cargo.toml", f"unexpected workspace member: {member}"))
    for member in sorted(expected - members):
        problems.append(Problem("Cargo.toml", f"required workspace member is missing: {member}"))
    return problems


def _package_identity(alias: str, value: object) -> tuple[str | None, str | None]:
    """Read identity only; Cargo and supply-chain checks still own source validity."""

    if isinstance(value, str):
        if value.strip():
            return alias, None
        return None, "dependency version must not be empty"
    if not isinstance(value, dict):
        return None, "dependency must be a version string or table"
    if "workspace" in value:
        return None, "workspace.dependencies entries cannot inherit workspace dependencies"
    package = value.get("package", alias)
    if not isinstance(package, str) or not package.strip():
        return None, "dependency package must be a non-empty string"
    return package, None


def _dependency_declarations(
    relative: str,
    manifest: dict[str, object],
    workspace_dependencies: dict[str, object],
) -> tuple[list[DependencyDeclaration], list[Problem]]:
    """Enumerate every declaration without evaluating target predicates/features."""

    declarations: list[DependencyDeclaration] = []
    problems: list[Problem] = []
    scopes = [("", manifest)]
    targets = manifest.get("target", {})
    if not isinstance(targets, dict):
        problems.append(Problem(relative, "target must be a table"))
    else:
        for target, scope in sorted(targets.items()):
            if not isinstance(scope, dict):
                problems.append(Problem(relative, f"target.{target!r} must be a table"))
            else:
                scopes.append((f"target.{target!r}.", scope))

    for prefix, scope in scopes:
        for legacy in ("dev_dependencies", "build_dependencies"):
            if legacy in scope:
                problems.append(
                    Problem(
                        relative,
                        f"{prefix}{legacy} is unsupported in Rust 2024; "
                        f"use {legacy.replace('_', '-')}",
                    )
                )
        for kind in DEPENDENCY_KINDS:
            table = f"{prefix}{kind}"
            location = f"{relative}/Cargo.toml [{table}]"
            dependencies = scope.get(kind, {})
            if not isinstance(dependencies, dict):
                problems.append(Problem(location, "dependency declarations must be a table"))
                continue
            for alias, value in sorted(dependencies.items()):
                inherited = isinstance(value, dict) and "workspace" in value
                optional = value.get("optional", False) if isinstance(value, dict) else False
                if not isinstance(optional, bool):
                    problems.append(Problem(location, f"{alias}: optional must be a boolean"))
                    continue
                identity = value
                if inherited:
                    if value["workspace"] is not True:
                        problems.append(Problem(location, f"{alias}: workspace must be true"))
                        continue
                    overrides = set(value) - INHERITED_DEPENDENCY_KEYS
                    if overrides:
                        problems.append(
                            Problem(
                                location,
                                f"{alias}: inherited dependency cannot override "
                                f"{', '.join(sorted(overrides))}; only optional, features, "
                                "and default-features may override workspace policy",
                            )
                        )
                        continue
                    if alias not in workspace_dependencies:
                        problems.append(Problem(location, f"{alias}: missing workspace.dependencies entry"))
                        continue
                    identity = workspace_dependencies[alias]
                package, error = _package_identity(alias, identity)
                if error is not None:
                    problems.append(Problem(location, f"{alias}: {error}"))
                    continue
                declarations.append(
                    DependencyDeclaration(table, kind, alias, package, optional, inherited)
                )
    return declarations, problems


def check_manifest_dependencies(
    relative: str,
    manifest: dict[str, object],
    workspace_dependencies: dict[str, object],
    rule: PackageRule,
) -> list[Problem]:
    """Require exact canonical identity unions, with no cross-kind authorization."""

    declarations, problems = _dependency_declarations(
        relative, manifest, workspace_dependencies
    )
    allowed_by_kind = {
        "dependencies": rule.dependencies,
        "dev-dependencies": rule.dev_dependencies,
        "build-dependencies": rule.build_dependencies,
    }
    identities = {kind: set() for kind in DEPENDENCY_KINDS}
    for declaration in declarations:
        identities[declaration.kind].add(declaration.package)
        if declaration.package not in allowed_by_kind[declaration.kind]:
            problems.append(
                Problem(
                    f"{relative}/Cargo.toml [{declaration.table}]",
                    f"undeclared dependency boundary: {declaration.package} "
                    f"(alias {declaration.alias!r}, "
                    f"optional={str(declaration.optional).lower()}, "
                    f"workspace={str(declaration.inherited).lower()}); "
                    "remove or move the declaration, or review an explicit kind-specific "
                    "PackageRule change in scripts/verify_architecture.py; "
                    "see docs/architecture/dependency-rules.md",
                )
            )
    for kind, allowed in allowed_by_kind.items():
        # Requirements apply to the union for a kind, not independently to each
        # target table. A platform-specific declaration never gains another kind.
        problems.extend(
            check_dependencies(
                f"{relative}/Cargo.toml [all {kind}]",
                identities[kind] & allowed,
                allowed,
            )
        )
    return problems


def verify(root: Path) -> list[Problem]:
    """Validate workspace membership, package metadata, and dependency direction."""

    problems: list[Problem] = []
    workspace_path = root / "Cargo.toml"
    if not workspace_path.is_file():
        return [Problem("Cargo.toml", "workspace manifest is missing")]

    workspace_manifest = _read_toml(workspace_path)
    workspace = workspace_manifest.get("workspace")
    if not isinstance(workspace, dict):
        return [Problem("Cargo.toml", "workspace table is missing")]

    raw_members = workspace.get("members", [])
    if not isinstance(raw_members, list) or not all(
        isinstance(member, str) for member in raw_members
    ):
        problems.append(Problem("Cargo.toml", "workspace members must be explicit strings"))
        members: set[str] = set()
    else:
        members = {member.replace("\\", "/").rstrip("/") for member in raw_members}
    problems.extend(check_workspace_members(members))

    workspace_dependencies = workspace.get("dependencies", {})
    if not isinstance(workspace_dependencies, dict):
        problems.append(Problem("Cargo.toml", "workspace.dependencies must be a table"))
        workspace_dependencies = {}

    for relative, rule in sorted(PACKAGE_RULES.items()):
        manifest_path = root / relative / "Cargo.toml"
        if not manifest_path.is_file():
            problems.append(Problem(relative, "package manifest is missing"))
            continue

        manifest = _read_toml(manifest_path)
        package = manifest.get("package")
        if not isinstance(package, dict):
            problems.append(Problem(relative, "package table is missing"))
            continue

        if package.get("name") != rule.name:
            problems.append(
                Problem(relative, f"package name must be {rule.name!r}")
            )
        if package.get("publish") is not False:
            problems.append(Problem(relative, "workspace package must set publish = false"))

        for field in sorted(INHERITED_PACKAGE_FIELDS):
            if package.get(field) != {"workspace": True}:
                problems.append(
                    Problem(relative, f"package field {field!r} must inherit workspace metadata")
                )

        if manifest.get("lints") != {"workspace": True}:
            problems.append(Problem(relative, "package must inherit workspace lints"))

        problems.extend(
            check_manifest_dependencies(relative, manifest, workspace_dependencies, rule)
        )

        source_name = "lib.rs" if rule.target == "lib" else "main.rs"
        source = root / relative / "src" / source_name
        if not source.is_file():
            problems.append(Problem(relative, f"required {rule.target} target is missing"))

    return sorted(set(problems))


def main(argv: list[str] | None = None) -> int:
    """Run the architecture verifier."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="repository root to validate",
    )
    arguments = parser.parse_args(argv)
    problems = verify(arguments.root.resolve())
    if problems:
        for problem in problems:
            print(problem.render(), file=sys.stderr)
        print(f"FAILED: {len(problems)} architecture problem(s)", file=sys.stderr)
        return 1
    print(f"OK: architecture verification passed for {arguments.root.resolve()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
