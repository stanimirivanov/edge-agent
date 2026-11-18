"""Deterministic idle gating and finite local benchmark supervision."""
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import benchmark_explanation_cpu as bench


def observation(ticks=0, busy=0, swap=0):
    return {"cpu": {"cpu": [ticks, busy], "cpu0": [ticks, busy]},
            "swap": swap, "available": 10 * 1024**3, "ac": True,
            "pressure": {"cpu": 0, "memory": 0, "io": 0}}


class IdleTests(unittest.TestCase):
    def test_hot_core_swap_and_pressure_each_prevent_idle(self):
        a, b = observation(), observation(100)
        self.assertTrue(bench.is_idle(a, b, 1024))
        for change in ({"swap": 1}, {"ac": False}, {"available": 0},
                       {"pressure": {"cpu": 20_000, "memory": 0, "io": 0}},
                       {"cpu": {"cpu": [100, 0], "cpu0": [100, 20]}}):
            with self.subTest(change=change):
                self.assertFalse(bench.is_idle(a, dict(b, **change), 1024))

    def test_counter_reset_and_topology_changes_fail_closed(self):
        self.assertFalse(bench.is_idle(observation(100), observation(50), 0))
        self.assertFalse(bench.is_idle(dict(observation(), cpu={}),
                                       dict(observation(100), cpu={}), 0))
        changed = observation(200)
        del changed["cpu"]["cpu0"]
        self.assertFalse(bench.is_idle(observation(100), changed, 0))

    def test_busy_sample_restarts_entire_idle_window(self):
        now = [0]
        def sleep(seconds):
            now[0] += seconds
        def read():
            # Busy at t=2; a complete three-second idle window ends at t=6.
            return observation(now[0] * 100, 20 if now[0] >= 2 else 0)
        events = []
        self.assertTrue(bench.wait_idle(read, lambda: now[0], sleep,
                                       events.append, 10, 0, window=3))
        self.assertEqual(now[0], 6)

    def test_busy_host_times_out_without_starting_a_process(self):
        now = [0]
        def sleep(seconds):
            now[0] += seconds
        self.assertFalse(bench.wait_idle(lambda: observation(now[0]*100, now[0]*20),
                                        lambda: now[0], sleep, lambda _: None, 4, 0, window=3))
        self.assertEqual(now[0], 4)


class SupervisionTests(unittest.TestCase):
    def test_results_are_numeric_allowlist_not_runtime_text(self):
        raw = [{"n_threads": 6, "n_prompt": 512, "n_gen": 0,
                "avg_ts": 12.5, "stddev_ts": 0.5, "samples_ts": [12, 12.5, 13],
                "model_filename": "PRIVATE", "unexpected": "SECRET"}]
        raw += [dict(raw[0], n_prompt=p, n_gen=g) for p, g in ((2048, 0), (0, 128), (0, 512))]
        result = bench.results(json.dumps(raw).encode(), 6)
        self.assertEqual(result[0]["samples_ts"], [12, 12.5, 13])
        self.assertNotIn("PRIVATE", json.dumps(result))
        self.assertNotIn("SECRET", json.dumps(result))
        for bad in (b'[]', b'not json', b'[{"avg_ts":NaN}]'):
            with self.assertRaises(bench.HarnessError):
                bench.results(bad, 6)

    def test_real_child_output_is_filtered_and_timeout_is_reaped(self):
        events = []
        command = [sys.executable, "-c", "import time; time.sleep(30)"]
        bench.run_bench(command, {}, events.append, read=lambda: observation(), timeout=0.05)
        self.assertEqual(events[-1]["status"], "timeout")
        self.assertGreater(events[-1]["elapsed_ms"], 0)

    def test_interruption_reaps_owned_process(self):
        import subprocess
        actual = subprocess.Popen
        children = []
        def start(*args, **kwargs):
            child = actual(*args, **kwargs)
            children.append(child)
            return child
        with patch.object(bench.subprocess, "Popen", side_effect=start):
            with self.assertRaises(KeyboardInterrupt):
                bench.run_bench([sys.executable, "-c", "import time; time.sleep(30)"], {},
                                lambda _: None, read=lambda: (_ for _ in ()).throw(KeyboardInterrupt()))
        self.assertIsNotNone(children[0].poll())

    def test_private_report_is_exclusive_and_mode_0600(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.jsonl"
            with bench.open_report(path) as stream:
                bench.record(stream, {"status": "not_run"})
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            with self.assertRaises(FileExistsError):
                bench.open_report(path)
            self.assertEqual(json.loads(path.read_text())["status"], "not_run")

    def test_manifest_requires_exact_file_pins(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "local"
            target.write_bytes(b"fixture")
            pin = {"path": str(target), "sha256": hashlib.sha256(b"fixture").hexdigest()}
            manifest = {"kind": bench.KIND, "bench": pin, "weights": pin, "libraries": []}
            bench.preflight(manifest)
            target.write_bytes(b"changed")
            with self.assertRaises(bench.HarnessError):
                bench.preflight(manifest)


class InvocationTests(unittest.TestCase):
    def test_no_opt_in_and_busy_host_leave_durable_not_run_receipts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "bench"
            binary.write_bytes(b"invented local fixture")
            pin = {"path": str(binary), "sha256": hashlib.sha256(binary.read_bytes()).hexdigest()}
            manifest = root / "manifest.json"
            manifest.write_bytes(bench.canonical({"kind": bench.KIND, "bench": pin,
                                                 "weights": pin, "libraries": []}))
            digest = hashlib.sha256(manifest.read_bytes()).hexdigest()
            for opt_in in (None, digest):
                report = root / ("dry.jsonl" if opt_in is None else "busy.jsonl")
                with patch.object(bench, "wait_idle", return_value=False), \
                     patch.object(bench.os, "sched_getaffinity", return_value=set(range(20))), \
                     patch.object(bench, "run_bench") as run:
                    self.assertEqual(bench.run(manifest, digest, report, opt_in), 2)
                    run.assert_not_called()
                records = [json.loads(line) for line in report.read_text().splitlines()]
                self.assertEqual(records[-1]["status"], "not_run")

    def test_successful_real_process_is_measured_and_bad_census_is_rejected(self):
        rows = [{"n_threads": 6, "n_prompt": p, "n_gen": g,
                 "avg_ts": 10, "stddev_ts": 0, "samples_ts": [10]*3}
                for p, g in ((512, 0), (2048, 0), (0, 128), (0, 512))]
        code = "print(" + repr(json.dumps(rows)) + ")"
        events = []
        status, raw, contaminated = bench.run_bench(
            [sys.executable, "-c", code], {}, events.append, read=lambda: observation())
        self.assertEqual(status, "completed")
        self.assertEqual(len(bench.results(raw, 6)), 4)
        with self.assertRaises(bench.HarnessError):
            bench.results(json.dumps(rows[:3]).encode(), 6)
        self.assertEqual(events[-1]["status"], "completed")

    def test_missing_required_telemetry_is_recorded_without_starting_model(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "fixture"
            target.write_bytes(b"fixture")
            pin = {"path": str(target), "sha256": hashlib.sha256(b"fixture").hexdigest()}
            manifest = root / "manifest.json"
            manifest.write_bytes(bench.canonical({"kind": bench.KIND, "bench": pin,
                                                 "weights": pin, "libraries": []}))
            digest = hashlib.sha256(manifest.read_bytes()).hexdigest()
            report = root / "report.jsonl"
            with patch.object(bench, "sample", side_effect=OSError()), \
                 patch.object(bench.os, "sched_getaffinity", return_value=set(range(20))), \
                 patch.object(bench, "run_bench") as run:
                self.assertEqual(bench.run(manifest, digest, report, digest), 2)
                run.assert_not_called()
            self.assertEqual(json.loads(report.read_text().splitlines()[-1])["status"], "not_run")


if __name__ == "__main__":
    unittest.main()
