#!/usr/bin/env python3
"""Validate a release tag and emit the manifest-derived OCI release matrix."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys

from verify_supply_chain import load_artifact_specs


SEMVER_PRERELEASE_IDENTIFIER = (
    r"(?:0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
)
SEMVER_TAG = re.compile(
    r"^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    rf"(?:-{SEMVER_PRERELEASE_IDENTIFIER}"
    rf"(?:\.{SEMVER_PRERELEASE_IDENTIFIER})*)?$"
)
REPOSITORY = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")
REGISTRY = re.compile(r"^[a-z0-9.-]+(?::[0-9]+)?$")


class ReleasePlanError(ValueError):
    """A caller-actionable release planning failure."""


def build_release_plan(
    root: Path,
    *,
    tag: str,
    repository: str,
    registry: str,
) -> tuple[str, dict[str, list[dict[str, str]]]]:
    """Return a safe version and one registry target per declared deployable."""

    if SEMVER_TAG.fullmatch(tag) is None:
        raise ReleasePlanError(
            "release tag must be vMAJOR.MINOR.PATCH with an optional SemVer prerelease"
        )
    if REPOSITORY.fullmatch(repository) is None:
        raise ReleasePlanError("repository must be an owner/name slug")
    if REGISTRY.fullmatch(registry) is None:
        raise ReleasePlanError("registry must be a lowercase registry hostname")

    specs, problems = load_artifact_specs(root / "deploy" / "images.toml")
    if problems:
        raise ReleasePlanError("; ".join(problem.render() for problem in problems))

    owner, name = repository.lower().split("/", 1)
    include = [
        {
            "service": spec.service,
            "binary": spec.binary,
            "image": f"{registry}/{owner}/{name}-{spec.service}",
        }
        for spec in specs
    ]
    return tag[1:], {"include": include}


def write_github_outputs(
    path: Path,
    *,
    version: str,
    matrix: dict[str, list[dict[str, str]]],
) -> None:
    """Append newline-safe scalar outputs to GitHub's step output file."""

    matrix_json = json.dumps(matrix, separators=(",", ":"), sort_keys=True)
    with path.open("a", encoding="utf-8", newline="\n") as output:
        output.write(f"version={version}\n")
        output.write(f"matrix={matrix_json}\n")


def main(argv: list[str] | None = None) -> int:
    """Build and emit a release plan for GitHub Actions."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--registry", required=True)
    parser.add_argument("--github-output", type=Path, required=True)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
    )
    arguments = parser.parse_args(argv)
    try:
        version, matrix = build_release_plan(
            arguments.root.resolve(),
            tag=arguments.tag,
            repository=arguments.repository,
            registry=arguments.registry,
        )
        write_github_outputs(arguments.github_output, version=version, matrix=matrix)
    except (OSError, ReleasePlanError) as error:
        print(f"FAILED: {error}", file=sys.stderr)
        return 1
    print(f"OK: planned {len(matrix['include'])} release images for {version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
