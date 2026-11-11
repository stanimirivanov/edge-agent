"""Qualification pins and case census fail closed before model dispatch."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import sys
import subprocess
import http.server
import threading
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
SPEC = importlib.util.spec_from_file_location("qualify_explanation_model", ROOT / "scripts/qualify_explanation_model.py")
harness = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(harness)


class QualificationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        def pin(name, content):
            path = self.root / name
            path.write_bytes(content)
            return {"path": str(path), "sha256": hashlib.sha256(content).hexdigest()}
        self.manifest = {
            "version": 1,
            "runtime": pin("llama-server", b"invented runtime"),
            "weights": pin("invented.gguf", b"invented weights"),
            "license": pin("LICENSE", b"invented license, not model approval"),
            "helper": pin("helper", b"invented helper"),
            "prompt": pin("prompt", b"Select only supported presentation variants."),
            "profile": pin("profile.json", (json.dumps({"kind":"research.explanation.profile.v1", "data":{
                "deployment":{"runtime":"a" * 64,"weights":"b" * 64},
                "settings":{"seed":0,"temperature_milli":0,"max_output_tokens":256,"timeout_ms":1000},
                "prompt":"c" * 64,"validator":"d" * 64,"renderer":"e" * 64,"tools":"none"
            }},sort_keys=True,separators=(",", ":")) + "\n").encode()),
            "libraries": [],
            "hardware": harness.hardware(),
            "threads": 1,
            "context": 4096,
        }
        self.profile = json.loads(Path(self.manifest["profile"]["path"]).read_bytes())
        self.profile["data"]["deployment"] = {
            "runtime": harness.runtime_id(self.manifest),
            "weights": self.manifest["weights"]["sha256"],
        }
        self.profile["data"]["prompt"] = self.manifest["prompt"]["sha256"]
        encoded = (json.dumps(self.profile, sort_keys=True, separators=(",", ":")) + "\n").encode()
        Path(self.manifest["profile"]["path"]).write_bytes(encoded)
        self.manifest["profile"]["sha256"] = hashlib.sha256(encoded).hexdigest()

    def test_preflight_checks_exact_local_files_and_hardware(self):
        self.assertEqual(harness.preflight(self.manifest)["data"], self.profile["data"])
        for field in ("runtime", "weights", "license", "helper", "prompt", "profile"):
            changed = copy.deepcopy(self.manifest)
            changed[field]["sha256"] = "f" * 64
            with self.subTest(field=field), self.assertRaises(harness.HarnessError):
                harness.preflight(changed)
        changed = copy.deepcopy(self.manifest)
        changed["hardware"] = {}
        with self.assertRaises(harness.HarnessError):
            harness.preflight(changed)
        Path(self.manifest["weights"]["path"]).unlink()
        with self.assertRaises(harness.HarnessError):
            harness.preflight(self.manifest)

    def test_missing_fields_symlinks_and_unbounded_launch_settings_are_refused(self):
        changed = copy.deepcopy(self.manifest)
        del changed["license"]
        with self.assertRaises(harness.HarnessError):
            harness.preflight(changed)
        for field, value in (("threads", 0), ("context", 1_000_000), ("threads", True)):
            changed = copy.deepcopy(self.manifest)
            changed[field] = value
            with self.assertRaises(harness.HarnessError):
                harness.preflight(changed)
        target = Path(self.manifest["runtime"]["path"])
        link = self.root / "link"
        link.symlink_to(target)
        changed = copy.deepcopy(self.manifest)
        changed["runtime"]["path"] = str(link)
        with self.assertRaises(harness.HarnessError):
            harness.preflight(changed)

    def test_default_preflight_records_not_run_and_never_dispatches(self):
        manifest = self.root / "manifest.json"
        manifest.write_bytes(harness.canonical(self.manifest))
        report = self.root / "report.json"
        with patch.object(harness, "collect", side_effect=AssertionError("unexpected model execution")):
            self.assertEqual(harness.main(["--manifest", str(manifest), "--report", str(report)]), 2)
        result = json.loads(report.read_bytes())
        self.assertEqual(result["qualification"], "not_run")
        self.assertFalse(result["generation_enabled"])
        original = report.read_bytes()
        with patch.object(harness, "collect", side_effect=AssertionError("unexpected model execution")):
            self.assertEqual(harness.main(["--manifest", str(manifest), "--report", str(report),
                                          "--execute-manifest", hashlib.sha256(manifest.read_bytes()).hexdigest()]), 2)
        self.assertEqual(report.read_bytes(), original)
        self.assertEqual(report.stat().st_mode & 0o777, 0o600)

    def test_startup_waits_for_health_after_the_listener_opens(self):
        observations = []
        class Health(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                observations.append(self.path)
                self.send_response(503 if len(observations) == 1 else 200)
                self.end_headers()
            def log_message(self, *_):
                pass
        server = http.server.HTTPServer(("127.0.0.1", 0), Health)
        server.timeout = 0.5
        thread = threading.Thread(target=lambda: [server.handle_request() for _ in range(2)], daemon=True)
        thread.start()
        try:
            class Process:
                def poll(self):
                    return None
            harness.wait_ready(Process(), server.server_port)
            thread.join(timeout=2)
            self.assertEqual(observations, ["/health", "/health"])
        finally:
            server.server_close()

    def test_owned_runtime_is_reaped_after_helper_failure(self):
        processes = []
        popen = subprocess.Popen
        def own(*args, **kwargs):
            process = popen(*args, **kwargs)
            processes.append(process)
            return process
        exported = {"suite": "a" * 64, "cases": []}
        with patch.object(harness, "command", return_value=["/usr/bin/sleep", "60"]), \
                patch.object(harness, "wait_ready"), \
                patch.object(harness, "helper", side_effect=[exported, harness.HarnessError("injected_failure")]), \
                patch.object(harness.subprocess, "Popen", side_effect=own):
            with self.assertRaises(harness.HarnessError):
                harness.collect(self.manifest)
        self.assertEqual(len(processes), 1)
        self.assertIsNotNone(processes[0].poll())

    def test_report_requires_the_exact_exported_case_census(self):
        exported = {"suite": "a" * 64, "cases": [{"id": "U01", "input": "b" * 64}]}
        result = {"suite": "a" * 64, "cases": [{"id": "U01", "input": "b" * 64, "outcome": "failed"}]}
        harness.check_census(exported, result)
        for cases in ([], result["cases"] * 2, [{"id": "U01", "input": "c" * 64}]):
            with self.assertRaises(harness.HarnessError):
                harness.check_census(exported, {"suite": exported["suite"], "cases": cases})


if __name__ == "__main__":
    unittest.main()
