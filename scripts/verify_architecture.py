#!/usr/bin/env python3
"""Verify EdgeAgent's initial Cargo workspace and dependency boundaries."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import sys
import tomllib


@dataclass(frozen=True)
class PackageRule:
    """Expected identity, target type, and direct dependencies for one package."""

    name: str
    target: str
    dependencies: frozenset[str]


@dataclass(frozen=True, order=True)
class Problem:
    """A deterministic architecture verification failure."""

    path: str
    message: str

    def render(self) -> str:
        """Return a compiler-style failure description."""

        return f"{self.path}: {self.message}"


CONTRACTS = "edgeagent-contracts"
SERVICE_RUNTIME = "edgeagent-service-runtime"
CONTRACT_DEPENDENCIES = frozenset({"cloudevents", "serde", "serde_json", "url"})
SERVICE_DEPENDENCIES = frozenset({CONTRACTS, SERVICE_RUNTIME})

PACKAGE_RULES = {
    "crates/contracts": PackageRule(CONTRACTS, "lib", CONTRACT_DEPENDENCIES),
    "crates/service-runtime": PackageRule(
        SERVICE_RUNTIME,
        "lib",
        frozenset({CONTRACTS}),
    ),
    "services/audit-projector": PackageRule(
        "edgeagent-audit-projector",
        "bin",
        SERVICE_DEPENDENCIES,
    ),
    "services/execution-simulator": PackageRule(
        "edgeagent-execution-simulator",
        "bin",
        SERVICE_DEPENDENCIES,
    ),
    "services/gateway": PackageRule(
        "edgeagent-gateway",
        "bin",
        SERVICE_DEPENDENCIES,
    ),
    "services/market-data": PackageRule(
        "edgeagent-market-data",
        "bin",
        SERVICE_DEPENDENCIES,
    ),
    "services/research": PackageRule(
        "edgeagent-research",
        "bin",
        SERVICE_DEPENDENCIES,
    ),
}

INHERITED_PACKAGE_FIELDS = {
    "edition",
    "license",
    "repository",
    "rust-version",
    "version",
}


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
            problems.append(Problem(relative, "foundation package must set publish = false"))

        for field in sorted(INHERITED_PACKAGE_FIELDS):
            if package.get(field) != {"workspace": True}:
                problems.append(
                    Problem(relative, f"package field {field!r} must inherit workspace metadata")
                )

        if manifest.get("lints") != {"workspace": True}:
            problems.append(Problem(relative, "package must inherit workspace lints"))

        raw_dependencies = manifest.get("dependencies", {})
        if not isinstance(raw_dependencies, dict):
            problems.append(Problem(relative, "dependencies must be a table"))
            dependencies: set[str] = set()
        else:
            dependencies = set(raw_dependencies)
        problems.extend(check_dependencies(relative, dependencies, rule.dependencies))

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
