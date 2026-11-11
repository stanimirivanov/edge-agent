#!/usr/bin/env python3
"""Opt-in local model qualification with pinned artifacts and private reports."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import secrets
import socket
import subprocess
import sys
import time

from qualify_daily import HarnessError, _hash_file, _open_report

ROOT = Path(__file__).resolve().parents[1]
LIMIT = 2 * 1024 * 1024
FIELDS = {"version", "runtime", "weights", "license", "helper", "prompt", "profile",
          "libraries", "hardware", "threads", "context"}


def canonical(value):
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def hardware():
    """Pin the local CPU-only execution environment."""
    return {"machine": platform.machine(), "kernel": platform.release(),
            "cpu_count": os.cpu_count(),
            "cpuinfo_sha256": hashlib.sha256(Path("/proc/cpuinfo").read_bytes().split(b"cpu MHz")[0]).hexdigest()}


def runtime_id(manifest):
    """Paths and the ephemeral listener are excluded from deployment identity."""
    value = {key: manifest[key] for key in ("hardware", "threads", "context")}
    value.update(runtime=manifest["runtime"]["sha256"], weights=manifest["weights"]["sha256"],
                 libraries=[item["sha256"] for item in manifest["libraries"]],
                 launcher="llama-cpu-qualification-v1",
                 launcher_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest())
    return hashlib.sha256(canonical(value)).hexdigest()


def load(path, limit=LIMIT):
    if path.stat().st_size > limit:
        raise HarnessError("input_size_limit")
    def unique(pairs):
        value = {}
        for key, item in pairs:
            if key in value:
                raise HarnessError("duplicate_field")
            value[key] = item
        return value
    return json.loads(path.read_bytes(), object_pairs_hook=unique)


def preflight(manifest):
    """Check every local pin before executing any process."""
    if set(manifest) != FIELDS or type(manifest["version"]) is not int or manifest["version"] != 1:
        raise HarnessError("invalid_manifest")
    if (type(manifest["threads"]) is not int or not 1 <= manifest["threads"] <= 256
            or type(manifest["context"]) is not int or not 512 <= manifest["context"] <= 32768
            or manifest["hardware"] != hardware()
            or not isinstance(manifest["libraries"], list) or len(manifest["libraries"]) > 64):
        raise HarnessError("invalid_runtime")
    for key in ("runtime", "weights", "license", "helper", "prompt", "profile"):
        limit = {"weights": 64 * 1024**3, "runtime": 256 * 1024**2,
                 "helper": 512 * 1024**2, "prompt": 4096, "profile": 2048}.get(key, LIMIT)
        check_file(manifest[key], limit)
    for library in manifest["libraries"]:
        check_file(library, 512 * 1024**2)
    profile = load(Path(manifest["profile"]["path"]), 2048)
    if Path(manifest["profile"]["path"]).read_bytes() != canonical(profile):
        raise HarnessError("noncanonical_profile")
    data = profile["data"]
    if (profile["kind"] != "research.explanation.profile.v1"
            or data["deployment"] != {"runtime": runtime_id(manifest), "weights": manifest["weights"]["sha256"]}
            or data["prompt"] != manifest["prompt"]["sha256"]):
        raise HarnessError("profile_binding_mismatch")
    return profile


def check_file(pin, limit):
    try:
        if set(pin) != {"path", "sha256"} or _hash_file(Path(pin["path"]), limit) != pin["sha256"]:
            raise HarnessError("file_pin_mismatch")
    except OSError:
        raise HarnessError("file_pin_mismatch") from None


def check_census(exported, report):
    expected = [(case["id"], case["input"]) for case in exported["cases"]]
    actual = [(case["id"], case["input"]) for case in report["cases"]]
    if exported.get("model_suite") != report.get("model_suite") or exported["suite"] != report["suite"] or expected != actual or len(set(expected)) != len(expected):
        raise HarnessError("case_census_mismatch")


def helper(manifest, mode, value):
    # Files bound subprocess output; no unbounded PIPE capture or model diagnostics.
    import tempfile
    with tempfile.TemporaryFile() as output:
        result = subprocess.run([manifest["helper"]["path"], mode], input=canonical(value),
                                stdout=output, stderr=subprocess.DEVNULL, timeout=600, check=False,
                                env={"PATH": "/usr/bin:/bin", "LANG": "C"})
        output.seek(0)
        data = output.read(LIMIT + 1)
    if result.returncode or len(data) > LIMIT:
        raise HarnessError("qualification_helper_failed")
    return json.loads(data)


def command(manifest, port):
    return [manifest["runtime"]["path"], "--host", "127.0.0.1", "--port", str(port),
            "--model", manifest["weights"]["path"], "--alias", manifest["weights"]["sha256"],
            "--threads", str(manifest["threads"]), "--ctx-size", str(manifest["context"]),
            "--parallel", "1", "--device", "none", "--n-gpu-layers", "0", "--log-disable"]


def wait_ready(process, port):
    deadline = time.monotonic() + 120
    while process.poll() is None and time.monotonic() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2) as connection:
                connection.sendall(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
                line = bytearray()
                while b"\r\n" not in line and len(line) <= 4096 and time.monotonic() < deadline:
                    chunk = connection.recv(1024)
                    if not chunk:
                        break
                    line.extend(chunk)
                fields = line.split()
                if len(fields) >= 2 and fields[:2] in ([b"HTTP/1.1", b"200"], [b"HTTP/1.0", b"200"]):
                    return
        except OSError:
            pass
        time.sleep(0.1)
    raise HarnessError("runtime_startup_failed")


def collect(manifest):
    profile = preflight(manifest)
    exported = helper(manifest, "export", {})
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    argv = command(manifest, port)
    environment = {"PATH": "/usr/bin:/bin", "LANG": "C"}
    if manifest["libraries"]:
        environment["LD_LIBRARY_PATH"] = ":".join(sorted({str(Path(pin["path"]).parent) for pin in manifest["libraries"]}))
    with subprocess.Popen(argv, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                          env=environment) as process:
        try:
            wait_ready(process, port)
            observation = {
                "pid": process.pid, "port": port, "command": argv,
                "start": Path(f"/proc/{process.pid}/stat").read_text().rsplit(")", 1)[1].split()[19],
                "executable": manifest["runtime"]["sha256"],
            }
            value = {"profile": canonical(profile).decode(),
                     "prompt": Path(manifest["prompt"]["path"]).read_text(),
                     "observation": observation,
                     "orders": [bool(secrets.randbits(1)) for _ in exported["cases"]]}
            report = helper(manifest, "collect", value)
            if process.poll() is not None:
                raise HarnessError("runtime_exited")
            preflight(manifest)
            check_census(exported, report)
            report.update(qualification="incomplete", generation_enabled=False,
                          reviews="pending", hardware=manifest["hardware"])
            return report
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--execute-manifest", help="SHA-256 of the exact reviewed manifest bytes")
    parser.add_argument("--collection", type=Path, help="Private collection report to assess without inference")
    parser.add_argument("--reviews", type=Path, help="Two independently supplied blinded reviewer records")
    options = parser.parse_args(argv)
    try:
        manifest = load(options.manifest)
        preflight(manifest)
        pin = hashlib.sha256(options.manifest.read_bytes()).hexdigest()
        report = {"manifest_sha256": pin, "qualification": "not_run", "generation_enabled": False}
        if bool(options.collection) != bool(options.reviews) or (options.collection and options.execute_manifest):
            raise HarnessError("invalid_phase")
        if options.execute_manifest is not None and options.execute_manifest != pin:
            raise HarnessError("execution_pin_mismatch")
        with _open_report(options.report) as output:
            report["kind"] = "edgeagent.explanation-qualification.v1"
            output.write(canonical(report).decode())
            output.flush()
            try:
                if options.collection:
                    collection = load(options.collection)
                    if collection.get("kind") != "edgeagent.explanation-qualification.v1" or collection["manifest_sha256"] != pin:
                        raise HarnessError("collection_manifest_mismatch")
                    check_census(helper(manifest, "export", {}), collection)
                    report = helper(manifest, "assess", {"collection": collection,
                        "profile": manifest["profile"]["sha256"], "reviews": load(options.reviews)})
                    report["collection_sha256"] = hashlib.sha256(options.collection.read_bytes()).hexdigest()
                    report["reviews_sha256"] = hashlib.sha256(options.reviews.read_bytes()).hexdigest()
                elif options.execute_manifest:
                    # Keep candidate position private. Reviewers receive only this second file.
                    with _open_report(options.report.with_suffix(".pairs.json")) as blinded:
                        report = collect(manifest)
                        pairs = [{**case["review"], "presentation": case["presentation"]} for case in report["cases"] if case["usefulness"]]
                        blinded.write(canonical(pairs).decode())
                report["manifest_sha256"] = pin
            except (HarnessError, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
                report = {"manifest_sha256": pin, "qualification": "failed", "generation_enabled": False,
                          "failure": "qualification_execution_failed"}
            report["kind"] = "edgeagent.explanation-qualification.v1"
            output.seek(0)
            output.truncate()
            output.write(canonical(report).decode())
            output.flush()
            os.fsync(output.fileno())
        return 0 if report["qualification"] == "passed" else 2
    except (HarnessError, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        print("explanation qualification refused", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
