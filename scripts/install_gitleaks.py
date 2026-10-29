#!/usr/bin/env python3
"""Install the reviewed Gitleaks release after authenticating its archive digest."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import io
import os
from pathlib import Path
import platform
import re
import sys
import tarfile
import tempfile
import tomllib
import urllib.error
import urllib.request


MAX_ARCHIVE_BYTES = 32 * 1024 * 1024
MAX_BINARY_BYTES = 32 * 1024 * 1024
ROOT = Path(__file__).resolve().parents[1]


class SecurityToolError(RuntimeError):
    """Safe, caller-actionable failure that does not include subprocess output."""


@dataclass(frozen=True, slots=True)
class Archive:
    name: str
    sha256: str
    executable: str


@dataclass(frozen=True, slots=True)
class ToolPolicy:
    version: str
    archives: dict[str, Archive]


def load_policy(root: Path) -> ToolPolicy:
    """Validate the release pin and the supported Linux archive identity."""

    try:
        source = (root / "supply-chain/security-tools.toml").read_bytes()
        manifest = tomllib.loads(source.decode("utf-8"))
    except (OSError, UnicodeError, tomllib.TOMLDecodeError) as error:
        raise SecurityToolError("cannot read security tool policy") from error
    tool = manifest.get("gitleaks")
    if manifest.get("schema_version") != 1 or not isinstance(tool, dict):
        raise SecurityToolError("invalid security tool policy schema")
    version = tool.get("version")
    if not isinstance(version, str) or re.fullmatch(r"8\.\d+\.\d+", version) is None:
        raise SecurityToolError("Gitleaks requires an exact reviewed 8.x release")
    if tool.get("license") != "MIT" or tool.get("release_url") != (
        f"https://github.com/gitleaks/gitleaks/releases/tag/v{version}"
    ):
        raise SecurityToolError("invalid Gitleaks release or license identity")
    archives = {}
    for target, suffix, executable in (
        ("linux_x64", "tar.gz", "gitleaks"),
    ):
        entry = tool.get(target)
        expected_name = f"gitleaks_{version}_{target}.{suffix}"
        if (
            not isinstance(entry, dict)
            or entry.get("archive") != expected_name
            or entry.get("executable") != executable
            or not isinstance(entry.get("sha256"), str)
            or re.fullmatch(r"[0-9a-f]{64}", entry["sha256"]) is None
        ):
            raise SecurityToolError(f"invalid Gitleaks archive policy for {target}")
        archives[target] = Archive(expected_name, entry["sha256"], executable)
    return ToolPolicy(version, archives)


def host_target() -> str:
    """Fail explicitly instead of installing a binary for an unsupported host."""

    if platform.machine().lower() not in {"amd64", "x86_64"}:
        raise SecurityToolError("Gitleaks installer supports x64 Linux only")
    system = platform.system().lower()
    if system != "linux":
        raise SecurityToolError("Gitleaks installer supports x64 Linux only")
    return f"{system}_x64"


def authenticated_binary(payload: bytes, archive: Archive) -> bytes:
    """Authenticate before parsing; extract only one exact regular executable."""

    if len(payload) > MAX_ARCHIVE_BYTES or hashlib.sha256(payload).hexdigest() != archive.sha256:
        raise SecurityToolError("Gitleaks archive checksum verification failed")
    try:
        with tarfile.open(fileobj=io.BytesIO(payload), mode="r:gz") as source:
            entries = [entry for entry in source.getmembers() if entry.name == archive.executable]
            if len(entries) != 1 or not entries[0].isfile() or entries[0].size > MAX_BINARY_BYTES:
                raise SecurityToolError("Gitleaks archive has an invalid executable member")
            reader = source.extractfile(entries[0])
            if reader is None:
                raise SecurityToolError("Gitleaks archive executable could not be read")
            with reader:
                binary = reader.read(MAX_BINARY_BYTES + 1)
    except (OSError, tarfile.TarError, RuntimeError) as error:
        if isinstance(error, SecurityToolError):
            raise
        raise SecurityToolError("Gitleaks archive could not be decoded") from error
    if not binary or len(binary) > MAX_BINARY_BYTES:
        raise SecurityToolError("Gitleaks archive executable is empty or oversized")
    return binary


def install(root: Path, destination: Path) -> Path:
    """Download from the official release, authenticate, then install atomically."""

    policy = load_policy(root)
    archive = policy.archives[host_target()]
    url = f"https://github.com/gitleaks/gitleaks/releases/download/v{policy.version}/{archive.name}"
    try:
        with urllib.request.urlopen(url, timeout=30) as response:
            payload = response.read(MAX_ARCHIVE_BYTES + 1)
    except (OSError, urllib.error.URLError) as error:
        raise SecurityToolError("cannot download pinned Gitleaks archive") from error
    binary = authenticated_binary(payload, archive)
    destination = destination.absolute()
    if any(parent.is_symlink() for parent in (destination, *destination.parents)):
        raise SecurityToolError("Gitleaks install directory must not contain a symlink")
    destination = destination.resolve()
    destination.mkdir(parents=True, exist_ok=True)
    target = destination / archive.executable
    if target.is_symlink() or (target.exists() and not target.is_file()):
        raise SecurityToolError("Gitleaks install target must be a regular file")
    staged: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(prefix=".gitleaks-install-", dir=destination, delete=False) as output:
            staged = Path(output.name)
            output.write(binary)
        staged.chmod(0o755)
        os.replace(staged, target)
    finally:
        if staged is not None and staged.exists():
            staged.unlink()
    return target


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--destination", type=Path)
    options = parser.parse_args()
    try:
        policy = load_policy(ROOT)
        destination = options.destination or ROOT / f"artifacts/security-tools/gitleaks-{policy.version}"
        target = install(ROOT, destination)
        github_path = os.environ.get("GITHUB_PATH")
        if github_path:
            # GitHub supplies this runner-owned file. This grants no token or
            # secret; it exposes the verified executable to subsequent steps.
            with Path(github_path).open("a", encoding="utf-8") as output:
                output.write(f"{target.parent}\n")
    except (SecurityToolError, OSError) as error:
        message = str(error) if isinstance(error, SecurityToolError) else "cannot write security tool install directory"
        print(f"Gitleaks installation failed: {message}", file=sys.stderr)
        return 1
    print(f"Installed checksum-verified Gitleaks: {target}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
