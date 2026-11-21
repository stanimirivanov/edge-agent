#!/usr/bin/env python3
"""Finite owned model supervisor. No downloads, activation or approval installation."""
import argparse
import qualify_daily
import hashlib
import os
from pathlib import Path
import selectors
import signal
import socket
import subprocess
import sys
import time
import qualify_explanation_model as common

LIMIT = 2 * 1024 * 1024
REQUEST = b'{"kind":"research.explanation-runtime.required.v2"}\n'


def runtime_id(manifest):
    value = {key: manifest[key] for key in ("hardware", "threads", "context")}
    value.update(runtime=manifest["runtime"]["sha256"], weights=manifest["weights"]["sha256"],
                 helper=manifest["helper"]["sha256"],
                 libraries=[item["sha256"] for item in manifest["libraries"]],
                 launcher="owned-explanation-cpu-v2",
                 launcher_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                 support_sha256=hashlib.sha256(Path(common.__file__).read_bytes()).hexdigest(),
                 hashing_sha256=hashlib.sha256(Path(qualify_daily.__file__).read_bytes()).hexdigest())
    if manifest.get("version") == 3:
        import explanation_cuda as cuda
        value.update(launcher="owned-explanation-cuda-v3",cuda=manifest["cuda"],
                     gpu_support_sha256=hashlib.sha256(Path(cuda.__file__).read_bytes()).hexdigest())
    return hashlib.sha256(common.canonical(value)).hexdigest()


def preflight(manifest, *, performance=False):
    if not isinstance(manifest,dict):raise common.HarnessError("invalid_manifest")
    version=manifest.get("version")
    if type(version) is not int or version not in (2,3):raise common.HarnessError("invalid_manifest")
    profile = common.preflight(manifest, version=version, identity=runtime_id)
    if version == 3:
        import explanation_cuda as cuda
        cuda.observe(manifest["cuda"])
    expected = "probe_explanation" if performance else "edgeagent-research"
    if Path(manifest["helper"]["path"]).name != expected:
        raise common.HarnessError("service_helper_required")
    return profile


def stop(process):
    if process is None:
        return
    try:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
        process.wait(timeout=5)
    except BaseException:
        # A whole-run budget/cancellation can interrupt wait itself. Reap this
        # owned child before propagating; no later request may outrun cleanup.
        if process.poll() is None:
            process.kill()
        process.wait(timeout=5)
        raise


def owned_command(argv):
    return [sys.executable, str(Path(__file__).resolve()), "_owned-exec", str(os.getpid()), *argv]


def exec_owned(arguments):
    # Run after Python startup, avoiding preexec_fn in a forked interpreter.
    # Linux preserves PDEATHSIG across ordinary exec; reject executables whose
    # privilege transitions would clear it. See PR_SET_PDEATHSIG(2const).
    import ctypes
    import stat
    parent, executable, *rest = arguments
    target = Path(executable)
    if (not target.is_absolute() or target.stat().st_mode & (stat.S_ISUID | stat.S_ISGID)
            or "security.capability" in os.listxattr(target)):
        raise OSError("privileged_child_refused")
    libc = ctypes.CDLL(None, use_errno=True)
    libc.prctl.argtypes = [ctypes.c_int, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong]
    libc.prctl.restype = ctypes.c_int
    if libc.prctl(1, signal.SIGKILL, 0, 0, 0) != 0 or os.getppid() != int(parent):
        raise OSError("owner_lost")
    os.execv(executable, [executable, *rest])


def diagnostic_source(source, source_pin, source_manifest, manifest):
    """A new helper/launcher identity may diagnose, but may not retune inference."""
    source, source_manifest = source.resolve(), source_manifest.resolve()
    common.check_file({"path": str(source), "sha256": source_pin}, LIMIT)
    bundle = common.load(source)
    if (set(bundle) != {"kind", "collection", "pairs", "permission_granted"}
            or bundle["kind"] != "research.explanation-runtime.collection.v2"
            or bundle["permission_granted"] is not False):
        raise common.HarnessError("invalid_diagnostic_source")
    original = bundle["collection"]
    common.check_file({"path": str(source_manifest), "sha256": original["manifest"]}, 32768)
    previous = common.load(source_manifest, 32768)
    if (set(previous) != common.FIELDS or previous["version"] != 2
            or original["helper"] != previous["helper"]["sha256"]
            or len(original["cases"]) != 30 or len(original["safety"]) != 4 or len(bundle["pairs"]) != 30
            or any(manifest[k] != previous[k] for k in common.FIELDS - {"helper", "profile"})):
        raise common.HarnessError("diagnostic_configuration_changed")
    profiles = []
    for pin in (previous["profile"], manifest["profile"]):
        common.check_file(pin, 2048)
        profiles.append(common.load(Path(pin["path"]), 2048))
    old, current = profiles
    if (old["kind"] != "research.explanation.profile.v1" or current["kind"] != old["kind"]
            or old["data"] != original["profile"]
            or {k:v for k,v in old["data"].items() if k != "deployment"}
               != {k:v for k,v in current["data"].items() if k != "deployment"}
            or old["data"]["deployment"]["weights"] != current["data"]["deployment"]["weights"]):
        raise common.HarnessError("diagnostic_profile_changed")
    return source_pin


EXTENDED_P03 = "performance-P03-extended"
DEFAULT_OBSERVATION_SECONDS = 180
EXTENDED_P03_SECONDS = 600


def helper_budget(mode, timeout_ms):
    if mode == EXTENDED_P03:
        return EXTENDED_P03_SECONDS + 10
    if mode.startswith("performance-"):
        return DEFAULT_OBSERVATION_SECONDS + 10
    extended_seconds = max(0, (timeout_ms + 999) // 1000 - 30)
    if mode.endswith("collect"):
        return 1200 + 34 * extended_seconds
    return 90 + extended_seconds


def supervise(manifest_path, manifest_pin, mode, diagnostic=None, *, performance_observer=None):
    raw = manifest_path.read_bytes()
    if len(raw) > 32768 or hashlib.sha256(raw).hexdigest() != manifest_pin:
        raise common.HarnessError("manifest_pin_mismatch")
    manifest = common.load(manifest_path, 32768)
    if not isinstance(manifest,dict):raise common.HarnessError("invalid_manifest")
    version=manifest.get("version")
    if type(version) is not int or version not in (2,3) or set(manifest) != common.FIELDS | ({"cuda"} if version==3 else set()):
        raise common.HarnessError("invalid_manifest")
    if version == 3:
        import explanation_cuda as cuda
    if diagnostic is not None and version != 2:raise common.HarnessError("cpu_diagnostic_required")
    if mode == EXTENDED_P03 and version != 3:
        raise common.HarnessError("cuda_manifest_required")
    performance = mode in {"performance-P01", "performance-P02", "performance-P03", EXTENDED_P03}
    if mode.startswith("performance-") and (not performance or diagnostic is not None):
        raise common.HarnessError("invalid_performance_mode")
    if not performance and performance_observer is not None:
        raise common.HarnessError("performance_observer_requires_probe")
    if performance:
        preflight(manifest, performance=True)
    # The helper alone is needed for recovery. Runtime/weights are checked only
    # after the helper has recovered the durable job and requested a new runtime.
    common.check_file(manifest["helper"], 512 * 1024**2)
    child = model = None
    environment = dict(os.environ, EDGEAGENT_EXPLANATION_MANIFEST=str(manifest_path),
                       EDGEAGENT_EXPLANATION_MANIFEST_SHA256=manifest_pin)
    for key in ("EDGEAGENT_EXPLANATION_DIAGNOSTIC_REPORT", "EDGEAGENT_EXPLANATION_DIAGNOSTIC_SOURCE"):
        environment.pop(key, None)
    if diagnostic is not None:
        if mode != "offline-explanation-runtime-collect":
            raise common.HarnessError("diagnostic_mode_required")
        source_pin, report = diagnostic
        environment.update(EDGEAGENT_EXPLANATION_DIAGNOSTIC_SOURCE=source_pin,
                           EDGEAGENT_EXPLANATION_DIAGNOSTIC_REPORT=str(report))
    timeout_ms = 30_000
    try:
        child = subprocess.Popen(owned_command([manifest["helper"]["path"], mode]), stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=environment)
        # Bounded control line, timeout includes database/owner preparation.
        first = bytearray()
        deadline = time.monotonic() + 90
        with selectors.DefaultSelector() as ready:
            ready.register(child.stdout, selectors.EVENT_READ)
            while not first.endswith(b"\n"):
                if len(first) > LIMIT or time.monotonic() >= deadline:
                    raise common.HarnessError("helper_control_timeout")
                if ready.select(timeout=0.2):
                    part = os.read(child.stdout.fileno(), 1)
                    if not part:
                        break
                    first.extend(part)
        if bytes(first) == REQUEST:
            profile = preflight(manifest, performance=performance)
            timeout_ms = profile["data"]["settings"]["timeout_ms"]
            with socket.socket() as reservation:
                reservation.bind(("127.0.0.1", 0))
                port = reservation.getsockname()[1]
            argv = common.command(manifest, port)
            model_environment = {"PATH": "/usr/bin:/bin", "LANG": "C"}
            if manifest["libraries"]:
                model_environment["LD_LIBRARY_PATH"] = ":".join(sorted({str(Path(p["path"]).parent) for p in manifest["libraries"]}))
            if version == 3:model_environment.update(cuda.environment(manifest["cuda"]))
            model = subprocess.Popen(owned_command(argv), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                     env=model_environment)
            common.wait_ready(model, port)
            if version == 3:cuda.check_process(manifest["cuda"],model.pid)
            observation = {"pid": model.pid, "port": port, "command": argv,
                           "start": Path(f"/proc/{model.pid}/stat").read_text().rsplit(")", 1)[1].split()[19],
                           "executable": manifest["runtime"]["sha256"]}
            child.stdin.write(common.canonical(observation))
            child.stdin.close()
            first.clear()
        else:
            child.stdin.close()
            if performance:
                raise common.HarnessError("performance_runtime_required")
        deadline = time.monotonic() + helper_budget(mode, timeout_ms)
        tail = bytearray()
        with selectors.DefaultSelector() as ready:
            ready.register(child.stdout, selectors.EVENT_READ)
            while True:
                if performance_observer is not None:
                    performance_observer(model.pid, child.pid, port)
                if time.monotonic() >= deadline:
                    raise common.HarnessError("helper_timeout")
                if not ready.select(timeout=0.2):
                    continue
                part = os.read(child.stdout.fileno(), 65536)
                if not part:
                    break
                tail.extend(part)
                if len(first) + len(tail) > LIMIT:
                    raise common.HarnessError("helper_output_limit")
        child.wait(timeout=5)
        if child.returncode:
            raise common.HarnessError("helper_failed")
        preflight(manifest, performance=performance) if model is not None else None
        if model is not None and version == 3:cuda.check_process(manifest["cuda"],model.pid)
        return bytes(first) + bytes(tail)
    finally:
        try:
            stop(child)
        finally:
            stop(model)
            if child is not None:
                if child.stdin is not None:
                    child.stdin.close()
                child.stdout.close()

def main():
    if sys.argv[1:2] == ["_owned-exec"]:
        try:
            exec_owned(sys.argv[2:])
        except (OSError, ValueError):
            return 125
    def cancelled(_number, _frame):
        raise KeyboardInterrupt
    signal.signal(signal.SIGTERM, cancelled)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--execute-manifest", required=True)
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--collect", action="store_true")
    selection.add_argument("--diagnose", action="store_true")
    parser.add_argument("--source-collection", type=Path)
    parser.add_argument("--source-collection-sha256")
    parser.add_argument("--source-manifest", type=Path)
    parser.add_argument("--diagnostic-report", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    options = parser.parse_args()
    try:
        diagnostic = None
        inputs = [options.source_collection, options.source_collection_sha256, options.source_manifest, options.diagnostic_report]
        if options.diagnose:
            if not all(inputs) or options.diagnostic_report.exists():
                raise common.HarnessError("invalid_diagnostic_configuration")
            manifest = common.load(options.manifest, 32768)
            source_pin = diagnostic_source(*inputs[:3], manifest)
            diagnostic = source_pin, options.diagnostic_report.resolve()
        elif any(inputs):
            raise common.HarnessError("diagnostic_mode_required")
        # Never overwrite a private report or expose proposal/reader material on stdout.
        with os.fdopen(os.open(options.output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "wb") as target:
            target.write(supervise(options.manifest, options.execute_manifest,
                                   "offline-explanation-runtime-collect" if options.collect or options.diagnose else "offline-explanation-worker-owned",
                                   diagnostic=diagnostic))
        return 0
    except (OSError, ValueError, KeyError, subprocess.SubprocessError, common.HarnessError, KeyboardInterrupt):
        print("owned explanation execution failed closed", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
