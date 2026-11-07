#!/usr/bin/env python3
"""Own isolated dependencies for the packaged Research owner qualification."""
from __future__ import annotations

import os
from pathlib import Path
import socket
import subprocess
import sys
import time
import tomllib
import uuid


def docker(*args: str) -> str:
    result = subprocess.run(
        ["docker", *args], check=True, capture_output=True, text=True, timeout=60
    )
    return result.stdout.strip()


def port(name: str, internal: int) -> int:
    return int(docker("port", name, str(internal)).split(":")[-1])


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    profiles = tomllib.loads((root / "deploy/local/stack.toml").read_text(encoding="utf-8"))
    prefix = f"edgeagent-owner-{uuid.uuid4().hex}"
    pg, nats = f"{prefix}-pg", f"{prefix}-nats"
    owned: list[str] = []
    try:
        for name, service, internal, extra in [
            (pg, "postgres", 5432, ["-e", "POSTGRES_USER=edgeagent", "-e",
             "POSTGRES_PASSWORD=image-proof", "-e", "POSTGRES_DB=edgeagent"]),
            (nats, "nats", 4222, []),
        ]:
            owned.append(name)
            docker("run", "-d", "--name", name, "-p", f"127.0.0.1::{internal}",
                   *extra, profiles["services"][service]["image"],
                   *(["-js"] if service == "nats" else []))
        pg_port, nats_port = port(pg, 5432), port(nats, 4222)
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            ready = subprocess.run(
                ["docker", "exec", pg, "pg_isready", "-U", "edgeagent"],
                check=False, capture_output=True, timeout=5,
            ).returncode == 0
            if ready:
                try:
                    with socket.create_connection(("127.0.0.1", nats_port), timeout=1):
                        break
                except OSError:
                    pass
            time.sleep(0.1)
        else:
            raise RuntimeError("isolated image dependencies did not become ready")
        environment = os.environ.copy()
        environment.update(
            EDGEAGENT_POSTGRES_URL=f"postgresql://edgeagent:image-proof@127.0.0.1:{pg_port}/edgeagent",
            EDGEAGENT_NATS_URL=f"nats://127.0.0.1:{nats_port}",
            EDGEAGENT_RESEARCH_IMAGE="edgeagent-ci/research:test",
        )
        return subprocess.run(
            ["cargo", "xtask", "verify", "--check", "research-image-owner", "--python", sys.executable],
            cwd=root, env=environment, check=False, timeout=330,
        ).returncode
    finally:
        for name in reversed(owned):
            subprocess.run(["docker", "rm", "-f", "-v", name], check=False,
                           capture_output=True, timeout=30)


if __name__ == "__main__":
    raise SystemExit(main())
