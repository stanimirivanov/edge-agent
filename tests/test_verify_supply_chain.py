from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest


MODULE_PATH = Path(__file__).parents[1] / "scripts" / "verify_supply_chain.py"
SPEC = importlib.util.spec_from_file_location("verify_supply_chain", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"could not load verifier from {MODULE_PATH}")
verify_supply_chain = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = verify_supply_chain
SPEC.loader.exec_module(verify_supply_chain)


class VerifySupplyChainTests(unittest.TestCase):
    def test_current_supply_chain_contract_is_valid(self) -> None:
        root = Path(__file__).parents[1]

        problems = verify_supply_chain.verify(root)

        self.assertEqual([], problems)

    def test_unapproved_license_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            policy = Path(directory) / "deny.toml"
            source = (Path(__file__).parents[1] / "deny.toml").read_text(encoding="utf-8")
            policy.write_text(
                source.replace('    "Zlib",\n', '    "Zlib",\n    "GPL-3.0-only",\n'),
                encoding="utf-8",
            )

            problems = verify_supply_chain.check_deny_policy(policy)

        self.assertTrue(any("approved set" in problem.message for problem in problems))

    def test_artifact_set_requires_source_and_image_sboms(self) -> None:
        tools = verify_supply_chain.ToolPolicy(
            cargo_deny="0.20.2",
            cargo_cyclonedx="0.5.9",
            syft="1.52.0",
            rust_spec="1.5",
            accepted_specs=("1.5", "1.6"),
            retention_days=14,
            cargo_deny_action="v2.1.1",
            cargo_deny_action_sha="3c6349835b2b7b196a839186cb8b78e02f7b5f25",
            download_syft_action="v0.24.2",
            download_syft_action_sha="3ad7283483fc7af8ff2b4ea19663c2d5ca935e26",
            upload_artifact_action="v7.0.1",
            upload_artifact_action_sha="043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
        )
        deployable = verify_supply_chain.ArtifactSpec(
            service="gateway",
            binary="edgeagent-gateway",
        )
        with tempfile.TemporaryDirectory() as directory:
            artifacts = Path(directory)
            document = {
                "bomFormat": "CycloneDX",
                "specVersion": "1.5",
                "version": 1,
                "metadata": {"component": {"name": "edgeagent-gateway"}},
            }
            (artifacts / deployable.source_filename).write_text(
                json.dumps(document),
                encoding="utf-8",
            )

            problems = verify_supply_chain.check_artifacts(
                artifacts,
                [deployable],
                tools,
            )

        self.assertEqual(1, len(problems))
        self.assertIn(deployable.image_filename, problems[0].message)

    def test_invalid_cyclonedx_envelope_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "invalid.cdx.json"
            path.write_text('{"bomFormat": "SPDX"}', encoding="utf-8")

            problems = verify_supply_chain.check_sbom(path, ("1.5", "1.6"))

        self.assertGreaterEqual(len(problems), 3)


if __name__ == "__main__":
    unittest.main()
