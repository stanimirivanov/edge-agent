from __future__ import annotations

import importlib.util
from contextlib import redirect_stderr, redirect_stdout
from dataclasses import replace
from io import StringIO
from pathlib import Path
import shutil
import sys
import tempfile
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

    def test_postgres_outbox_does_not_import_transport_adapter(self) -> None:
        problems = verify_architecture.check_dependencies(
            "crates/outbox-postgres",
            {*verify_architecture.OUTBOX_POSTGRES_DEPENDENCIES, "async-nats"},
            verify_architecture.OUTBOX_POSTGRES_DEPENDENCIES,
        )

        self.assertEqual(1, len(problems))
        self.assertIn("async-nats", problems[0].message)

    def test_postgres_inbox_does_not_import_transport_adapter(self) -> None:
        problems = verify_architecture.check_dependencies(
            "crates/inbox-postgres",
            {*verify_architecture.INBOX_POSTGRES_DEPENDENCIES, "async-nats"},
            verify_architecture.INBOX_POSTGRES_DEPENDENCIES,
        )

        self.assertEqual(1, len(problems))
        self.assertIn("async-nats", problems[0].message)

    def test_outbox_relay_depends_on_port_not_transport_adapter(self) -> None:
        problems = verify_architecture.check_dependencies(
            "crates/outbox-relay",
            {*verify_architecture.OUTBOX_RELAY_DEPENDENCIES, "async-nats"},
            verify_architecture.OUTBOX_RELAY_DEPENDENCIES,
        )

        self.assertEqual(1, len(problems))
        self.assertIn("async-nats", problems[0].message)

    def test_outbox_relay_cannot_import_storage_adapter_or_driver(self) -> None:
        for dependency in (
            verify_architecture.OUTBOX_POSTGRES,
            "tokio-postgres",
        ):
            with self.subTest(dependency=dependency):
                problems = verify_architecture.check_dependencies(
                    "crates/outbox-relay",
                    {
                        *verify_architecture.OUTBOX_RELAY_DEPENDENCIES,
                        dependency,
                    },
                    verify_architecture.OUTBOX_RELAY_DEPENDENCIES,
                )

                self.assertEqual(1, len(problems))
                self.assertIn(dependency, problems[0].message)

    def test_inbox_handler_cannot_import_storage_adapter_or_driver(self) -> None:
        for dependency in (
            verify_architecture.INBOX_POSTGRES,
            "tokio-postgres",
        ):
            with self.subTest(dependency=dependency):
                problems = verify_architecture.check_dependencies(
                    "crates/inbox-handler",
                    {
                        *verify_architecture.INBOX_HANDLER_DEPENDENCIES,
                        dependency,
                    },
                    verify_architecture.INBOX_HANDLER_DEPENDENCIES,
                )

                self.assertEqual(1, len(problems))
                self.assertIn(dependency, problems[0].message)

    def test_telemetry_cannot_import_transport_or_storage_adapters(self) -> None:
        problems = verify_architecture.check_dependencies(
            "crates/telemetry",
            {*verify_architecture.TELEMETRY_DEPENDENCIES, "async-nats"},
            verify_architecture.TELEMETRY_DEPENDENCIES,
        )

        self.assertEqual(1, len(problems))
        self.assertIn("async-nats", problems[0].message)

    def test_architecture_tool_dependencies_are_explicitly_bounded(self) -> None:
        problems = verify_architecture.check_dependencies(
            "crates/xtask",
            {*verify_architecture.XTASK_DEPENDENCIES, "toml"},
            verify_architecture.XTASK_DEPENDENCIES,
        )

        self.assertEqual(1, len(problems))
        self.assertIn("toml", problems[0].message)

    def test_missing_workspace_member_is_rejected(self) -> None:
        members = set(verify_architecture.PACKAGE_RULES)
        members.remove("services/research")

        problems = verify_architecture.check_workspace_members(members)

        self.assertEqual(1, len(problems))
        self.assertIn("services/research", problems[0].message)


class ManifestDeclarationTests(unittest.TestCase):
    """Exercise discovery through real TOML fixtures, not precomputed key sets."""

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        repository = MODULE_PATH.parents[1]
        shutil.copyfile(repository / "Cargo.toml", self.root / "Cargo.toml")
        for relative, rule in verify_architecture.PACKAGE_RULES.items():
            directory = self.root / relative
            (directory / "src").mkdir(parents=True)
            shutil.copyfile(
                repository / relative / "Cargo.toml", directory / "Cargo.toml"
            )
            source = "lib.rs" if rule.target == "lib" else "main.rs"
            (directory / "src" / source).write_text("", encoding="utf-8")

    def manifest(self, relative: str) -> Path:
        return self.root / relative / "Cargo.toml"

    def replace_once(self, path: Path, old: str, new: str) -> None:
        original = path.read_text(encoding="utf-8")
        self.assertEqual(1, original.count(old), f"fixture declaration changed: {old}")
        path.write_text(original.replace(old, new), encoding="utf-8")

    def assert_rejected(self, table: str, alias: str, package: str) -> None:
        problems = verify_architecture.verify(self.root)
        self.assertTrue(
            any(
                table in problem.path
                and alias in problem.message
                and package in problem.message
                and "undeclared dependency boundary" in problem.message
                for problem in problems
            ),
            problems,
        )

    def test_build_and_all_target_dependency_kinds_are_checked(self) -> None:
        path = self.manifest("crates/contracts")
        original = path.read_text(encoding="utf-8")
        tables = ["build-dependencies"]
        for target in ("cfg(any())", "cfg(windows)", "x86_64-pc-windows-msvc"):
            tables.extend(
                f"target.'{target}'.{kind}"
                for kind in ("dependencies", "dev-dependencies", "build-dependencies")
            )
        for table in tables:
            with self.subTest(table=table):
                path.write_text(
                    original + f"\n[{table}]\nsqlx = {{ workspace = true }}\n",
                    encoding="utf-8",
                )
                self.assert_rejected(table, "sqlx", "sqlx")

    def test_cli_fails_with_actionable_hidden_dependency_diagnostic(self) -> None:
        path = self.manifest("crates/contracts")
        table = "target.'cfg(any())'.build-dependencies"
        path.write_text(
            path.read_text(encoding="utf-8")
            + f'\n[{table}]\nhidden = {{ package = "sqlx", version = "=0.9.0", optional = true }}\n',
            encoding="utf-8",
        )
        stderr = StringIO()
        stdout = StringIO()

        with redirect_stderr(stderr), redirect_stdout(stdout):
            result = verify_architecture.main(["--root", str(self.root)])

        self.assertEqual(1, result)
        self.assertEqual("", stdout.getvalue())
        for evidence in (
            "crates/contracts/Cargo.toml",
            table,
            "sqlx",
            "alias 'hidden'",
            "optional=true",
            "remove or move the declaration",
            "docs/architecture/dependency-rules.md",
            "FAILED: 1 architecture problem(s)",
        ):
            self.assertIn(evidence, stderr.getvalue())

    def test_inactive_optional_dependency_is_checked_by_package_not_alias(self) -> None:
        path = self.manifest("crates/contracts")
        table = "target.'cfg(any())'.dependencies"
        path.write_text(
            path.read_text(encoding="utf-8")
            + f'\n[{table}]\ninnocent = {{ package = "sqlx", version = "=0.9.0", optional = true }}\n',
            encoding="utf-8",
        )

        self.assert_rejected(table, "innocent", "sqlx")
        self.assertTrue(
            any(
                "optional=true" in problem.message
                for problem in verify_architecture.verify(self.root)
            )
        )

    def test_approved_alias_cannot_retarget_its_package(self) -> None:
        path = self.manifest("crates/contracts")
        self.replace_once(
            path,
            "serde.workspace = true",
            'serde = { package = "sqlx", version = "=0.9.0" }',
        )

        self.assert_rejected("dependencies", "serde", "sqlx")

    def test_workspace_alias_identity_is_checked_against_fixed_policy(self) -> None:
        path = self.root / "Cargo.toml"
        self.replace_once(
            path,
            'serde = { version = "=1.0.229",',
            'serde = { package = "sqlx", version = "=1.0.229",',
        )

        self.assert_rejected("dependencies", "serde", "sqlx")

    def test_approved_package_can_be_renamed_directly_or_inherited(self) -> None:
        path = self.manifest("crates/contracts")
        original = path.read_text(encoding="utf-8")
        self.assertEqual(1, original.count("serde.workspace = true"))
        workspace = self.root / "Cargo.toml"
        self.replace_once(
            workspace,
            "[workspace.dependencies]",
            '[workspace.dependencies]\nrenamed = { package = "serde", version = "=1.0.229" }',
        )
        for declaration in (
            'renamed = { package = "serde", version = "=1.0.229" }',
            'renamed = { workspace = true, optional = true, features = ["derive"], default-features = true }',
        ):
            with self.subTest(declaration=declaration):
                path.write_text(
                    original.replace("serde.workspace = true", declaration),
                    encoding="utf-8",
                )
                self.assertEqual([], verify_architecture.verify(self.root))

    def test_approved_declarations_can_span_targets_without_expanding_their_kind(self) -> None:
        path = self.manifest("crates/inbox-handler")
        self.replace_once(path, "sqlx.workspace = true\n", "")
        path.write_text(
            path.read_text(encoding="utf-8")
            + "\n[target.'cfg(any())'.dev-dependencies]\nsqlx = { workspace = true }\n",
            encoding="utf-8",
        )

        self.assertEqual([], verify_architecture.verify(self.root))

    def test_normal_identities_can_span_target_tables(self) -> None:
        path = self.manifest("crates/contracts")
        self.replace_once(path, "url.workspace = true\n", "")
        path.write_text(
            path.read_text(encoding="utf-8")
            + "\n[target.'cfg(any())'.dependencies]\nurl = { workspace = true }\n",
            encoding="utf-8",
        )

        self.assertEqual([], verify_architecture.verify(self.root))

    def test_development_allowance_does_not_authorize_normal_or_build_usage(self) -> None:
        path = self.manifest("crates/inbox-handler")
        original = path.read_text(encoding="utf-8")
        for kind in ("dependencies", "build-dependencies"):
            table = f"target.'cfg(any())'.{kind}"
            with self.subTest(kind=kind):
                path.write_text(
                    original + f"\n[{table}]\nsqlx = {{ workspace = true }}\n",
                    encoding="utf-8",
                )
                self.assert_rejected(table, "sqlx", "sqlx")

    def test_normal_allowance_does_not_authorize_build_usage(self) -> None:
        path = self.manifest("crates/contracts")
        path.write_text(
            path.read_text(encoding="utf-8") + "\n[build-dependencies]\nserde = { workspace = true }\n",
            encoding="utf-8",
        )

        self.assert_rejected("build-dependencies", "serde", "serde")

    def test_build_allowance_requires_its_own_reviewed_policy(self) -> None:
        rule = replace(
            verify_architecture.PACKAGE_RULES["crates/contracts"],
            build_dependencies=frozenset({"serde"}),
        )
        manifest = verify_architecture._read_toml(self.manifest("crates/contracts"))
        manifest["build-dependencies"] = {"serde": {"workspace": True}}
        workspace = verify_architecture._read_toml(self.root / "Cargo.toml")

        problems = verify_architecture.check_manifest_dependencies(
            "crates/contracts", manifest, workspace["workspace"]["dependencies"], rule
        )

        self.assertEqual([], problems)

    def test_missing_workspace_entry_is_reported(self) -> None:
        path = self.manifest("crates/contracts")
        path.write_text(
            path.read_text(encoding="utf-8") + "\n[build-dependencies]\nunknown = { workspace = true }\n",
            encoding="utf-8",
        )

        problems = verify_architecture.verify(self.root)
        self.assertTrue(
            any(
                "unknown" in problem.message and "workspace.dependencies" in problem.message
                for problem in problems
            ),
            problems,
        )

    def test_invalid_shapes_and_inheritance_fail_closed(self) -> None:
        path = self.manifest("crates/contracts")
        original = path.read_text(encoding="utf-8")
        cases = (
            ("\nbuild-dependencies = 1\n", "must be a table", False),
            ("\ntarget = []\n", "target must be a table", False),
            ("\n[target]\n'cfg(any())' = 1\n", "'cfg(any())' must be a table", True),
            ("\n[target.'cfg(any())']\ndependencies = []\n", "must be a table", True),
            ("\n[build-dependencies]\nserde = false\n", "version string or table", True),
            ('\n[build-dependencies]\nserde = { package = 1 }\n', "package must be a non-empty string", True),
            ('\n[build-dependencies]\nserde = { optional = "true" }\n', "optional must be a boolean", True),
            ('\n[build-dependencies]\nserde = { workspace = false }\n', "workspace must be true", True),
            ('\n[build-dependencies]\nserde = { workspace = true, package = "sqlx" }\n', "cannot override package", True),
            ('\n[build-dependencies]\nserde = { workspace = true, path = "../sqlx" }\n', "cannot override path", True),
            ('\n[build-dependencies]\nserde = { workspace = true, git = "https://example.invalid" }\n', "cannot override git", True),
            ('\n[build-dependencies]\nserde = { workspace = true, version = "=0.9.0" }\n', "cannot override version", True),
        )
        for suffix, expected, append in cases:
            with self.subTest(suffix=suffix):
                # Root scalar keys must precede package tables in valid TOML.
                text = original + suffix if append else suffix + original
                path.write_text(text, encoding="utf-8")
                problems = verify_architecture.verify(self.root)
                self.assertTrue(
                    any(expected in problem.message for problem in problems), problems
                )

    def test_malformed_workspace_dependency_table_is_reported(self) -> None:
        path = self.root / "Cargo.toml"
        self.replace_once(path, "[workspace.dependencies]", "[workspace.not_dependencies]")
        self.replace_once(
            path,
            "[workspace.dependencies.tokio]",
            "[workspace.not_dependencies.tokio]",
        )
        self.replace_once(path, "resolver = \"3\"", "resolver = \"3\"\ndependencies = []")

        self.assertTrue(
            any(
                problem.path == "Cargo.toml"
                and problem.message == "workspace.dependencies must be a table"
                for problem in verify_architecture.verify(self.root)
            )
        )

    def test_legacy_dependency_table_spellings_are_rejected(self) -> None:
        path = self.manifest("crates/contracts")
        original = path.read_text(encoding="utf-8")
        for prefix in ("", "target.'cfg(any())'."):
            for legacy in ("dev_dependencies", "build_dependencies"):
                with self.subTest(prefix=prefix, legacy=legacy):
                    path.write_text(
                        original + f"\n[{prefix}{legacy}]\nsqlx = {{ workspace = true }}\n",
                        encoding="utf-8",
                    )
                    problems = verify_architecture.verify(self.root)
                    self.assertTrue(
                        any(
                            legacy in problem.message
                            and legacy.replace("_", "-") in problem.message
                            for problem in problems
                        ),
                        problems,
                    )


if __name__ == "__main__":
    unittest.main()
