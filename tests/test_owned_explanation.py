"""The owned supervisor pins its helper and both launcher sources."""
from pathlib import Path
import sys
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import run_explanation_owned as owned

class OwnedLauncherTests(unittest.TestCase):
    def test_identity_changes_with_helper_and_resources(self):
        manifest = {"hardware": {}, "threads": 1, "context": 4096,
                    "runtime": {"sha256": "a"*64}, "weights": {"sha256": "b"*64},
                    "helper": {"sha256": "c"*64}, "libraries": []}
        first = owned.runtime_id(manifest)
        manifest["helper"]["sha256"] = "d"*64
        self.assertNotEqual(first, owned.runtime_id(manifest))
        manifest["threads"] = 2
        self.assertNotEqual(first, owned.runtime_id(manifest))

    def test_receipt_recovery_needs_no_runtime_or_weights(self):
        import hashlib
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            helper = root / "edgeagent-research"
            helper.write_text("#!/bin/sh\nprintf 'recovered\\n'\n")
            helper.chmod(0o700)
            manifest = dict.fromkeys(owned.common.FIELDS)
            manifest.update(version=2, helper={"path": str(helper), "sha256": hashlib.sha256(helper.read_bytes()).hexdigest()})
            path = root / "manifest.json"
            path.write_bytes(owned.common.canonical(manifest))
            result = owned.supervise(path, hashlib.sha256(path.read_bytes()).hexdigest(), "offline-explanation-worker-owned")
            self.assertEqual(result, b"recovered\n")

    def test_failed_helper_is_reaped(self):
        import hashlib
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            helper = root / "edgeagent-research"
            helper.write_text("#!/bin/sh\nexit 2\n")
            helper.chmod(0o700)
            manifest = dict.fromkeys(owned.common.FIELDS)
            manifest.update(version=2, helper={"path": str(helper), "sha256": hashlib.sha256(helper.read_bytes()).hexdigest()})
            path = root / "manifest.json"
            path.write_bytes(owned.common.canonical(manifest))
            with self.assertRaises(owned.common.HarnessError):
                owned.supervise(path, hashlib.sha256(path.read_bytes()).hexdigest(), "offline-explanation-worker-owned")

    def test_model_is_terminated_when_helper_fails_after_startup(self):
        import hashlib
        import os
        import tempfile
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pidfile = root / "model.pid"
            helper = root / "edgeagent-research"
            helper.write_text("#!/bin/sh\nprintf '%s\\n' '{\"kind\":\"research.explanation-runtime.required.v2\"}'\ncat >/dev/null\nexit 2\n")
            helper.chmod(0o700)
            manifest = dict.fromkeys(owned.common.FIELDS)
            manifest.update(version=2, helper={"path": str(helper), "sha256": hashlib.sha256(helper.read_bytes()).hexdigest()},
                            runtime={"sha256": "a"*64}, libraries=[])
            path = root / "manifest.json"
            path.write_bytes(owned.common.canonical(manifest))
            def command(_manifest, port):
                code = ("import os,http.server; from pathlib import Path; "
                        f"Path({str(pidfile)!r}).write_text(str(os.getpid())); "
                        "exec('class Handler(http.server.BaseHTTPRequestHandler):\\n def do_GET(self): self.send_response(200); self.end_headers()\\n def log_message(self,*args): pass'); "
                        f"http.server.HTTPServer(('127.0.0.1',{port}),Handler).serve_forever()")
                return [sys.executable, "-c", code]
            with patch.object(owned, "preflight", return_value=None), patch.object(owned.common, "command", side_effect=command):
                with self.assertRaises(owned.common.HarnessError):
                    owned.supervise(path, hashlib.sha256(path.read_bytes()).hexdigest(), "offline-explanation-worker-owned")
            with self.assertRaises(ProcessLookupError):
                os.kill(int(pidfile.read_text()), 0)

    def test_sigterm_reaps_the_waiting_helper(self):
        import hashlib
        import os
        import signal
        import subprocess
        import tempfile
        import time
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pidfile = root / "helper.pid"
            helper = root / "edgeagent-research"
            helper.write_text(f"#!{sys.executable}\nimport os,time\nfrom pathlib import Path\nPath({str(pidfile)!r}).write_text(str(os.getpid()))\ntime.sleep(60)\n")
            helper.chmod(0o700)
            manifest = dict.fromkeys(owned.common.FIELDS)
            manifest.update(version=2, helper={"path": str(helper), "sha256": hashlib.sha256(helper.read_bytes()).hexdigest()})
            path = root / "manifest.json"
            path.write_bytes(owned.common.canonical(manifest))
            process = subprocess.Popen([sys.executable, owned.__file__, "--manifest", str(path),
                                        "--execute-manifest", hashlib.sha256(path.read_bytes()).hexdigest(),
                                        "--output", str(root / "output")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                deadline = time.monotonic() + 5
                while not pidfile.exists() and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertTrue(pidfile.exists())
                process.send_signal(signal.SIGTERM)
                self.assertEqual(process.wait(timeout=10), 2)
                with self.assertRaises(ProcessLookupError):
                    os.kill(int(pidfile.read_text()), 0)
            finally:
                owned.stop(process)

    def test_sigkill_cannot_leave_a_live_helper(self):
        import hashlib
        import os
        import signal
        import subprocess
        import tempfile
        import time
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pidfile = root / "helper.pid"
            helper = root / "edgeagent-research"
            helper.write_text(f"#!{sys.executable}\nimport os,time\nfrom pathlib import Path\nPath({str(pidfile)!r}).write_text(str(os.getpid()))\ntime.sleep(60)\n")
            helper.chmod(0o700)
            manifest = dict.fromkeys(owned.common.FIELDS)
            manifest.update(version=2, helper={"path": str(helper), "sha256": hashlib.sha256(helper.read_bytes()).hexdigest()})
            path = root / "manifest.json"
            path.write_bytes(owned.common.canonical(manifest))
            process = subprocess.Popen([sys.executable, owned.__file__, "--manifest", str(path),
                "--execute-manifest", hashlib.sha256(path.read_bytes()).hexdigest(), "--output", str(root / "out")],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            pid = None
            def live():
                try:
                    return Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[0] != "Z"
                except FileNotFoundError:
                    return False
            try:
                deadline = time.monotonic() + 5
                while not pidfile.exists() and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertTrue(pidfile.exists())
                pid = int(pidfile.read_text())
                process.kill()
                process.wait(timeout=5)
                deadline = time.monotonic() + 3
                while live() and time.monotonic() < deadline:
                    time.sleep(0.01)
                self.assertFalse(live(), "an unowned helper survived supervisor death")
            finally:
                owned.stop(process)
                if pid is not None and live():
                    os.kill(pid, signal.SIGKILL)
