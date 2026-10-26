from __future__ import annotations

from contextlib import redirect_stderr, redirect_stdout
from dataclasses import replace
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("qualify_daily", ROOT / "scripts/qualify_daily.py")
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("could not load qualification harness")
harness = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = harness
SPEC.loader.exec_module(harness)


class UnreadableEnvironment(dict):
    def __contains__(self, key):
        raise AssertionError("offline execution read environment")

    def get(self, key, default=None):
        raise AssertionError("offline execution read environment")

    def keys(self):
        raise AssertionError("offline execution copied environment")


class QualificationHarnessTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        directory = Path(self.temporary.name)
        self.profile = directory / "owner-private-profile.json"
        self.profile.write_bytes(b'{"invented":true}\n')
        self.binary = directory / "edgeagent-market-data.exe"
        self.binary.write_bytes(b"invented binary; never executed")
        self.invocation = harness.Invocation(
            "acquire", self.profile, hashlib.sha256(self.profile.read_bytes()).hexdigest(),
            self.binary, hashlib.sha256(self.binary.read_bytes()).hexdigest(),
            directory / "private-report.json",
        )

    def report(self):
        return json.loads(self.invocation.report.read_text(encoding="utf-8"))

    def execution(self, **changes):
        return replace(self.invocation, execute_profile=self.invocation.profile_sha256, **changes)

    def environment(self):
        return {"EDGEAGENT_QUALIFICATION_LIVE_OPT_IN": self.invocation.profile_sha256}

    def test_default_validates_local_pins_but_never_reads_environment_or_executes(self):
        runner = Mock(side_effect=AssertionError("unexpected external work"))
        self.assertEqual(harness.supervise(self.invocation, environment=UnreadableEnvironment(), runner=runner),
                         (2, "not_run"))
        runner.assert_not_called()
        report = self.report()
        self.assertEqual(report["harness_sha256"], hashlib.sha256((ROOT / "scripts/qualify_daily.py").read_bytes()).hexdigest())
        self.assertEqual(report["live_qualification"], "not_run")
        self.assertEqual(report["cleanup_verification"], "not_run")
        self.assertEqual(report["external_effects"], "not_started")
        original = self.invocation.report.read_bytes()
        self.invocation.report.unlink()
        harness.supervise(self.invocation, environment=UnreadableEnvironment(), runner=runner)
        self.assertEqual(self.invocation.report.read_bytes(), original)

    def test_pins_sizes_paths_names_and_exact_execution_opt_in_fail_closed(self):
        mutations = [
            replace(self.invocation, phase="arbitrary-shell"),
            replace(self.invocation, profile_sha256="private-rejected-value"),
            replace(self.invocation, profile_sha256="0" * 64),
            replace(self.invocation, binary_sha256="0" * 64),
            replace(self.invocation, profile=Path("relative-profile")),
            replace(self.invocation, binary=Path("unexpected")),
            replace(self.invocation, execute_profile="0" * 64),
        ]
        runner = Mock()
        for invocation in mutations:
            with self.subTest(invocation=invocation.phase):
                self.assertEqual(harness.supervise(invocation, runner=runner), (2, "blocked"))
                self.assertNotIn("private-rejected-value", self.invocation.report.read_text())
                self.invocation.report.unlink()
        for payload in (b"", b"x" * 8193):
            self.profile.write_bytes(payload)
            invocation = replace(self.invocation, profile_sha256=hashlib.sha256(payload).hexdigest())
            self.assertEqual(harness.supervise(invocation, runner=runner), (2, "blocked"))
            self.invocation.report.unlink()
        runner.assert_not_called()

    def test_acquisition_mock_flags_and_missing_runtime_opt_in_cannot_launch(self):
        runner = Mock()
        for environment in ({}, {"USE_MOCK": "0", **self.environment()},
                            {"USE_MOCK": "1", **self.environment()}):
            self.assertEqual(harness.supervise(self.execution(), environment=environment, runner=runner),
                             (2, "blocked"))
            self.assertEqual(self.report()["reason"], "source_opt_in_denied")
            self.invocation.report.unlink()
        runner.assert_not_called()

    def test_every_phase_executes_one_fixed_bounded_command_after_durable_running_report(self):
        for phase, (command, timeout) in harness.PHASES.items():
            with self.subTest(phase=phase):
                invocation = self.execution(phase=phase)

                def run(arguments, **options):
                    self.assertEqual(self.report()["status"], "running")
                    self.assertEqual(self.report()["external_effects"], "unknown")
                    self.assertEqual(arguments, [str(self.binary), command, str(self.profile),
                                                 invocation.profile_sha256])
                    self.assertEqual(options["timeout"], timeout)
                    self.assertFalse(options["shell"])
                    self.assertTrue(options["close_fds"])
                    for channel in ("stdin", "stdout", "stderr"):
                        self.assertEqual(options[channel], subprocess.DEVNULL)
                    return subprocess.CompletedProcess(arguments, 0, stdout="private-output")

                runner = Mock(side_effect=run)
                self.assertEqual(harness.supervise(invocation, environment=self.environment(), runner=runner),
                                 (0, "command_succeeded"))
                runner.assert_called_once()
                self.assertEqual(self.report()["live_qualification"], "not_verified")
                self.assertEqual(self.report()["external_effects"], "not_independently_verified")
                self.assertNotIn("private-output", self.invocation.report.read_text())
                self.assertNotIn(str(self.profile), self.invocation.report.read_text())
                if phase == "cleanup":
                    self.assertEqual(self.report()["cleanup_verification"], "not_verified")
                self.invocation.report.unlink()

    def test_failure_timeout_interrupt_and_launch_errors_never_retry_or_claim_erasure(self):
        cases = [
            (subprocess.CompletedProcess([], 2), 1, "command_failed"),
            (subprocess.TimeoutExpired("private-command", 420, output=b"private-payload"), 3, "unknown"),
            (KeyboardInterrupt(), 3, "unknown"),
            (OSError("private-path-and-credential"), 3, "unknown"),
            (ValueError("private-environment"), 3, "unknown"),
        ]
        for result, code, status in cases:
            with self.subTest(status=status):
                runner = Mock(side_effect=result) if isinstance(result, BaseException) else Mock(return_value=result)
                self.assertEqual(harness.supervise(self.execution(phase="cleanup"),
                                                 environment={}, runner=runner), (code, status))
                runner.assert_called_once()
                report = self.report()
                self.assertEqual(report["external_effects"], "unknown")
                self.assertEqual(report["cleanup_verification"], "not_verified")
                self.assertNotIn("private-", self.invocation.report.read_text())
                self.invocation.report.unlink()

    def test_existing_report_or_unrecordable_running_state_prevents_launch(self):
        runner = Mock()
        self.invocation.report.write_text("earlier-evidence", encoding="utf-8")
        with self.assertRaises(FileExistsError):
            harness.supervise(self.execution(), runner=runner)
        self.assertEqual(self.invocation.report.read_text(), "earlier-evidence")
        self.invocation.report.unlink()
        original = harness._persist

        def fail_running(stream, report):
            if report["status"] != "not_run":
                raise OSError("private-write-failure")
            original(stream, report)

        with patch.object(harness, "_persist", side_effect=fail_running), self.assertRaises(OSError):
            harness.supervise(self.execution(), environment=self.environment(), runner=runner)
        runner.assert_not_called()

    def test_cli_errors_are_redacted_and_checkout_reports_are_rejected(self):
        output = io.StringIO()
        with redirect_stderr(output), redirect_stdout(output):
            self.assertEqual(harness.main(["--phase", "private-rejected-value"]), 2)
        self.assertNotIn("private-rejected-value", output.getvalue())
        invocation = replace(self.invocation, report=ROOT / "never-create-this-report.json")
        with self.assertRaises(harness.HarnessError):
            harness.supervise(invocation, runner=Mock())
        self.assertFalse(invocation.report.exists())
        with patch.object(Path, "resolve", side_effect=RuntimeError("private-path-cycle")):
            with self.assertRaises(harness.HarnessError) as error:
                harness._open_report(self.invocation.report)
        self.assertEqual(str(error.exception), "invalid_report_path")

    def test_actual_cli_default_cannot_execute_even_an_invalid_binary(self):
        result = subprocess.run(
            [sys.executable, str(ROOT / "scripts/qualify_daily.py"),
             "--phase", "acquire", "--profile", str(self.profile),
             "--profile-sha256", self.invocation.profile_sha256,
             "--binary", str(self.binary), "--binary-sha256", self.invocation.binary_sha256,
             "--report", str(self.invocation.report)],
            capture_output=True, text=True, timeout=30, check=False,
        )
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, "qualification_harness_status=not_run\n")
        self.assertEqual(result.stderr, "")
        self.assertEqual(self.report()["live_qualification"], "not_run")

    def test_unrecordable_final_result_exits_unknown_without_exposing_io_details(self):
        arguments = ["--phase", "cleanup", "--profile", str(self.profile),
                     "--profile-sha256", self.invocation.profile_sha256,
                     "--binary", str(self.binary), "--binary-sha256", self.invocation.binary_sha256,
                     "--report", str(self.invocation.report)]
        output = io.StringIO()
        with patch.object(harness, "supervise", side_effect=OSError("private-report-path")), \
                redirect_stderr(output), redirect_stdout(output):
            self.assertEqual(harness.main(arguments), 3)
        self.assertEqual(output.getvalue(), "qualification_harness_status=report_io_failure\n")


if __name__ == "__main__":
    unittest.main()
