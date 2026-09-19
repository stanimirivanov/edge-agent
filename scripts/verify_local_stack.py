#!/usr/bin/env python3
"""Verify the local dependency profile and optionally wait for readiness."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import socket
import sys
import time
import tomllib
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


EXPECTED_SERVICES = {"nats", "object-store", "otel-collector", "postgres"}
REQUIRED_ENVIRONMENT = {
    "EDGEAGENT_POSTGRES_DB",
    "EDGEAGENT_POSTGRES_PASSWORD",
    "EDGEAGENT_POSTGRES_USER",
    "EDGEAGENT_S3_ACCESS_KEY",
    "EDGEAGENT_S3_BUCKET",
    "EDGEAGENT_S3_SECRET_KEY",
}


@dataclass(frozen=True, order=True)
class Problem:
    """A stable local-platform verification failure."""

    path: str
    message: str

    def render(self) -> str:
        """Return a compiler-style failure description."""

        return f"{self.path}: {self.message}"


@dataclass(frozen=True)
class ServiceSpec:
    """One dependency and its local readiness contract."""

    name: str
    image: str
    license: str
    ports: tuple[int, ...]
    readiness: str
    readiness_port: int
    readiness_path: str


def _load_toml(path: Path) -> dict[str, object]:
    with path.open("rb") as source:
        return tomllib.load(source)


def _is_pinned_image(image: str) -> bool:
    """Return whether an image uses a digest or a non-floating version tag."""

    if "@sha256:" in image:
        return True
    final_segment = image.rsplit("/", 1)[-1]
    if ":" not in final_segment:
        return False
    tag = final_segment.rsplit(":", 1)[-1]
    return tag not in {"latest", "stable", "main", "master"}


def load_manifest(path: Path) -> tuple[list[ServiceSpec], list[Problem]]:
    """Parse the local dependency manifest."""

    if not path.is_file():
        return [], [Problem(path.as_posix(), "local stack manifest is missing")]

    manifest = _load_toml(path)
    problems: list[Problem] = []
    relative = "deploy/local/stack.toml"
    if manifest.get("schema_version") != 1:
        problems.append(Problem(relative, "schema_version must be 1"))
    raw_services = manifest.get("services")
    if not isinstance(raw_services, dict):
        return [], problems + [Problem(relative, "services table is missing")]

    service_names = set(raw_services)
    for name in sorted(service_names - EXPECTED_SERVICES):
        problems.append(Problem(relative, f"unexpected local service: {name}"))
    for name in sorted(EXPECTED_SERVICES - service_names):
        problems.append(Problem(relative, f"required local service is missing: {name}"))

    specs = []
    for name, raw_spec in sorted(raw_services.items()):
        if not isinstance(name, str) or not isinstance(raw_spec, dict):
            problems.append(Problem(relative, f"invalid local service entry: {name}"))
            continue
        image = raw_spec.get("image")
        license_name = raw_spec.get("license")
        ports = raw_spec.get("ports")
        readiness = raw_spec.get("readiness")
        readiness_port = raw_spec.get("readiness_port")
        readiness_path = raw_spec.get("readiness_path", "")
        if not isinstance(image, str) or not _is_pinned_image(image):
            problems.append(Problem(relative, f"service {name} image must use a pinned tag or digest"))
            continue
        if not isinstance(license_name, str) or not license_name:
            problems.append(Problem(relative, f"service {name} license must be declared"))
            continue
        if not isinstance(ports, list) or not ports or not all(
            isinstance(port, int) and 0 < port < 65_536 for port in ports
        ):
            problems.append(Problem(relative, f"service {name} ports are invalid"))
            continue
        if len(ports) != len(set(ports)):
            problems.append(Problem(relative, f"service {name} ports must be unique"))
            continue
        if readiness not in {"http", "http-client-response", "tcp"}:
            problems.append(Problem(relative, f"service {name} readiness type is invalid"))
            continue
        if not isinstance(readiness_port, int) or readiness_port not in ports:
            problems.append(Problem(relative, f"service {name} readiness port must be exposed"))
            continue
        if readiness != "tcp" and (
            not isinstance(readiness_path, str) or not readiness_path.startswith("/")
        ):
            problems.append(Problem(relative, f"service {name} readiness path is invalid"))
            continue

        specs.append(
            ServiceSpec(
                name=name,
                image=image,
                license=license_name,
                ports=tuple(ports),
                readiness=readiness,
                readiness_port=readiness_port,
                readiness_path=readiness_path if isinstance(readiness_path, str) else "",
            )
        )
    return specs, problems


def _load_environment(path: Path) -> tuple[dict[str, str], list[Problem]]:
    if not path.is_file():
        return {}, [Problem("deploy/local/.env.example", "local environment example is missing")]

    values: dict[str, str] = {}
    problems = []
    for line_number, raw_line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        key, separator, value = line.partition("=")
        if not separator or not key or not value:
            problems.append(
                Problem("deploy/local/.env.example", f"invalid entry on line {line_number}")
            )
            continue
        if key in values:
            problems.append(
                Problem("deploy/local/.env.example", f"duplicate environment key: {key}")
            )
        values[key] = value

    for key in sorted(set(values) - REQUIRED_ENVIRONMENT):
        problems.append(Problem("deploy/local/.env.example", f"unexpected environment key: {key}"))
    for key in sorted(REQUIRED_ENVIRONMENT - set(values)):
        problems.append(Problem("deploy/local/.env.example", f"required environment key is missing: {key}"))
    for key, value in values.items():
        if key.endswith(("PASSWORD", "ACCESS_KEY", "SECRET_KEY")) and "local" not in value:
            problems.append(
                Problem(
                    "deploy/local/.env.example",
                    f"development-only credential must be visibly local: {key}",
                )
            )
    return values, problems


def verify(root: Path) -> tuple[list[ServiceSpec], list[Problem]]:
    """Verify manifests, localhost exposure, credentials, and collector policy."""

    local_root = root / "deploy" / "local"
    specs, problems = load_manifest(local_root / "stack.toml")
    environment, environment_problems = _load_environment(local_root / ".env.example")
    problems.extend(environment_problems)

    compose_path = local_root / "compose.yaml"
    if not compose_path.is_file():
        return specs, problems + [Problem("deploy/local/compose.yaml", "Compose file is missing")]
    compose = compose_path.read_text(encoding="utf-8")
    for spec in specs:
        if f"image: {spec.image}" not in compose:
            problems.append(
                Problem("deploy/local/compose.yaml", f"service image is not declared: {spec.image}")
            )
        for port in spec.ports:
            mapping = f'"127.0.0.1:{port}:{port}"'
            if mapping not in compose:
                problems.append(
                    Problem(
                        "deploy/local/compose.yaml",
                        f"service {spec.name} port must be bound to localhost: {port}",
                    )
                )
    for key in sorted(environment):
        if f"${{{key}}}" not in compose:
            problems.append(
                Problem("deploy/local/compose.yaml", f"environment key is unused: {key}")
            )
    for forbidden in ("0.0.0.0:", "network_mode: host", "privileged: true"):
        if forbidden in compose:
            problems.append(
                Problem("deploy/local/compose.yaml", f"forbidden local exposure: {forbidden}")
            )
    if compose.count("no-new-privileges:true") != len(EXPECTED_SERVICES):
        problems.append(
            Problem(
                "deploy/local/compose.yaml",
                "every local service must enable no-new-privileges",
            )
        )
    if "--jetstream" not in compose or "--store_dir=/data" not in compose:
        problems.append(
            Problem("deploy/local/compose.yaml", "NATS must enable persistent JetStream")
        )
    if "http://127.0.0.1:9333/cluster/status" not in compose:
        problems.append(
            Problem(
                "deploy/local/compose.yaml",
                "SeaweedFS must expose an internal cluster health check",
            )
        )

    collector_path = local_root / "otel-collector.yaml"
    if not collector_path.is_file():
        problems.append(
            Problem("deploy/local/otel-collector.yaml", "collector configuration is missing")
        )
    else:
        collector = collector_path.read_text(encoding="utf-8")
        for fragment in (
            "health_check:",
            "endpoint: 0.0.0.0:13133",
            "grpc:",
            "endpoint: 0.0.0.0:4317",
            "http:",
            "endpoint: 0.0.0.0:4318",
            "logs:",
            "metrics:",
            "traces:",
        ):
            if fragment not in collector:
                problems.append(
                    Problem(
                        "deploy/local/otel-collector.yaml",
                        f"required collector contract is missing: {fragment}",
                    )
                )

    return specs, sorted(set(problems))


def _ready(spec: ServiceSpec) -> bool:
    if spec.readiness == "tcp":
        try:
            with socket.create_connection(("127.0.0.1", spec.readiness_port), timeout=1):
                return True
        except OSError:
            return False

    request = Request(
        f"http://127.0.0.1:{spec.readiness_port}{spec.readiness_path}",
        headers={"User-Agent": "edgeagent-local-readiness/1"},
    )
    try:
        with urlopen(request, timeout=1) as response:
            return 200 <= response.status < 400
    except HTTPError as error:
        return spec.readiness == "http-client-response" and 400 <= error.code < 500
    except (OSError, URLError):
        return False


def wait_until_ready(specs: list[ServiceSpec], timeout_seconds: float) -> list[Problem]:
    """Wait for every declared local dependency to satisfy its readiness contract."""

    deadline = time.monotonic() + timeout_seconds
    pending = {spec.name: spec for spec in specs}
    while pending and time.monotonic() < deadline:
        for name, spec in list(pending.items()):
            if _ready(spec):
                del pending[name]
        if pending:
            time.sleep(0.5)
    return [
        Problem(name, f"did not become ready within {timeout_seconds:g} seconds")
        for name in sorted(pending)
    ]


def main(argv: list[str] | None = None) -> int:
    """Run static validation and optional live readiness checks."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="repository root to validate",
    )
    parser.add_argument(
        "--running",
        action="store_true",
        help="also wait for every localhost dependency endpoint",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=90.0,
        help="readiness timeout in seconds",
    )
    arguments = parser.parse_args(argv)
    root = arguments.root.resolve()
    specs, problems = verify(root)
    if arguments.running and not problems:
        problems.extend(wait_until_ready(specs, arguments.timeout))
    if problems:
        for problem in sorted(set(problems)):
            print(problem.render(), file=sys.stderr)
        print(f"FAILED: {len(set(problems))} local platform problem(s)", file=sys.stderr)
        return 1
    mode = "local platform and readiness" if arguments.running else "local platform contract"
    print(f"OK: {mode} verification passed for {root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
