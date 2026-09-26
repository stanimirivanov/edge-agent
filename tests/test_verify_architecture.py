from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import unittest


MODULE_PATH = Path(__file__).parents[1] / "scripts" / "verify_architecture.py"
SPEC = importlib.util.spec_from_file_location("verify_architecture", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"could not load verifier from {MODULE_PATH}")
verify_architecture = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = verify_architecture
SPEC.loader.exec_module(verify_architecture)


class VerifyArchitectureTests(unittest.TestCase):
    def test_current_workspace_satisfies_architecture_rules(self) -> None:
        root = Path(__file__).parents[1]

        self.assertEqual([], verify_architecture.verify(root))

    def test_peer_service_dependency_is_rejected(self) -> None:
        problems = verify_architecture.check_dependencies(
            "services/gateway",
            {
                "edgeagent-contracts",
                "edgeagent-research",
                "edgeagent-service-runtime",
            },
            verify_architecture.SERVICE_DEPENDENCIES,
        )

        self.assertEqual(1, len(problems))
        self.assertIn("edgeagent-research", problems[0].message)

    def test_contract_dependency_outside_allowlist_is_rejected(self) -> None:
        problems = verify_architecture.check_dependencies(
            "crates/contracts",
            {*verify_architecture.CONTRACT_DEPENDENCIES, "async-nats"},
            verify_architecture.CONTRACT_DEPENDENCIES,
        )

        self.assertEqual(1, len(problems))
        self.assertIn("async-nats", problems[0].message)

    def test_application_messaging_port_cannot_import_broker_client(self) -> None:
        problems = verify_architecture.check_dependencies(
            "crates/messaging",
            {*verify_architecture.MESSAGING_DEPENDENCIES, "async-nats"},
            verify_architecture.MESSAGING_DEPENDENCIES,
        )

        self.assertEqual(1, len(problems))
        self.assertIn("async-nats", problems[0].message)

    def test_adapter_test_dependencies_are_explicitly_bounded(self) -> None:
        problems = verify_architecture.check_dependencies(
            "crates/messaging-nats [dev-dependencies]",
            {*verify_architecture.MESSAGING_NATS_DEV_DEPENDENCIES, "testcontainers"},
            verify_architecture.MESSAGING_NATS_DEV_DEPENDENCIES,
        )

        self.assertEqual(1, len(problems))
        self.assertIn("testcontainers", problems[0].message)

    def test_missing_workspace_member_is_rejected(self) -> None:
        members = set(verify_architecture.PACKAGE_RULES)
        members.remove("services/research")

        problems = verify_architecture.check_workspace_members(members)

        self.assertEqual(1, len(problems))
        self.assertIn("services/research", problems[0].message)


if __name__ == "__main__":
    unittest.main()
