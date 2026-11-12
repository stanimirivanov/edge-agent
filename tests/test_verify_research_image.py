"""Supervisor failure must clean up both owned dependency containers."""
from __future__ import annotations

import contextlib
import io
import subprocess
import unittest
from unittest.mock import MagicMock, patch

from scripts import verify_research_image as supervisor


class ResearchImageTests(unittest.TestCase):
    def test_failed_qualification_removes_owned_dependencies(self) -> None:
        def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
            return subprocess.CompletedProcess(command, 1 if command[0] == "cargo" else 0)

        with (
            patch.object(supervisor, "docker", side_effect=[
                "pg-id", "nats-id", "127.0.0.1:15436", "127.0.0.1:15438",
            ]) as docker,
            patch.object(supervisor.subprocess, "run", side_effect=run) as invoked,
            patch.object(supervisor.socket, "create_connection", return_value=MagicMock()),
        ):
            self.assertEqual(supervisor.main(), 1)
        started = [call.args[3] for call in docker.call_args_list if call.args[0] == "run"]
        removed = [call.args[0][-1] for call in invoked.call_args_list
                   if call.args[0][:4] == ["docker", "rm", "-f", "-v"]]
        self.assertEqual(removed, list(reversed(started)))
        self.assertEqual(len(set(removed)), 2)

    def test_dependency_start_failure_reports_docker_cause_and_cleans_owned_names(self) -> None:
        for failed_service in ("pg", "nats"):
            with self.subTest(service=failed_service):
                commands: list[list[str]] = []
                diagnostic = "Error response from daemon: dependency startup cause"

                def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
                    commands.append(command)
                    if command[:2] == ["docker", "run"] and command[4].endswith(f"-{failed_service}"):
                        raise subprocess.CalledProcessError(125, command, stderr=diagnostic)
                    return subprocess.CompletedProcess(command, 0, stdout="container-id", stderr="")

                stderr = io.StringIO()
                with patch.object(supervisor.subprocess, "run", side_effect=run), contextlib.redirect_stderr(stderr):
                    self.assertEqual(supervisor.main(), 1)
                self.assertIn(diagnostic, stderr.getvalue())
                self.assertIn("125", stderr.getvalue())
                self.assertNotIn("POSTGRES_PASSWORD", stderr.getvalue())
                self.assertFalse(any(command[0] == "cargo" for command in commands))
                started = [command[4] for command in commands if command[:2] == ["docker", "run"]]
                removed = [command[-1] for command in commands if command[:4] == ["docker", "rm", "-f", "-v"]]
                self.assertEqual(removed, list(reversed(started)))

    def test_timeout_reports_bounded_partial_stderr_and_cleans_dependency(self) -> None:
        commands: list[list[str]] = []

        def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
            commands.append(command)
            if command[:2] == ["docker", "run"]:
                error = subprocess.TimeoutExpired(command, 60)
                error.stderr = b"x" * 20000 + b"daemon: final startup diagnostic"
                raise error
            return subprocess.CompletedProcess(command, 0, stdout="", stderr="")

        stderr = io.StringIO()
        with patch.object(supervisor.subprocess, "run", side_effect=run), contextlib.redirect_stderr(stderr):
            self.assertEqual(supervisor.main(), 1)
        self.assertIn("timed out", stderr.getvalue())
        self.assertIn("daemon: final startup diagnostic", stderr.getvalue())
        self.assertLess(len(stderr.getvalue()), 5000)
        self.assertEqual(len([command for command in commands if command[:4] == ["docker", "rm", "-f", "-v"]]), 1)


if __name__ == "__main__":
    unittest.main()
