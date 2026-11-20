#!/usr/bin/env python3
"""Opt-in Linux throughput measurement; never qualification or timeout policy."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time

from qualify_explanation_model import HarnessError, canonical, check_file, load
from qualify_daily import _hash_file, _open_report

KIND = "research.explanation-cpu-benchmark.v1"
THREADS = (6, 10, 14, 20)
LIMIT = 1024 * 1024


def open_report(path):
    # Reports must stay outside every Git checkout, including sibling repositories.
    if any((parent / ".git").is_file() or (parent / ".git/HEAD").is_file()
           for parent in path.resolve().parents):
        raise HarnessError("private_report_required")
    return _open_report(path)


def record(stream, value):
    stream.write(canonical(value).decode())
    stream.flush()
    os.fsync(stream.fileno())


def preflight(manifest):
    if (set(manifest) != {"kind", "bench", "weights", "libraries"}
            or manifest["kind"] != KIND or not isinstance(manifest["libraries"], list)
            or len(manifest["libraries"]) > 64):
        raise HarnessError("invalid_benchmark_manifest")
    for name, limit in (("bench", 256 * 1024**2), ("weights", 64 * 1024**3)):
        check_file(manifest[name], limit)
    for pin in manifest["libraries"]:
        check_file(pin, 512 * 1024**2)


def ac_power():
    supplies = Path("/sys/class/power_supply")
    batteries = [p for p in supplies.glob("*") if (p / "type").read_text().strip() == "Battery"]
    if not batteries:
        return True  # Desktop/no exposed battery: AC is not applicable.
    return any(p.read_text().strip() == "1" for p in supplies.glob("*/online"))


def sample():
    cpu = {}
    for line in Path("/proc/stat").read_text().splitlines():
        name, *values = line.split()
        if name == "cpu" or (name.startswith("cpu") and name[3:].isdigit()):
            ticks = list(map(int, values[:8]))  # guest is already included in user/nice.
            total = sum(ticks)
            cpu[name] = [total, total - ticks[3]]  # iowait is not idle capacity.
    vm = dict(line.split() for line in Path("/proc/vmstat").read_text().splitlines())
    mem = {line.split(":")[0]: int(line.split()[1]) * 1024
           for line in Path("/proc/meminfo").read_text().splitlines()}
    pressure = {}
    for name in ("cpu", "memory", "io"):
        row = Path(f"/proc/pressure/{name}").read_text().splitlines()[0].split()
        pressure[name] = int(dict(field.split("=") for field in row[1:])["total"])
    return {"cpu": cpu, "swap": int(vm["pswpin"]) + int(vm["pswpout"]),
            "available": mem["MemAvailable"], "ac": ac_power(), "pressure": pressure}


def is_idle(before, after, memory):
    if ("cpu" not in after["cpu"] or len(after["cpu"]) < 2
            or not after["ac"] or after["available"] < memory or after["swap"] != before["swap"]
            or before["cpu"].keys() != after["cpu"].keys()):
        return False
    for name, (total, busy) in after["cpu"].items():
        dt, db = total - before["cpu"][name][0], busy - before["cpu"][name][1]
        if dt <= 0 or not 0 <= db <= dt or db / dt > (0.05 if name == "cpu" else 0.10):
            return False
    return all(0 <= after["pressure"][key] - before["pressure"][key] <= 10_000
               for key in ("cpu", "memory", "io"))


def wait_idle(read, clock, sleep, emit, deadline, memory, window=300, *, additional_idle=None):
    previous, since = read(), None
    while clock() < deadline:
        sleep(min(1, deadline - clock()))
        current, now = read(), clock()
        idle = is_idle(previous, current, memory)
        if additional_idle is not None:
            idle = additional_idle() and idle
        since = (now if since is None else since) if idle else None
        emit({"event": "idle_sample", "idle": idle, "host": current})
        if since is not None and now - since >= window:
            return True
        previous = current
    return False


def hardware():
    def numbers(pattern):
        result = []
        for path in sorted(Path("/sys").glob(pattern)):
            try:
                result.append(int(path.read_text().strip()))
            except (OSError, ValueError):
                result.append(None)
        return result
    return {"affinity": sorted(os.sched_getaffinity(0)), "kernel": os.uname().release,
            "cpuinfo_sha256": hashlib.sha256(Path("/proc/cpuinfo").read_bytes()).hexdigest(),
            "core_ids": numbers("devices/system/cpu/cpu[0-9]*/topology/core_id"),
            "max_khz": numbers("devices/system/cpu/cpu[0-9]*/cpufreq/cpuinfo_max_freq"),
            "current_khz": numbers("devices/system/cpu/cpu[0-9]*/cpufreq/scaling_cur_freq"),
            "temperatures_millic": numbers("class/thermal/thermal_zone*/temp"),
            "governors": sorted({p.read_text().strip() for p in
                Path("/sys/devices/system/cpu").glob("cpu*/cpufreq/scaling_governor")})}


def results(raw, threads):
    try:
        rows = json.loads(raw)
        if not isinstance(rows, list) or not 1 <= len(rows) <= 16:
            raise ValueError()
        clean = []
        for row in rows:
            keys = ("n_threads", "n_prompt", "n_gen", "avg_ts", "stddev_ts", "samples_ts")
            item = {key: row[key] for key in keys}
            if item["n_threads"] != threads or not isinstance(item["samples_ts"], list):
                raise ValueError()
            values = [item[key] for key in keys[:-1]] + item["samples_ts"]
            if (not 1 <= len(item["samples_ts"]) <= 10 or any(
                    type(v) not in (int, float) or not math.isfinite(v) or v < 0 for v in values)):
                raise ValueError()
            clean.append(item)
        if ({(r["n_prompt"], r["n_gen"]) for r in clean}
                != {(512, 0), (2048, 0), (0, 128), (0, 512)} or len(clean) != 4
                or any(len(r["samples_ts"]) != 3 for r in clean)):
            raise ValueError()
        return clean
    except (ValueError, TypeError, KeyError):
        raise HarnessError("invalid_benchmark_result") from None


def process_ticks(pid):
    # comm can contain spaces or parentheses; subsequent fields start after the last ')'.
    fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    return int(fields[11]) + int(fields[12])


def run_bench(command, environment, emit, *, read=sample, timeout=900):
    start = time.monotonic()
    with tempfile.TemporaryFile() as output:
        process = subprocess.Popen(command, env=environment, stdout=output,
                                   stderr=subprocess.DEVNULL, start_new_session=True)
        status, contaminated, observations = "failed", False, 0
        try:
            previous, own = read(), process_ticks(process.pid)
            while True:
                time.sleep(min(1, max(0, timeout - (time.monotonic() - start))))
                current = read()
                try:
                    ticks = process_ticks(process.pid)
                    dt = current["cpu"]["cpu"][0] - previous["cpu"]["cpu"][0]
                    busy = current["cpu"]["cpu"][1] - previous["cpu"]["cpu"][1]
                    external = max(0, busy - (ticks - own)) / dt if dt > 0 else None
                    contaminated |= (external is None or external > 0.05 or not current["ac"]
                                     or current["swap"] != previous["swap"]
                                     or current["available"] < 2 * 1024**3)
                    own = ticks
                except (OSError, ValueError, IndexError):
                    external, contaminated = None, True
                emit({"event": "load", "external_busy_fraction": external,
                      "host": current, "hardware": hardware()})
                observations += 1
                previous = current
                if output.tell() > LIMIT:
                    status = "output_limit"
                    break
                if process.poll() is not None:
                    status = "completed" if process.returncode == 0 else "failed"
                    break
                if time.monotonic() - start >= timeout:
                    status = "timeout"
                    break
        finally:
            # Only our session is signalled; descendants cannot outlive a timed-out block.
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
        output.seek(0)
        raw = output.read(LIMIT + 1) if status == "completed" else b""
        emit({"event": "finished", "status": status, "contaminated": contaminated,
              "observations": observations, "elapsed_ms": round((time.monotonic()-start)*1000)})
        return status, raw, contaminated


def run(manifest_path, digest, report_path, execute):
    if len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
        raise HarnessError("invalid_manifest_digest")
    with open_report(report_path) as stream:
        started = time.monotonic()
        def emit(value):
            record(stream, dict(value, journal_ms=round((time.monotonic()-started)*1000)))
        emit({"kind": KIND, "event": "start", "manifest_sha256": digest,
              "script_sha256": _hash_file(Path(__file__).resolve(), LIMIT),
              "qualification": "not_run", "threads": THREADS})
        launched = False
        try:
            if execute != digest:
                raise HarnessError("execution_opt_in_required")
            if _hash_file(manifest_path, LIMIT) != digest:
                raise HarnessError("manifest_pin_mismatch")
            manifest = load(manifest_path, LIMIT)
            preflight(manifest)
            if max(THREADS) > len(os.sched_getaffinity(0)):
                raise HarnessError("thread_census_unavailable")
            emit({"event": "hardware", "hardware": hardware()})
            environment = {"PATH": "/usr/bin:/bin", "LANG": "C"}
            if manifest["libraries"]:
                environment["LD_LIBRARY_PATH"] = ":".join(sorted({
                    str(Path(p["path"]).parent) for p in manifest["libraries"]}))
            memory = Path(manifest["weights"]["path"]).stat().st_size + 2 * 1024**3
            overall = time.monotonic() + 7200
            clean = True
            for threads in THREADS:
                preflight(manifest)
                if not wait_idle(sample, time.monotonic, time.sleep, emit,
                                 min(overall, time.monotonic()+3600), memory):
                    emit({"event": "complete", "status": "incomplete" if launched else "not_run", "reason": "idle_wait_expired"})
                    return 2
                if time.monotonic() >= overall:
                    emit({"event": "complete", "status": "incomplete", "reason": "overall_budget_expired"})
                    return 2
                emit({"event": "block", "threads": threads})
                command = [manifest["bench"]["path"], "-m", manifest["weights"]["path"],
                           "-t", str(threads), "-ngl", "0", "-dev", "none", "-p", "512,2048",
                           "-n", "128,512", "-r", "3", "-o", "json"]
                launched = True
                status, raw, contaminated = run_bench(command, environment, emit,
                    timeout=max(0, min(900, overall-time.monotonic())))
                clean &= status == "completed" and not contaminated
                if status == "completed":
                    emit({"event": "throughput", "threads": threads, "rows": results(raw, threads)})
            emit({"event": "complete", "status": "completed" if clean else "inconclusive"})
            return 0 if clean else 2
        except (HarnessError, OSError, ValueError, KeyError, TypeError):
            emit({"event": "complete", "status": "incomplete" if launched else "not_run", "reason": "preflight_or_measurement_failed"})
            return 2
        except KeyboardInterrupt:
            emit({"event": "complete", "status": "interrupted"})
            return 130


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--manifest-sha256", required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--execute-manifest", help="Explicitly repeat the exact manifest digest")
    args = parser.parse_args()
    def interrupted(signum, frame):
        raise KeyboardInterrupt()
    signal.signal(signal.SIGTERM, interrupted)
    try:
        return run(args.manifest, args.manifest_sha256, args.report, args.execute_manifest)
    except (HarnessError, OSError):
        print("benchmark: private report unavailable")
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
