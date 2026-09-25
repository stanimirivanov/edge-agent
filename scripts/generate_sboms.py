#!/usr/bin/env python3
"""Generate per-deployable Rust and OCI-image CycloneDX SBOM artifacts."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

from verify_supply_chain import (
    ArtifactSpec,
    check_sbom,
    load_artifact_specs,
    load_tool_policy,
)


class GenerationError(RuntimeError):
    """A caller-actionable SBOM generation failure."""


def _run(
    command: list[str],
    *,
    root: Path,
    environment: dict[str, str] | None = None,
    capture: bool = False,
) -> subprocess.CompletedProcess[str]:
    try:
        result = subprocess.run(
            command,
            cwd=root,
            env=environment,
            check=False,
            text=True,
            capture_output=capture,
        )
    except FileNotFoundError as error:
        raise GenerationError(f"required executable is unavailable: {command[0]}") from error
    if result.returncode != 0:
        detail = result.stderr.strip() if capture else "see command output above"
        raise GenerationError(f"command failed ({' '.join(command)}): {detail}")
    return result


def _git_value(root: Path, arguments: list[str], fallback: str) -> str:
    result = _run(["git", *arguments], root=root, capture=True)
    return result.stdout.strip() or fallback


def _require_tool_version(root: Path, command: list[str], expected: str) -> None:
    """Fail rather than generating evidence with an unreviewed tool version."""

    result = _run(command, root=root, capture=True)
    output = f"{result.stdout}\n{result.stderr}"
    if expected not in output:
        raise GenerationError(
            f"{' '.join(command[:-1])} must report pinned version {expected!r}"
        )


def rust_sbom_command(
    *,
    cargo_cyclonedx: str,
    spec_version: str,
) -> list[str]:
    """Build the stable cargo-cyclonedx command for all workspace binaries."""

    return [
        cargo_cyclonedx,
        "cyclonedx",
        "--manifest-path",
        "Cargo.toml",
        "--format",
        "json",
        "--describe",
        "binaries",
        "--all-features",
        "--target",
        "all",
        "--license-strict",
        "--spec-version",
        spec_version,
    ]


def image_sbom_command(spec: ArtifactSpec, *, syft: str, output: Path) -> list[str]:
    """Build the stable Syft command for one local OCI image."""

    tag = f"edgeagent-ci/{spec.service}:sbom"
    return [
        syft,
        "scan",
        f"docker:{tag}",
        "--quiet",
        "--source-name",
        spec.binary,
        "--output",
        f"cyclonedx-json={output}",
    ]


def generate_rust_sboms(
    root: Path,
    output: Path,
    specs: list[ArtifactSpec],
    *,
    cargo_cyclonedx: str,
    spec_version: str,
) -> None:
    """Generate one dependency-complete Cargo SBOM per deployable binary."""

    output.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    environment.setdefault(
        "SOURCE_DATE_EPOCH",
        _git_value(root, ["log", "-1", "--format=%ct"], "0"),
    )
    generated_files = {
        spec: root / "services" / spec.service / f"{spec.binary}_bin.cdx.json"
        for spec in specs
    }
    for generated in generated_files.values():
        if generated.exists():
            raise GenerationError(f"refusing to overwrite unexpected generated file: {generated}")
    try:
        _run(
            rust_sbom_command(
                cargo_cyclonedx=cargo_cyclonedx,
                spec_version=spec_version,
            ),
            root=root,
            environment=environment,
        )
        for spec, generated in generated_files.items():
            destination = output / spec.source_filename
            if not generated.is_file():
                raise GenerationError(f"cargo-cyclonedx did not produce {generated}")
            destination.unlink(missing_ok=True)
            shutil.move(str(generated), destination)
            print(f"generated {destination}")
    finally:
        for generated in generated_files.values():
            generated.unlink(missing_ok=True)


def generate_image_sboms(
    root: Path,
    output: Path,
    specs: list[ArtifactSpec],
    *,
    docker: str,
    syft: str,
) -> None:
    """Build every declared image and generate its runtime-filesystem SBOM."""

    output.mkdir(parents=True, exist_ok=True)
    _run([docker, "version"], root=root, capture=True)
    revision = os.environ.get(
        "GITHUB_SHA",
        _git_value(root, ["rev-parse", "HEAD"], "local"),
    )
    environment = os.environ.copy()
    environment["SYFT_CHECK_FOR_APP_UPDATE"] = "false"
    tags: list[str] = []
    try:
        for spec in specs:
            tag = f"edgeagent-ci/{spec.service}:sbom"
            tags.append(tag)
            destination = (output / spec.image_filename).resolve()
            destination.unlink(missing_ok=True)
            _run(
                [
                    docker,
                    "build",
                    "--file",
                    "Dockerfile",
                    "--build-arg",
                    f"SERVICE={spec.service}",
                    "--build-arg",
                    f"SOURCE_REVISION={revision}",
                    "--tag",
                    tag,
                    ".",
                ],
                root=root,
            )
            _run(
                image_sbom_command(spec, syft=syft, output=destination),
                root=root,
                environment=environment,
            )
            if not destination.is_file():
                raise GenerationError(f"Syft did not produce {destination}")
            print(f"generated {destination}")
    finally:
        for tag in tags:
            subprocess.run(
                [docker, "image", "rm", "--force", tag],
                cwd=root,
                check=False,
                text=True,
                capture_output=True,
            )


def main(argv: list[str] | None = None) -> int:
    """Run one explicit SBOM generation mode."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("rust", "images"))
    parser.add_argument("--output", type=Path, default=Path("artifacts/sbom"))
    parser.add_argument("--cargo-cyclonedx", default="cargo-cyclonedx")
    parser.add_argument("--docker", default="docker")
    parser.add_argument("--syft", default="syft")
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
    )
    arguments = parser.parse_args(argv)
    root = arguments.root.resolve()
    output = arguments.output
    if not output.is_absolute():
        output = root / output

    tools, tool_problems = load_tool_policy(root / "supply-chain" / "tools.toml")
    specs, spec_problems = load_artifact_specs(root / "deploy" / "images.toml")
    problems = [*tool_problems, *spec_problems]
    if tools is None or problems:
        for problem in problems:
            print(problem.render(), file=sys.stderr)
        return 1

    try:
        if arguments.mode == "rust":
            _require_tool_version(
                root,
                [arguments.cargo_cyclonedx, "cyclonedx", "--version"],
                tools.cargo_cyclonedx,
            )
            generate_rust_sboms(
                root,
                output,
                specs,
                cargo_cyclonedx=arguments.cargo_cyclonedx,
                spec_version=tools.rust_spec,
            )
        else:
            _require_tool_version(root, [arguments.syft, "version"], tools.syft)
            generate_image_sboms(
                root,
                output,
                specs,
                docker=arguments.docker,
                syft=arguments.syft,
            )
        filenames = (
            [spec.source_filename for spec in specs]
            if arguments.mode == "rust"
            else [spec.image_filename for spec in specs]
        )
        artifact_problems = [
            problem
            for filename in filenames
            for problem in check_sbom(output / filename, tools.accepted_specs)
        ]
        if artifact_problems:
            for problem in artifact_problems:
                print(problem.render(), file=sys.stderr)
            return 1
    except GenerationError as error:
        print(f"FAILED: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
