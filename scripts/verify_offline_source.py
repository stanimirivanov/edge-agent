#!/usr/bin/env python3
"""Run the invented source proof in a fresh Linux network namespace, without downloads."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys


def main() -> int:
    if sys.platform != "linux":
        print("Offline network-denial proof not run: Linux network namespaces required.")
        return 1
    root = Path(__file__).resolve().parents[1]
    built = subprocess.run(
        ["cargo", "test", "--offline", "--locked", "-p", "edgeagent-market-data-replay",
         "--test", "offline", "--no-run", "--message-format=json"],
        cwd=root, text=True, capture_output=True, check=False,
    )
    if built.returncode:
        print(built.stderr)
        return built.returncode
    executables = []
    for line in built.stdout.splitlines():
        item = json.loads(line)
        if (item.get("reason") == "compiler-artifact"
                and item.get("target", {}).get("name") == "offline"
                and item.get("executable")):
            executables.append(item["executable"])
    if len(executables) != 1:
        print("Offline proof failed: expected one compiled test executable.")
        return 1
    # New namespace has no external interfaces/routes; elevation is isolated to this
    # synthetic child. Build first, then run directly, so Cargo needs no network here.
    command = ["sudo", "-n", "unshare", "--net", "--", executables[0],
               "--test-threads=1"]
    return subprocess.run(command, cwd=root, check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main())
