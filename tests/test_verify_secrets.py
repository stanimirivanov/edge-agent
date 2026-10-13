from __future__ import annotations

import hashlib
from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import subprocess
import re
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile


ROOT = Path(__file__).parents[1]
SCRIPTS = ROOT / "scripts"
sys.path.insert(0, str(SCRIPTS))
import install_gitleaks
import verify_secrets


class SecretVerificationTests(unittest.TestCase):
    def copy_policy(self, root: Path) -> None:
        (root / "supply-chain").mkdir()
        for relative in ("supply-chain/security-tools.toml", ".gitleaks.toml"):
            (root / relative).write_bytes((ROOT / relative).read_bytes())

    def test_checked_in_detector_policy_is_valid(self) -> None:
        policy = verify_secrets.validate_policy(ROOT)
        self.assertEqual("8.30.1", policy.version)
        self.assertEqual({"linux_x64", "windows_x64"}, set(policy.archives))

    def test_policy_rejects_configuration_suppression_and_shape_drift(self) -> None:
        original = (ROOT / ".gitleaks.toml").read_text(encoding="utf-8")
        changes = (
            original.replace('condition = "AND"', 'condition = "OR"'),
            original.replace("useDefault = true", "useDefault = false"),
            original.replace("deploy[/\\\\]local[/\\\\]\\.env\\.example$", ".*"),
            original.replace("^edgeagent-local-(?:postgres|access|secret)$", "edgeagent"),
            original.replace("[extend]", '[extend]\ndisabledRules = ["github-pat"]'),
            original + '\n[[allowlists]]\npaths = ["tests/.*"]\n',
            original + '\n[[rules]]\nid = "disabled-boundary"\nregex = "does-not-matter"\n',
        )
        for source in changes:
            with self.subTest(source_index=changes.index(source)), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.copy_policy(root)
                (root / ".gitleaks.toml").write_text(source, encoding="utf-8")
                with self.assertRaisesRegex(install_gitleaks.SecurityToolError, "policy drift"):
                    verify_secrets.validate_policy(root)

    def test_policy_rejects_malformed_tool_identity_and_extra_fields(self) -> None:
        original = (ROOT / "supply-chain/security-tools.toml").read_text(encoding="utf-8")
        for source in (
            original.replace('version = "8.30.1"', 'version = "latest"'),
            original.replace('license = "MIT"', 'license = "unknown"'),
            original.replace("https://github.com/gitleaks/gitleaks/releases/tag/", "https://example.invalid/"),
            original.replace('archive = "gitleaks_8.30.1_windows_x64.zip"', 'archive = "../../unsafe.zip"'),
            original.replace('executable = "gitleaks.exe"', 'executable = "../gitleaks.exe"'),
            original.replace("d29144deff3a68aa93ced33dddf84b7fdc26070add4aa0f4513094c8332afc4e", "not-a-digest"),
            original + '\nextra = "unreviewed"\n',
        ):
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.copy_policy(root)
                (root / "supply-chain/security-tools.toml").write_text(source, encoding="utf-8")
                with self.assertRaises(install_gitleaks.SecurityToolError):
                    verify_secrets.validate_policy(root)

    def test_fingerprint_ignore_file_is_not_an_accepted_escape_hatch(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.copy_policy(root)
            (root / ".gitleaksignore").write_text("suppressed-finding", encoding="utf-8")
            with self.assertRaisesRegex(install_gitleaks.SecurityToolError, "fingerprint"):
                verify_secrets.validate_policy(root)

    def test_ambient_detector_configuration_is_removed(self) -> None:
        with patch.dict("os.environ", {"GITLEAKS_CONFIG": "unsafe", "GITLEAKS_CONFIG_TOML": "unsafe", "gitleaks_extra": "unsafe", "GIT_NO_REPLACE_OBJECTS": "0", "GIT_DIR": "other-repository", "GIT_WORK_TREE": "other-source", "GIT_INDEX_FILE": "empty-index", "GIT_CONFIG_COUNT": "1", "GIT_CONFIG_PARAMETERS": "hide-source", "GIT_CONFIG_KEY_0": "safe.directory", "GIT_CONFIG_VALUE_0": "*"}):
            environment = verify_secrets.sanitized_environment()
        self.assertFalse(any(key.upper().startswith("GITLEAKS_") for key in environment))
        self.assertEqual({"GIT_NO_REPLACE_OBJECTS"}, {key for key in environment if key.upper().startswith("GIT_")})
        self.assertEqual("1", environment["GIT_NO_REPLACE_OBJECTS"])

    def test_only_exact_controlled_source_trust_is_reinjected(self) -> None:
        with patch.dict("os.environ", {"GIT_CONFIG_COUNT": "7", "GIT_CONFIG_KEY_0": "safe.directory", "GIT_CONFIG_VALUE_0": "*", "GIT_CONFIG_VALUE_6": "untrusted", "GIT_DIR": "other-checkout"}):
            environment = verify_secrets.sanitized_environment(ROOT)
        git_keys = {key: value for key, value in environment.items() if key.startswith("GIT_")}
        self.assertEqual({"GIT_NO_REPLACE_OBJECTS": "1", "GIT_CONFIG_COUNT": "1", "GIT_CONFIG_KEY_0": "safe.directory", "GIT_CONFIG_VALUE_0": str(ROOT.resolve())}, git_keys)

    def test_fixture_paths_are_root_anchored_on_both_platforms(self) -> None:
        for expression, relative in ((verify_secrets.FIXTURE_PATH_REGEX, "deploy/local/.env.example"), (verify_secrets.CANCELLATION_PATH_REGEX, "crates/outbox-relay/tests/cancellation.rs")):
            for candidate in (relative, "./" + relative, relative.replace("/", "\\"), ".\\" + relative.replace("/", "\\")):
                self.assertIsNotNone(re.fullmatch(expression, candidate))
            for candidate in ("nested/" + relative, "../" + relative, "/" + relative, "C:/source/" + relative):
                self.assertIsNone(re.fullmatch(expression, candidate))

    def test_version_protocol_accepts_only_exact_successful_release(self) -> None:
        policy = install_gitleaks.load_policy(ROOT)
        for stdout in (b"8.30.1\n", b"8.30.1\r\n"):
            with patch.object(verify_secrets, "captured_command", return_value=subprocess.CompletedProcess([], 0, stdout=stdout)) as command:
                verify_secrets.require_version("gitleaks", policy, ROOT)
                self.assertEqual(["gitleaks", "version"], command.call_args.args[0])
        for status, stdout in ((1, b"8.30.1"), (0, b"8.30.0"), (0, b"8.30.1-dev"), (0, b"gitleaks 8.30.1"), (0, b"\xff")):
            with patch.object(verify_secrets, "captured_command", return_value=subprocess.CompletedProcess([], status, stdout=stdout)):
                with self.assertRaisesRegex(install_gitleaks.SecurityToolError, "pinned Gitleaks"):
                    verify_secrets.require_version("gitleaks", policy, ROOT)

    def report(self, **overrides: object) -> dict:
        return {"RuleID": "github-pat", "File": "private/path.txt", "StartLine": 2, "Commit": "a" * 40, "Secret": "REDACTED", "Match": "REDACTED", "Message": "private commit message", "Author": "private author", **overrides}

    def test_report_projects_only_redacted_metadata_and_hashes_filenames(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.json"
            path.write_text(json.dumps([self.report()]), encoding="utf-8")
            findings = verify_secrets.parse_report(path)
        output = findings[0].describe()
        self.assertIn("github-pat", output)
        self.assertIn("line=2", output)
        self.assertIn("commit=aaaaaaaaaaaa", output)
        for private in ("private/path.txt", "private commit message", "private author", "REDACTED"):
            self.assertNotIn(private, output)

    def test_report_rejects_unredacted_or_malformed_evidence(self) -> None:
        for document in ({}, [None], [self.report(Secret="synthetic-private")], [self.report(RuleID="bad\nrule")], [self.report(File=None)], [self.report(StartLine=True)], [self.report(StartLine=0)], [self.report(Commit="unsafe")]):
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "report.json"
                path.write_text(json.dumps(document), encoding="utf-8")
                with self.assertRaises(install_gitleaks.SecurityToolError):
                    verify_secrets.parse_report(path)

    def test_report_file_identity_does_not_depend_on_snapshot_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.json"
            ids = []
            for source in (Path(directory) / "one", Path(directory) / "two"):
                path.write_text(json.dumps([self.report(File=str(source / "src/probe.rs"))]), encoding="utf-8")
                ids.append(verify_secrets.parse_report(path, source)[0].file_id)
        self.assertEqual(ids[0], ids[1])

    def test_scan_preserves_redaction_and_history_flags(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory)
            def execute(arguments: list[str], **kwargs: object) -> subprocess.CompletedProcess:
                (temporary / "report.json").write_text("[]", encoding="utf-8")
                self.assertIn("--redact=100", arguments)
                self.assertIn("--ignore-gitleaks-allow", arguments)
                self.assertIn("--log-opts=--all --full-history --diff-merges=separate --no-ext-diff --no-textconv", arguments)
                self.assertIn(str(ROOT / ".gitleaks.toml"), arguments)
                self.assertNotIn("--baseline-path", arguments)
                self.assertEqual(".", arguments[-1])
                self.assertEqual(ROOT, kwargs["cwd"])
                self.assertEqual(subprocess.DEVNULL, kwargs["stderr"])
                self.assertEqual(subprocess.DEVNULL, kwargs["stdout"])
                return subprocess.CompletedProcess(arguments, 0)
            with patch.object(verify_secrets.subprocess, "run", side_effect=execute):
                self.assertEqual([], verify_secrets.scan("gitleaks", ROOT, ROOT, "git", temporary))

    def test_scan_rejects_partial_errors_stale_reports_and_status_disagreement(self) -> None:
        for status, report in ((1, []), (0, [self.report()]), (23, []), (0, None)):
            with self.subTest(status=status, report=report), tempfile.TemporaryDirectory() as directory:
                temporary = Path(directory)
                (temporary / "report.json").write_text("[]", encoding="utf-8")
                def execute(arguments: list[str], **kwargs: object) -> subprocess.CompletedProcess:
                    if report is not None:
                        (temporary / "report.json").write_text(json.dumps(report), encoding="utf-8")
                    return subprocess.CompletedProcess(arguments, status)
                with patch.object(verify_secrets.subprocess, "run", side_effect=execute):
                    with self.assertRaises(install_gitleaks.SecurityToolError):
                        verify_secrets.scan("gitleaks", ROOT, ROOT, "dir", temporary)

    def test_snapshot_enumeration_includes_tracked_and_nonignored_new_paths(self) -> None:
        with patch.object(verify_secrets, "captured_command", return_value=subprocess.CompletedProcess([], 0, stdout=b"src/new.rs\0src/old.rs\0src/new.rs\0")) as command:
            paths = verify_secrets.source_paths(ROOT)
        self.assertEqual(["src/new.rs", "src/old.rs"], paths)
        self.assertIn("--cached", command.call_args.args[0])
        self.assertIn("--others", command.call_args.args[0])
        self.assertIn("--exclude-standard", command.call_args.args[0])

    def test_snapshot_enumeration_rejects_empty_non_utf8_unsafe_and_ignore_paths(self) -> None:
        for stdout in (b"", b"missing-terminator", b"\xff\0", b"../escape\0", b"/absolute\0", b"C:/escape\0", b".git/config\0", b"src\\unsafe\0", b"nested/.gitleaksignore\0"):
            with patch.object(verify_secrets, "captured_command", return_value=subprocess.CompletedProcess([], 0, stdout=stdout)):
                with self.assertRaises(install_gitleaks.SecurityToolError):
                    verify_secrets.source_paths(ROOT)

    def test_snapshot_preserves_current_source_and_omits_deleted_tracked_files(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory)
            root, target = temporary / "root", temporary / "target"
            root.mkdir()
            target.mkdir()
            (root / "new.txt").write_text("source", encoding="utf-8")
            copied = verify_secrets.snapshot(root, target, ["new.txt", "deleted.txt"])
            self.assertEqual(1, copied)
            self.assertEqual("source", (target / "new.txt").read_text(encoding="utf-8"))

    def test_snapshot_rejects_non_files_and_size_limit(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory)
            root, target = temporary / "root", temporary / "target"
            root.mkdir()
            target.mkdir()
            (root / "directory").mkdir()
            (root / "large.txt").write_text("oversized", encoding="utf-8")
            with self.assertRaisesRegex(install_gitleaks.SecurityToolError, "non-file"):
                verify_secrets.snapshot(root, target, ["directory"])
            with patch.object(verify_secrets, "MAX_SNAPSHOT_BYTES", 2):
                with self.assertRaisesRegex(install_gitleaks.SecurityToolError, "limit"):
                    verify_secrets.snapshot(root, target, ["large.txt"])

    def test_snapshot_rejects_symlinks_without_platform_privileges(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory)
            root, target = temporary / "root", temporary / "target"
            root.mkdir()
            target.mkdir()
            (root / "file.txt").write_text("source", encoding="utf-8")
            with patch.object(Path, "is_symlink", return_value=True):
                with self.assertRaisesRegex(install_gitleaks.SecurityToolError, "symlink"):
                    verify_secrets.snapshot(root, target, ["file.txt"])

    def test_complete_history_rejects_shallow_empty_and_failed_git(self) -> None:
        valid = [b"false\n", b"a" * 40 + b"\n", b"2\n"]
        for position, replacement in ((0, b"true\n"), (1, b""), (2, b"0\n")):
            responses = [subprocess.CompletedProcess([], 0, stdout=str(ROOT).encode("utf-8")), *[subprocess.CompletedProcess([], 0, stdout=value) for value in valid]]
            responses[position + 1].stdout = replacement
            with patch.object(verify_secrets, "captured_command", side_effect=responses):
                with self.assertRaisesRegex(install_gitleaks.SecurityToolError, "complete Git history"):
                    verify_secrets.require_complete_history(ROOT)
        with patch.object(verify_secrets, "captured_command", return_value=subprocess.CompletedProcess([], 1, stdout=b"false\n")):
            with self.assertRaises(install_gitleaks.SecurityToolError):
                verify_secrets.require_complete_history(ROOT)

    def test_complete_history_accepts_valid_nonempty_repository(self) -> None:
        responses = [subprocess.CompletedProcess([], 0, stdout=value) for value in (str(ROOT).encode("utf-8"), b"false\n", b"a" * 40 + b"\n", b"2\n")]
        with patch.object(verify_secrets, "captured_command", side_effect=responses):
            verify_secrets.require_complete_history(ROOT)

    def test_complete_history_rejects_nested_or_redirected_repository(self) -> None:
        with patch.object(verify_secrets, "captured_command", return_value=subprocess.CompletedProcess([], 0, stdout=str(ROOT.parent).encode("utf-8"))):
            with self.assertRaisesRegex(install_gitleaks.SecurityToolError, "intended repository root"):
                verify_secrets.require_complete_history(ROOT)

    def test_actual_self_test_executes_every_owned_case_and_history_probe(self) -> None:
        expected_ids = (
            "clean", "built-in-rule", "canonical-fixture", "changed-fixture",
            "fixture-outside-allowlist", "nested-fixture-outside-allowlist", "inline-suppression",
            "idempotency-fixture", "changed-idempotency-fixture", "idempotency-fixture-outside-allowlist",
            "nested-idempotency-fixture-outside-allowlist", "removed-secret-history",
            "removed-merge-only-secret-history",
        )
        self.assertEqual(expected_ids, verify_secrets.SELF_TEST_CASE_IDS)
        rules = iter((None, "github-pat", None, "edgeagent-config-credential", "edgeagent-config-credential", "edgeagent-config-credential", "github-pat", None, "generic-api-key", "generic-api-key", "generic-api-key", None, "github-pat", None, "github-pat"))
        def scan(executable: str, root: Path, source: Path, mode: str, temporary: Path) -> list:
            (temporary / "report.json").write_text("[]", encoding="utf-8")
            rule = next(rules)
            return [] if rule is None else [verify_secrets.Finding(rule, "safe-file", 1, "a" * 12)]
        def command(arguments: list[str], root: Path) -> subprocess.CompletedProcess:
            if "--show-toplevel" in arguments:
                stdout = str(root.resolve()).encode("utf-8")
            elif "--is-shallow-repository" in arguments:
                stdout = b"false"
            elif "rev-parse" in arguments:
                stdout = b"a" * 40
            elif "rev-list" in arguments:
                stdout = b"2"
            else:
                stdout = b""
            return subprocess.CompletedProcess(arguments, 0, stdout=stdout)
        stdout = io.StringIO()
        with patch.object(verify_secrets, "scan", side_effect=scan) as detector, patch.object(verify_secrets, "captured_command", side_effect=command), redirect_stdout(stdout):
            count = verify_secrets.self_test("mock-detector", ROOT)
        self.assertEqual(13, count)
        self.assertEqual(15, detector.call_count)
        self.assertEqual(2, sum(call.args[3] == "git" for call in detector.call_args_list))
        prefix = "secret detector self-proof passed: "
        self.assertEqual(list(expected_ids), [line.removeprefix(prefix) for line in stdout.getvalue().splitlines()])


class GitleaksInstallerTests(unittest.TestCase):
    def archive(self, content: bytes = b"synthetic executable", name: str = "gitleaks.exe", extra: bool = False) -> bytes:
        output = io.BytesIO()
        with zipfile.ZipFile(output, "w") as source:
            source.writestr(name, content)
            if extra:
                source.writestr("../../escape", "not extracted")
        return output.getvalue()

    def test_authentication_precedes_archive_parsing(self) -> None:
        with patch.object(install_gitleaks.zipfile, "ZipFile") as parse:
            with self.assertRaisesRegex(install_gitleaks.SecurityToolError, "checksum"):
                install_gitleaks.authenticated_binary(b"not an archive", install_gitleaks.Archive("test.zip", "0" * 64, "gitleaks.exe"))
            parse.assert_not_called()

    def test_archive_extracts_only_exact_executable_no_traversal(self) -> None:
        payload = self.archive(extra=True)
        binary = install_gitleaks.authenticated_binary(payload, install_gitleaks.Archive("test.zip", hashlib.sha256(payload).hexdigest(), "gitleaks.exe"))
        self.assertEqual(b"synthetic executable", binary)

    def test_archive_rejects_missing_empty_symlink_duplicate_and_oversized_member(self) -> None:
        payloads = [self.archive(name="../gitleaks.exe"), self.archive(content=b"")]
        duplicate = io.BytesIO()
        with zipfile.ZipFile(duplicate, "w") as source:
            source.writestr("gitleaks.exe", "one")
            with self.assertWarns(UserWarning):
                source.writestr("gitleaks.exe", "two")
        payloads.append(duplicate.getvalue())
        symlink = io.BytesIO()
        with zipfile.ZipFile(symlink, "w") as source:
            entry = zipfile.ZipInfo("gitleaks.exe")
            entry.external_attr = 0o120777 << 16
            source.writestr(entry, "../escape")
        payloads.append(symlink.getvalue())
        for payload in payloads:
            with self.assertRaises(install_gitleaks.SecurityToolError):
                install_gitleaks.authenticated_binary(payload, install_gitleaks.Archive("test.zip", hashlib.sha256(payload).hexdigest(), "gitleaks.exe"))
        payload = self.archive()
        with patch.object(install_gitleaks, "MAX_BINARY_BYTES", 2):
            with self.assertRaises(install_gitleaks.SecurityToolError):
                install_gitleaks.authenticated_binary(payload, install_gitleaks.Archive("test.zip", hashlib.sha256(payload).hexdigest(), "gitleaks.exe"))

    def test_tar_archive_extracts_exact_regular_binary(self) -> None:
        output = io.BytesIO()
        content = b"synthetic Linux binary"
        with tarfile.open(fileobj=output, mode="w:gz") as source:
            member = tarfile.TarInfo("gitleaks")
            member.size = len(content)
            source.addfile(member, io.BytesIO(content))
        payload = output.getvalue()
        self.assertEqual(content, install_gitleaks.authenticated_binary(payload, install_gitleaks.Archive("test.tar.gz", hashlib.sha256(payload).hexdigest(), "gitleaks")))

    def test_supported_host_resolution_is_explicit(self) -> None:
        for system, machine, expected in (("Linux", "x86_64", "linux_x64"), ("Windows", "AMD64", "windows_x64")):
            with patch.object(install_gitleaks.platform, "system", return_value=system), patch.object(install_gitleaks.platform, "machine", return_value=machine):
                self.assertEqual(expected, install_gitleaks.host_target())
        for system, machine in (("Darwin", "x86_64"), ("Linux", "aarch64")):
            with patch.object(install_gitleaks.platform, "system", return_value=system), patch.object(install_gitleaks.platform, "machine", return_value=machine):
                with self.assertRaises(install_gitleaks.SecurityToolError):
                    install_gitleaks.host_target()


if __name__ == "__main__":
    unittest.main()
