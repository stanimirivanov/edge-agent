from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import unittest


MODULE_PATH = Path(__file__).parents[1] / "scripts" / "verify_local_stack.py"
SPEC = importlib.util.spec_from_file_location("verify_local_stack", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"could not load verifier from {MODULE_PATH}")
verify_local_stack = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = verify_local_stack
SPEC.loader.exec_module(verify_local_stack)


class VerifyLocalStackTests(unittest.TestCase):
    def test_current_local_platform_contract_is_valid(self) -> None:
        root = Path(__file__).parents[1]

        specs, problems = verify_local_stack.verify(root)

        self.assertEqual(4, len(specs))
        self.assertEqual([], problems)

    def test_floating_image_tags_are_rejected(self) -> None:
        self.assertFalse(verify_local_stack._is_pinned_image("postgres:latest"))
        self.assertFalse(verify_local_stack._is_pinned_image("postgres"))
        self.assertTrue(verify_local_stack._is_pinned_image("postgres:18.6-alpine3.23"))

    def test_every_dependency_uses_a_distinct_readiness_port(self) -> None:
        root = Path(__file__).parents[1]
        specs, problems = verify_local_stack.verify(root)
        ports = [spec.readiness_port for spec in specs]

        self.assertEqual([], problems)
        self.assertEqual(len(ports), len(set(ports)))


if __name__ == "__main__":
    unittest.main()
