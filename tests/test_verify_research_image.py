"""Supervisor failure must clean up both owned dependency containers."""
from __future__ import annotations

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


if __name__ == "__main__":
    unittest.main()
