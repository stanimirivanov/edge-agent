from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest


SCRIPTS = Path(__file__).parents[1] / "scripts"
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))
MODULE_PATH = SCRIPTS / "verify_release.py"
SPEC = importlib.util.spec_from_file_location("verify_release", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"could not load release verifier from {MODULE_PATH}")
verify_release = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = verify_release
SPEC.loader.exec_module(verify_release)


class VerifyReleaseTests(unittest.TestCase):
    def test_current_release_contract_is_valid(self) -> None:
        root = Path(__file__).parents[1]

        problems = verify_release.verify(root)

        self.assertEqual([], problems)

    def test_mutable_latest_tag_is_rejected(self) -> None:
        root = Path(__file__).parents[1]
        policy, problems = verify_release.load_release_policy(
            root / "supply-chain" / "release.toml"
        )
        self.assertEqual([], problems)
        self.assertIsNotNone(policy)
        source = (root / ".github" / "workflows" / "release-images.yml").read_text(
            encoding="utf-8"
        )
        source = source.replace(
            "${{ matrix.image }}:${{ github.sha }}",
            "${{ matrix.image }}:latest",
        )
        with tempfile.TemporaryDirectory() as directory:
            workflow = Path(directory) / "release-images.yml"
            workflow.write_text(source, encoding="utf-8")

            workflow_problems = verify_release.check_workflow(
                workflow,
                policy,
                syft_action_sha="3ad7283483fc7af8ff2b4ea19663c2d5ca935e26",
                syft_action_version="v0.24.2",
                syft_version="1.52.0",
            )

        self.assertTrue(
            any("latest tag" in problem.message for problem in workflow_problems)
        )


if __name__ == "__main__":
    unittest.main()
