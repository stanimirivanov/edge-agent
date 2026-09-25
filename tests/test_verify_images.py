from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import unittest


MODULE_PATH = Path(__file__).parents[1] / "scripts" / "verify_images.py"
SPEC = importlib.util.spec_from_file_location("verify_images", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"could not load verifier from {MODULE_PATH}")
verify_images = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = verify_images
SPEC.loader.exec_module(verify_images)


class VerifyImagesTests(unittest.TestCase):
    def test_current_image_contract_is_valid(self) -> None:
        root = Path(__file__).parents[1]

        contract, specs, problems = verify_images.verify(root)

        self.assertIsNotNone(contract)
        self.assertEqual(5, len(specs))
        self.assertEqual([], problems)

    def test_descriptor_component_mismatch_is_rejected(self) -> None:
        spec = verify_images.ImageSpec(
            service="gateway",
            package="edgeagent-gateway",
            binary="edgeagent-gateway",
            component="edgeagent.gateway",
            capabilities=("command-intake", "query-composition"),
        )
        output = "\n".join(
            [
                "contract=edgeagent.component.v1",
                "component=edgeagent.research",
                "summary=Wrong component.",
                "capabilities=command-intake,query-composition",
            ]
        )

        problems = verify_images.check_descriptor(spec, output)

        self.assertEqual(1, len(problems))
        self.assertIn("component", problems[0].message)

    def test_build_inputs_are_digest_pinned(self) -> None:
        root = Path(__file__).parents[1]

        contract, _, problems = verify_images.verify(root)

        self.assertEqual([], problems)
        self.assertIsNotNone(contract)
        self.assertRegex(contract.dockerfile_frontend, r"@sha256:[0-9a-f]{64}$")
        self.assertRegex(contract.builder_image, r"@sha256:[0-9a-f]{64}$")

    def test_descriptor_with_unknown_field_is_rejected(self) -> None:
        spec = verify_images.ImageSpec(
            service="research",
            package="edgeagent-research",
            binary="edgeagent-research",
            component="edgeagent.research",
            capabilities=("artifact-publication", "deterministic-research"),
        )
        output = "\n".join(
            [
                "contract=edgeagent.component.v1",
                "component=edgeagent.research",
                "summary=Research.",
                "capabilities=artifact-publication,deterministic-research",
                "extra=unexpected",
            ]
        )

        problems = verify_images.check_descriptor(spec, output)

        self.assertEqual(1, len(problems))
        self.assertIn("fields", problems[0].message)


if __name__ == "__main__":
    unittest.main()
