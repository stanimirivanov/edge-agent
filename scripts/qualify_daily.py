#!/usr/bin/env python3
"""Opt-in supervision, not authorization or certification, of daily qualification."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
from collections.abc import Callable, Mapping, Sequence
from typing import TextIO


ROOT = Path(__file__).resolve().parents[1]
PHASES = {
    "preflight": ("qualification-preflight", 60),
    "acquire": ("qualification-worker", 420),
    "publish": ("qualification-relay", 300),
    "cleanup": ("qualification-maintain", 420),
}


class HarnessError(Exception):
    """Only static categories cross the diagnostics boundary."""


@dataclass(frozen=True)
class Invocation:
    phase: str
    profile: Path
    profile_sha256: str
    binary: Path
    binary_sha256: str
    report: Path
    execute_profile: str | None = None


def _is_digest(value: str) -> bool:
    return re.fullmatch(r"[0-9a-f]{64}", value) is not None


def _hash_file(path: Path, limit: int) -> str:
    if not path.is_absolute() or not stat.S_ISREG(path.lstat().st_mode):
        raise HarnessError("invalid_input_file")
    with path.open("rb") as stream:
        if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
            raise HarnessError("invalid_input_file")
        digest = hashlib.sha256()
        size = 0
        while chunk := stream.read(65536):
            size += len(chunk)
            if size > limit:
                raise HarnessError("input_size_limit")
            digest.update(chunk)
        if not size:
            raise HarnessError("empty_input_file")
        return digest.hexdigest()


def _open_report(path: Path) -> TextIO:
    # Never overwrite an older outcome or place private operator reports in Git.
    try:
        private = path.is_absolute() and not path.resolve().is_relative_to(ROOT)
    except (OSError, RuntimeError, ValueError):
        raise HarnessError("invalid_report_path") from None
    if not private:
        raise HarnessError("private_absolute_report_required")
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    return os.fdopen(descriptor, "w", encoding="utf-8", newline="\n")


def _persist(stream: TextIO, report: dict) -> None:
    stream.seek(0)
    stream.write(json.dumps(report, sort_keys=True, separators=(",", ":")) + "\n")
    stream.truncate()
    stream.flush()
    os.fsync(stream.fileno())


def supervise(
    invocation: Invocation,
    *,
    environment: Mapping[str, str] | None = None,
    runner: Callable = subprocess.run,
) -> tuple[int, str]:
    """Write a private receipt; default mode never reads environment or starts work.

    A command receipt cannot prove a provider call, consumer acceptance, physical
    quiescence or licensed-copy erasure. Unknown outcomes must be reconciled by
    the existing owner controls, never retried or compensated by this harness.
    """
    report = {
        "version": "edgeagent.qualification-command-report.v1",
        "harness_sha256": _hash_file(Path(__file__).resolve(), 256 * 1024),
        "phase": invocation.phase if invocation.phase in PHASES else "invalid",
        "timeout_seconds": PHASES[invocation.phase][1] if invocation.phase in PHASES else None,
        "profile_sha256": invocation.profile_sha256 if _is_digest(invocation.profile_sha256) else None,
        "binary_sha256": invocation.binary_sha256 if _is_digest(invocation.binary_sha256) else None,
        "status": "not_run",
        "reason": "execution_not_requested",
        "return_code": None,
        "live_qualification": "not_run",
        "cleanup_verification": "not_run",
        "external_effects": "not_started",
    }
    # Reserve the report before inspecting configuration or attempting any effect.
    with _open_report(invocation.report) as stream:
        _persist(stream, report)
        try:
            if invocation.phase not in PHASES or None in (report["profile_sha256"], report["binary_sha256"]):
                raise HarnessError("invalid_invocation")
            if invocation.binary.name != "edgeagent-market-data":
                raise HarnessError("unexpected_binary_name")
            if _hash_file(invocation.profile, 8192) != invocation.profile_sha256:
                raise HarnessError("profile_digest_mismatch")
            if _hash_file(invocation.binary, 256 * 1024 * 1024) != invocation.binary_sha256:
                raise HarnessError("binary_digest_mismatch")
            if invocation.execute_profile is None:
                return 2, "not_run"
            if invocation.execute_profile != invocation.profile_sha256:
                raise HarnessError("execution_opt_in_mismatch")
            # Read inherited configuration only after explicit exact-profile intent.
            selected = os.environ if environment is None else environment
            if invocation.phase == "acquire" and (
                "USE_MOCK" in selected
                or selected.get("EDGEAGENT_QUALIFICATION_LIVE_OPT_IN") != invocation.profile_sha256
            ):
                raise HarnessError("source_opt_in_denied")
            command, timeout = PHASES[invocation.phase]
            report.update(status="running", reason="command_started", external_effects="unknown",
                          live_qualification="not_verified")
            if invocation.phase == "cleanup":
                report["cleanup_verification"] = "not_verified"
            # Persist uncertainty before launch. If this fails, no child is started.
            _persist(stream, report)
            result = runner(
                [str(invocation.binary), command, str(invocation.profile), invocation.profile_sha256],
                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                shell=False, close_fds=True, timeout=timeout, env=dict(selected),
            )
            report["return_code"] = result.returncode if -255 <= result.returncode <= 255 else None
            if result.returncode == 0:
                report.update(status="command_succeeded", reason="independent_evidence_required",
                              external_effects="not_independently_verified")
                code = 0
            else:
                report.update(status="command_failed", reason="nonzero_command_exit")
                code = 1
        except HarnessError as error:
            report.update(status="blocked", reason=str(error))
            code = 2
        except (subprocess.TimeoutExpired, KeyboardInterrupt):
            report.update(status="unknown", reason="owner_reconciliation_required")
            code = 3
        except (OSError, ValueError):
            # Process launch failures cannot establish remote or prior-run state.
            running = report["status"] == "running"
            report.update(status="unknown" if running else "blocked", reason="io_failure")
            code = 3 if running else 2
        _persist(stream, report)
        return code, report["status"]


class _Parser(argparse.ArgumentParser):
    def error(self, message: str) -> None:
        raise HarnessError("invalid_arguments")


def main(argv: Sequence[str] | None = None) -> int:
    parser = _Parser(description=__doc__, allow_abbrev=False)
    parser.add_argument("--phase", required=True, choices=tuple(PHASES))
    parser.add_argument("--profile", required=True, type=Path)
    parser.add_argument("--profile-sha256", required=True)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--binary-sha256", required=True)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--execute-profile", help="Exact profile digest; operational intent, not owner approval")
    try:
        arguments = parser.parse_args(argv)
        code, status = supervise(Invocation(**vars(arguments)))
        print(f"qualification_harness_status={status}")
        return code
    except HarnessError:
        print("qualification_harness_status=report_or_input_failure", file=sys.stderr)
        return 2
    except OSError:
        # A final report failure may follow external work; never label it blocked.
        print("qualification_harness_status=report_io_failure", file=sys.stderr)
        return 3
    except KeyboardInterrupt:
        print("qualification_harness_status=unknown", file=sys.stderr)
        return 3


if __name__ == "__main__":
    raise SystemExit(main())
