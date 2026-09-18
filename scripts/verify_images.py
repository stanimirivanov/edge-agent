#!/usr/bin/env python3
"""Verify and optionally smoke-test EdgeAgent OCI image contracts."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib


EXPECTED_SERVICES = {
    "audit-projector",
    "execution-simulator",
    "gateway",
    "market-data",
    "research",
}
TOKEN = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")


@dataclass(frozen=True, order=True)
class Problem:
    """A stable image-contract verification failure."""

    path: str
    message: str

    def render(self) -> str:
        """Return a compiler-style failure description."""

        return f"{self.path}: {self.message}"


@dataclass(frozen=True)
class ImageSpec:
    """One service image declared by the checked-in manifest."""

    service: str
    package: str
    binary: str
    component: str
    capabilities: tuple[str, ...]


@dataclass(frozen=True)
class BuildContract:
    """Shared OCI build and runtime properties."""

    repository: str
    builder_image: str
    runtime_image: str
    runtime_user: str
    entrypoint: str


def _load_toml(path: Path) -> dict[str, object]:
    with path.open("rb") as source:
        return tomllib.load(source)


def load_manifest(path: Path) -> tuple[BuildContract | None, list[ImageSpec], list[Problem]]:
    """Parse and validate the image manifest without contacting a registry."""

    relative = path.as_posix()
    if not path.is_file():
        return None, [], [Problem(relative, "image manifest is missing")]

    manifest = _load_toml(path)
    problems: list[Problem] = []
    if manifest.get("schema_version") != 1:
        problems.append(Problem(relative, "schema_version must be 1"))

    repository = manifest.get("repository")
    build = manifest.get("build")
    if not isinstance(repository, str) or not repository:
        problems.append(Problem(relative, "repository must be a non-empty string"))
    if not isinstance(build, dict):
        problems.append(Problem(relative, "build table is missing"))
        return None, [], problems

    build_fields = {
        field: build.get(field)
        for field in ("builder_image", "runtime_image", "runtime_user", "entrypoint")
    }
    for field, value in build_fields.items():
        if not isinstance(value, str) or not value:
            problems.append(Problem(relative, f"build.{field} must be a non-empty string"))
    if problems or not isinstance(repository, str):
        return None, [], problems

    contract = BuildContract(repository=repository, **build_fields)  # type: ignore[arg-type]
    raw_images = manifest.get("images")
    if not isinstance(raw_images, dict):
        return contract, [], problems + [Problem(relative, "images table is missing")]

    services = set(raw_images)
    for service in sorted(services - EXPECTED_SERVICES):
        problems.append(Problem(relative, f"unexpected image service: {service}"))
    for service in sorted(EXPECTED_SERVICES - services):
        problems.append(Problem(relative, f"required image service is missing: {service}"))

    specs = []
    for service, raw_spec in sorted(raw_images.items()):
        if not isinstance(service, str) or not isinstance(raw_spec, dict):
            problems.append(Problem(relative, f"invalid image entry: {service}"))
            continue
        values = {
            field: raw_spec.get(field)
            for field in ("package", "binary", "component")
        }
        if not all(isinstance(value, str) and value for value in values.values()):
            problems.append(Problem(relative, f"image {service} has invalid identity fields"))
            continue
        capabilities = raw_spec.get("capabilities")
        if not isinstance(capabilities, list) or not all(
            isinstance(capability, str) for capability in capabilities
        ):
            problems.append(Problem(relative, f"image {service} capabilities must be strings"))
            continue

        expected_package = f"edgeagent-{service}"
        if values["package"] != expected_package or values["binary"] != expected_package:
            problems.append(
                Problem(relative, f"image {service} package and binary must be {expected_package}")
            )
        expected_component = f"edgeagent.{service}"
        if values["component"] != expected_component:
            problems.append(
                Problem(relative, f"image {service} component must be {expected_component}")
            )
        if not capabilities or capabilities != sorted(set(capabilities)):
            problems.append(
                Problem(relative, f"image {service} capabilities must be unique and sorted")
            )
        for capability in capabilities:
            if TOKEN.fullmatch(capability) is None:
                problems.append(
                    Problem(relative, f"image {service} has invalid capability: {capability}")
                )

        specs.append(
            ImageSpec(
                service=service,
                package=values["package"],  # type: ignore[arg-type]
                binary=values["binary"],  # type: ignore[arg-type]
                component=values["component"],  # type: ignore[arg-type]
                capabilities=tuple(capabilities),
            )
        )

    return contract, specs, problems


def check_descriptor(spec: ImageSpec, output: str) -> list[Problem]:
    """Validate the descriptor emitted from a built container image."""

    values: dict[str, str] = {}
    problems: list[Problem] = []
    for line in output.splitlines():
        key, separator, value = line.partition("=")
        if not separator or not key or key in values:
            problems.append(Problem(spec.service, f"invalid descriptor line: {line!r}"))
            continue
        values[key] = value

    expected = {
        "contract": "edgeagent.component.v1",
        "component": spec.component,
        "capabilities": ",".join(spec.capabilities),
    }
    for key, value in expected.items():
        if values.get(key) != value:
            problems.append(
                Problem(spec.service, f"descriptor {key} must be {value!r}")
            )
    if not values.get("summary"):
        problems.append(Problem(spec.service, "descriptor summary must not be empty"))
    if set(values) != {"contract", "component", "summary", "capabilities"}:
        problems.append(Problem(spec.service, "descriptor fields do not match the contract"))
    return problems


def verify(root: Path) -> tuple[BuildContract | None, list[ImageSpec], list[Problem]]:
    """Verify manifest, Docker build policy, and package mappings."""

    manifest_path = root / "deploy" / "images.toml"
    contract, specs, problems = load_manifest(manifest_path)
    dockerfile_path = root / "Dockerfile"
    if not dockerfile_path.is_file():
        return contract, specs, problems + [Problem("Dockerfile", "file is missing")]

    dockerfile = dockerfile_path.read_text(encoding="utf-8")
    if contract is not None:
        required_fragments = {
            f"ARG RUST_IMAGE={contract.builder_image}": "pinned builder default",
            f"FROM {contract.runtime_image} AS runtime": "minimal runtime stage",
            f"USER {contract.runtime_user}": "non-root runtime user",
            f'ENTRYPOINT ["{contract.entrypoint}"]': "fixed service entrypoint",
            "cargo build --locked --release --workspace --bins": "locked release build",
            "org.opencontainers.image.source": "OCI source label",
            "org.opencontainers.image.revision": "OCI revision label",
            "org.opencontainers.image.licenses": "OCI license label",
        }
        for fragment, purpose in required_fragments.items():
            if fragment not in dockerfile:
                problems.append(Problem("Dockerfile", f"missing {purpose}: {fragment}"))

        toolchain = _load_toml(root / "rust-toolchain.toml").get("toolchain")
        channel = toolchain.get("channel") if isinstance(toolchain, dict) else None
        if not isinstance(channel, str) or not contract.builder_image.startswith(
            f"rust:{channel}-"
        ):
            problems.append(
                Problem(
                    "deploy/images.toml",
                    "builder image must use the Rust version pinned by rust-toolchain.toml",
                )
            )

    dockerignore_path = root / ".dockerignore"
    if not dockerignore_path.is_file():
        problems.append(Problem(".dockerignore", "file is missing"))
    else:
        patterns = set(dockerignore_path.read_text(encoding="utf-8").splitlines())
        required_patterns = {
            "*",
            "!Cargo.lock",
            "!Cargo.toml",
            "!Dockerfile",
            "!rust-toolchain.toml",
            "!crates/",
            "!crates/**",
            "!services/",
            "!services/**",
        }
        for pattern in sorted(required_patterns - patterns):
            problems.append(
                Problem(".dockerignore", f"required build-context pattern is missing: {pattern}")
            )

    for spec in specs:
        manifest_path = root / "services" / spec.service / "Cargo.toml"
        if not manifest_path.is_file():
            problems.append(Problem(spec.service, "service Cargo manifest is missing"))
            continue
        package = _load_toml(manifest_path).get("package")
        if not isinstance(package, dict) or package.get("name") != spec.package:
            problems.append(Problem(spec.service, "image package does not match Cargo package"))
        if spec.service not in dockerfile:
            problems.append(Problem("Dockerfile", f"service allowlist omits {spec.service}"))

    return contract, specs, sorted(set(problems))


def _run(command: list[str], *, root: Path, capture: bool = False) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=root,
        check=False,
        text=True,
        capture_output=capture,
    )


def smoke_images(
    root: Path,
    contract: BuildContract,
    specs: list[ImageSpec],
) -> list[Problem]:
    """Build and execute every image with a locked-down Docker runtime."""

    problems: list[Problem] = []
    revision = os.environ.get("GITHUB_SHA", "local")
    tags: list[str] = []
    try:
        available = _run(["docker", "version"], root=root, capture=True)
    except FileNotFoundError:
        return [Problem("docker", "Docker CLI is unavailable; image smoke tests cannot run")]
    if available.returncode != 0:
        return [Problem("docker", "Docker daemon is unavailable; image smoke tests cannot run")]

    try:
        for spec in specs:
            tag = f"edgeagent-ci/{spec.service}:test"
            tags.append(tag)
            build = _run(
                [
                    "docker",
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
            if build.returncode != 0:
                problems.append(Problem(spec.service, "container image build failed"))
                break

            inspect_user = _run(
                ["docker", "image", "inspect", "--format={{.Config.User}}", tag],
                root=root,
                capture=True,
            )
            if inspect_user.returncode != 0 or inspect_user.stdout.strip() != contract.runtime_user:
                problems.append(Problem(spec.service, "image runtime user is not the declared non-root user"))

            inspect_entrypoint = _run(
                ["docker", "image", "inspect", "--format={{json .Config.Entrypoint}}", tag],
                root=root,
                capture=True,
            )
            expected_entrypoint = f'["{contract.entrypoint}"]'
            if (
                inspect_entrypoint.returncode != 0
                or inspect_entrypoint.stdout.strip() != expected_entrypoint
            ):
                problems.append(Problem(spec.service, "image entrypoint does not match the contract"))

            run = _run(
                [
                    "docker",
                    "run",
                    "--rm",
                    "--network=none",
                    "--read-only",
                    "--cap-drop=ALL",
                    "--security-opt=no-new-privileges=true",
                    tag,
                    "describe",
                ],
                root=root,
                capture=True,
            )
            if run.returncode != 0:
                problems.append(Problem(spec.service, "locked-down descriptor smoke test failed"))
            else:
                problems.extend(check_descriptor(spec, run.stdout))

        invalid = _run(
            [
                "docker",
                "build",
                "--file",
                "Dockerfile",
                "--build-arg",
                "SERVICE=not-a-service",
                "--tag",
                "edgeagent-ci/invalid:test",
                ".",
            ],
            root=root,
            capture=True,
        )
        if invalid.returncode == 0:
            problems.append(Problem("Dockerfile", "unsupported service build unexpectedly succeeded"))
            tags.append("edgeagent-ci/invalid:test")
    finally:
        for tag in tags:
            _run(["docker", "image", "rm", "--force", tag], root=root, capture=True)

    return sorted(set(problems))


def main(argv: list[str] | None = None) -> int:
    """Run structural verification and optional Docker smoke tests."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build", action="store_true", help="build and smoke-test every image")
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="repository root to validate",
    )
    arguments = parser.parse_args(argv)
    root = arguments.root.resolve()
    contract, specs, problems = verify(root)
    if arguments.build and contract is not None and not problems:
        problems.extend(smoke_images(root, contract, specs))
    if problems:
        for problem in sorted(set(problems)):
            print(problem.render(), file=sys.stderr)
        print(f"FAILED: {len(set(problems))} image problem(s)", file=sys.stderr)
        return 1
    mode = "image build and smoke" if arguments.build else "image contract"
    print(f"OK: {mode} verification passed for {root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
