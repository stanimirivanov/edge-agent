from __future__ import annotations

import importlib.util
import io
from contextlib import nullcontext
from pathlib import Path
import stat
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch


MODULE_PATH = Path(__file__).parents[1] / "scripts" / "verify_supply_chain.py"
SPEC = importlib.util.spec_from_file_location("workspace_lock_inventory", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"could not load verifier from {MODULE_PATH}")
verifier = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = verifier
SPEC.loader.exec_module(verifier)


class WorkspaceLockInventoryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.write("Cargo.toml", "[workspace]\nmembers = []\n")
        self.write("Cargo.lock", "version = 4\n")

    def write(self, relative: str, content: str) -> Path:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        return path

    def messages(self) -> list[str]:
        return [problem.render() for problem in verifier.check_workspace_locks(self.root)]

    def test_minimal_root_workspace_is_valid(self) -> None:
        self.assertEqual([], self.messages())

    def test_missing_root_manifest_fails(self) -> None:
        (self.root / "Cargo.toml").unlink()
        self.assertTrue(any("root workspace manifest is missing" in message for message in self.messages()))

    def test_root_package_without_workspace_is_rejected(self) -> None:
        self.write("Cargo.toml", '[package]\nname = "root"\n')
        self.assertTrue(any("must declare [workspace]" in message for message in self.messages()))

    def test_new_independent_workspace_without_lock_cannot_escape(self) -> None:
        self.write("experiments/new-proof/Cargo.toml", "[workspace]\n")
        self.assertEqual(
            ["experiments/new-proof/Cargo.lock: independent workspace lockfile is missing"],
            self.messages(),
        )

    def test_new_independent_workspace_with_lock_is_discovered(self) -> None:
        self.write("experiments/new-proof/Cargo.toml", "[workspace]\n")
        self.write("experiments/new-proof/Cargo.lock", "version = 4\n")
        self.assertEqual([], self.messages())

    def test_nested_member_is_not_an_independent_workspace(self) -> None:
        self.write("experiments/flawless-proof/Cargo.toml", '[workspace]\nmembers = ["workflows/m08"]\n')
        self.write("experiments/flawless-proof/Cargo.lock", "version = 4\n")
        self.write("experiments/flawless-proof/workflows/m08/Cargo.toml", '[package]\nname = "m08"\n')
        self.assertEqual([], self.messages())

    def test_orphan_lock_without_manifest_is_rejected(self) -> None:
        self.write("experiments/orphan/Cargo.lock", "version = 4\n")
        self.assertEqual(
            ["experiments/orphan/Cargo.lock: orphan Cargo.lock has no adjacent [workspace] manifest"],
            self.messages(),
        )

    def test_member_lock_is_rejected(self) -> None:
        self.write("crates/member/Cargo.toml", '[package]\nname = "member"\n')
        self.write("crates/member/Cargo.lock", "version = 4\n")
        self.assertTrue(any("orphan Cargo.lock" in message for message in self.messages()))

    def test_malformed_manifest_fails_even_without_lock(self) -> None:
        self.write("experiments/broken/Cargo.toml", "[workspace\n")
        self.assertTrue(any("cannot read Cargo manifest" in message for message in self.messages()))

    def test_parser_diagnostics_do_not_echo_untrusted_toml_keys(self) -> None:
        sentinel = "private-test-key-do-not-print"
        self.write("Cargo.toml", f'[workspace]\n[workspace."{sentinel}"]\n[workspace."{sentinel}"]\n')
        messages = self.messages()
        self.assertTrue(any("cannot read Cargo manifest" in message for message in messages))
        self.assertNotIn(sentinel, "\n".join(messages))

    def test_read_is_bounded_even_if_manifest_grows_after_discovery(self) -> None:
        payload = io.BytesIO(b"x" * (verifier.SOURCE_MANIFEST_MAX_BYTES + 2))
        payload.read = Mock(wraps=payload.read)
        with patch.object(Path, "open", return_value=payload):
            messages = self.messages()
        payload.read.assert_called_once_with(verifier.SOURCE_MANIFEST_MAX_BYTES + 1)
        self.assertTrue(any("exceeds the source size limit" in message for message in messages))

    def test_workspace_value_must_be_a_table(self) -> None:
        self.write("experiments/broken/Cargo.toml", 'workspace = "not a table"\n')
        self.assertTrue(any("workspace must be a TOML table" in message for message in self.messages()))

    def test_non_utf8_manifest_fails(self) -> None:
        manifest = self.write("experiments/broken/Cargo.toml", "")
        manifest.write_bytes(b"\xff")
        self.assertTrue(any("cannot read Cargo manifest" in message for message in self.messages()))

    def test_cache_and_generated_trees_are_pruned(self) -> None:
        for directory in verifier.SOURCE_SCAN_IGNORED_PARTS:
            self.write(f"{directory}/dependency/Cargo.toml", "[workspace\n")
            self.write(f"experiments/proof/{directory}/Cargo.lock", "version = 4\n")
        self.assertEqual([], self.messages())

    def test_source_symlink_is_rejected_without_following_it(self) -> None:
        with tempfile.TemporaryDirectory() as external_directory:
            external = Path(external_directory)
            (external / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            link = self.root / "linked-source"
            try:
                link.symlink_to(external, target_is_directory=True)
            except OSError as error:
                self.skipTest(f"filesystem does not permit symlink creation: {error}")
            self.assertEqual(
                ["linked-source: source symlinks and reparse points are not allowed"],
                self.messages(),
            )

    def test_symlinks_and_windows_reparse_points_fail_without_privileges(self) -> None:
        for mode, attributes in [
            (stat.S_IFLNK, 0),
            (stat.S_IFDIR, stat.FILE_ATTRIBUTE_REPARSE_POINT),
            (stat.S_IFREG, stat.FILE_ATTRIBUTE_REPARSE_POINT),
        ]:
            with self.subTest(mode=mode, attributes=attributes):
                inspect = Mock(
                    return_value=SimpleNamespace(
                        st_mode=mode,
                        st_file_attributes=attributes,
                    )
                )
                entry = SimpleNamespace(name="unsafe-source", stat=inspect)
                with patch.object(verifier.os, "scandir", return_value=nullcontext([entry])):
                    messages = self.messages()
                inspect.assert_called_once_with(follow_symlinks=False)
                self.assertIn(
                    "unsafe-source: source symlinks and reparse points are not allowed",
                    messages,
                )

    def test_unreadable_source_directory_fails_closed(self) -> None:
        with patch.object(verifier.os, "scandir", side_effect=PermissionError("denied")):
            self.assertTrue(any("cannot inspect source directory" in message for message in self.messages()))

    def test_default_policy_verification_includes_workspace_census(self) -> None:
        self.write("experiments/new-proof/Cargo.toml", "[workspace]\n")
        with (
            patch.object(verifier, "load_tool_policy", return_value=(None, [])),
            patch.object(verifier, "load_artifact_specs", return_value=([], [])),
            patch.object(verifier, "check_deny_policy", return_value=[]),
        ):
            problems = verifier.verify(self.root)
        self.assertEqual(
            ["experiments/new-proof/Cargo.lock: independent workspace lockfile is missing"],
            [problem.render() for problem in problems],
        )

    def test_oversized_manifest_fails_closed(self) -> None:
        with patch.object(verifier, "SOURCE_MANIFEST_MAX_BYTES", 1):
            self.assertTrue(any("exceeds the source size limit" in message for message in self.messages()))

    def test_source_entry_limit_fails_closed(self) -> None:
        with patch.object(verifier, "SOURCE_SCAN_MAX_ENTRIES", 0):
            self.assertTrue(any("exceeded its entry limit" in message for message in self.messages()))

    def test_source_depth_limit_fails_closed(self) -> None:
        self.write("deep/source/Cargo.toml", "[workspace]\n")
        with patch.object(verifier, "SOURCE_SCAN_MAX_DEPTH", 0):
            self.assertTrue(any("exceeded its depth limit" in message for message in self.messages()))


if __name__ == "__main__":
    unittest.main()
