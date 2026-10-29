"""Deterministic launcher proofs; these mocks do not claim Linux isolation ran."""

from contextlib import redirect_stdout
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "offline_source_launcher", ROOT / "scripts" / "verify_offline_source.py"
)
assert SPEC is not None and SPEC.loader is not None
LAUNCHER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LAUNCHER)


class OfflineLauncherTests(unittest.TestCase):
    def test_unsupported_platform_cannot_pass_or_execute_a_child(self):
        with patch.object(LAUNCHER.sys, "platform", "unsupported"), patch.object(
            LAUNCHER.subprocess, "run"
        ) as run, redirect_stdout(io.StringIO()):
            self.assertEqual(LAUNCHER.main(), 1)
            run.assert_not_called()

    def test_locked_cached_build_precedes_unconditional_network_namespace(self):
        artifact = {"reason": "compiler-artifact", "target": {"name": "offline"},
                    "executable": "/invented/offline"}
        built = subprocess.CompletedProcess([], 0, json.dumps(artifact), "")
        failed_proof = subprocess.CompletedProcess([], 31)
        with patch.object(LAUNCHER.sys, "platform", "linux"), patch.object(
            LAUNCHER.subprocess, "run", side_effect=[built, failed_proof]
        ) as run:
            self.assertEqual(LAUNCHER.main(), 31)
            build = run.call_args_list[0].args[0]
            self.assertIn("--offline", build)
            self.assertIn("--locked", build)
            self.assertIn("--no-run", build)
            self.assertEqual(run.call_args_list[1].args[0], [
                "sudo", "-n", "unshare", "--net", "--", "/invented/offline", "--test-threads=1"
            ])

    def test_build_failure_never_dispatches_the_isolation_child(self):
        with patch.object(LAUNCHER.sys, "platform", "linux"), patch.object(
            LAUNCHER.subprocess, "run", return_value=subprocess.CompletedProcess([], 23, "", "build failed")
        ) as run, redirect_stdout(io.StringIO()):
            self.assertEqual(LAUNCHER.main(), 23)
            self.assertEqual(run.call_count, 1)

    def test_missing_or_ambiguous_executable_never_silently_skips(self):
        artifact = {"reason": "compiler-artifact", "target": {"name": "offline"},
                    "executable": "/invented/offline"}
        for output in ["", "\n".join([json.dumps(artifact)] * 2)]:
            with self.subTest(output=output), patch.object(LAUNCHER.sys, "platform", "linux"), patch.object(
                LAUNCHER.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, output, "")
            ) as run, redirect_stdout(io.StringIO()):
                self.assertEqual(LAUNCHER.main(), 1)
                self.assertEqual(run.call_count, 1)
