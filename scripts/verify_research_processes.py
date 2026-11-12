#!/usr/bin/env python3
"""Build real service executables, then run protected M04 process acceptance."""
from __future__ import annotations

import json
import os
import signal
from pathlib import Path
import subprocess
import sys


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    binaries = {
        "edgeagent-gateway": "EDGEAGENT_PROCESS_GATEWAY",
        "edgeagent-market-data": "EDGEAGENT_PROCESS_MARKET_DATA",
        "edgeagent-research": "EDGEAGENT_PROCESS_RESEARCH",
        "edgeagent-audit-projector": "EDGEAGENT_PROCESS_PROJECTOR",
    }
    for key in ("EDGEAGENT_POSTGRES_URL", "EDGEAGENT_NATS_URL"):
        if not os.environ.get(key):
            print(f"required process prerequisite unavailable: {key}", file=sys.stderr)
            return 1
    packages = tuple(binaries)
    binaries["qualify_explanation_process"] = "EDGEAGENT_PROCESS_EXPLANATION_QUALIFICATION"
    argv = ["cargo", "build", "--locked", "--bins", "--example", "qualify_explanation_process", "--message-format=json"]
    for package in packages:
        argv.extend(["-p", package])
    build = subprocess.run(argv, cwd=root, stdout=subprocess.PIPE, text=True, check=False)
    environment = os.environ.copy()
    found = set()
    for line in build.stdout.splitlines():
        item = json.loads(line)
        name = item.get("target", {}).get("name")
        if item.get("reason") == "compiler-artifact" and name in binaries and item.get("executable"):
            environment[binaries[name]] = item["executable"]
            found.add(name)
        if build.returncode and item.get("reason") == "compiler-message":
            print(item["message"].get("rendered", "build failed"), file=sys.stderr)
    if build.returncode:
        return build.returncode
    if found != set(binaries):
        print("build did not return every required service executable", file=sys.stderr)
        return 1
    test_args = sys.argv[1:]
    if len(test_args) > 5 and test_args[5] == "image_owner":
        if not os.environ.get("EDGEAGENT_RESEARCH_IMAGE"):
            print("required process prerequisite unavailable: EDGEAGENT_RESEARCH_IMAGE", file=sys.stderr)
            return 1
        environment["EDGEAGENT_IMAGE_UID"] = str(os.getuid())
        environment["EDGEAGENT_IMAGE_GID"] = str(os.getgid())
    if test_args[:5] != ["test", "--locked", "-p", "edgeagent-research", "--test"] or (
        len(test_args) < 6 or test_args[5] not in {"processes", "image_owner"}
    ):
        print("expected the protected Research process test command", file=sys.stderr)
        return 1
    process = subprocess.Popen(["cargo", *test_args], cwd=root, env=environment, start_new_session=True)
    try:
        return process.wait(timeout=300)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
        print("Research process qualification exceeded its deadline", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
