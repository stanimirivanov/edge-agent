from __future__ import annotations

from copy import deepcopy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tomllib
import unittest


ROOT = Path(__file__).parents[1]
SPEC = importlib.util.spec_from_file_location(
    "verify_sqlx_query_policy", ROOT / "scripts" / "verify_sqlx_query_policy.py"
)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("could not load SQLx policy verifier")
policy = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = policy
SPEC.loader.exec_module(policy)


class VerifySqlxQueryPolicyTests(unittest.TestCase):
    def test_fixture_uses_workspace_sqlx_version_and_features(self) -> None:
        original = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
        fixture = tomllib.loads(policy.fixture_manifest(ROOT))
        self.assertEqual(original["workspace"]["dependencies"]["sqlx"], fixture["dependencies"]["sqlx"])

    def test_removed_duplicate_or_added_restriction_is_rejected(self) -> None:
        actual = tomllib.loads((ROOT / "crates/outbox-postgres/clippy.toml").read_text(encoding="utf-8"))
        policy.validate_policy(actual)
        for field in ["disallowed-methods", "disallowed-macros"]:
            for mutation in ["remove", "duplicate", "add"]:
                with self.subTest(field=field, mutation=mutation):
                    changed = deepcopy(actual)
                    if mutation == "remove":
                        changed[field].pop()
                    elif mutation == "duplicate":
                        changed[field].append(changed[field][0])
                    else:
                        changed[field].append({"path": "sqlx::not_reviewed"})
                    with self.assertRaises(RuntimeError):
                        policy.validate_policy(changed)

    def test_only_expected_lint_failure_establishes_a_rejection(self) -> None:
        def output(codes: list[str | None], returncode: int = 1) -> subprocess.CompletedProcess:
            messages = [{"reason": "compiler-message", "message": {
                "level": "error", "message": "fixture", "code": {"code": code} if code else None,
            }} for code in codes]
            return subprocess.CompletedProcess([], returncode, "\n".join(map(json.dumps, messages)), "")

        policy.require_rejection(output(["clippy::disallowed_methods"]), "disallowed_methods", "fixture")
        for result in [output([], 0), output([]), output([None]), output(["E0308"]),
                       output(["clippy::disallowed_methods"], 101),
                       output(["clippy::disallowed_methods", "E0308"])]:
            with self.subTest(result=result), self.assertRaises(RuntimeError):
                policy.require_rejection(result, "disallowed_methods", "fixture")

    def test_direct_compiler_json_is_checked_without_cargo_wrapping(self) -> None:
        diagnostic = {"$message_type": "diagnostic", "level": "error",
                      "message": "fixture", "code": {"code": "clippy::disallowed_methods"}}
        result = subprocess.CompletedProcess([], 1, "", json.dumps(diagnostic))
        policy.require_rejection(result, "disallowed_methods", "direct compiler")
        self.assertEqual([diagnostic], policy.compiler_errors(result))

    def test_only_span_free_compiler_abort_summary_is_ignored(self) -> None:
        lint = {"level": "error", "message": "fixture", "code": {"code": "clippy::disallowed_methods"}}
        summary = {"level": "error", "code": None, "spans": [], "message": "aborting due to 1 previous error"}
        result = subprocess.CompletedProcess([], 1, "", "\n".join(map(json.dumps, [lint, summary])))
        policy.require_rejection(result, "disallowed_methods", "with summary")
        for extra in [summary, {**summary, "spans": [{"file_name": "fixture.rs"}]},
                      {**summary, "message": "no cached SQLx data"}]:
            records = [extra] if extra == summary else [lint, extra]
            result = subprocess.CompletedProcess([], 1, "", "\n".join(map(json.dumps, records)))
            with self.subTest(extra=extra), self.assertRaises(RuntimeError):
                policy.require_rejection(result, "disallowed_methods", "must fail")

    def test_sdk_artifact_comes_from_successful_unambiguous_cargo_output(self) -> None:
        record = {"reason": "compiler-artifact", "target": {"name": "sqlx", "kind": ["lib"]},
                  "filenames": ["sdk/libsqlx.rmeta", "sdk/libsqlx.rlib"]}
        result = subprocess.CompletedProcess([], 0, json.dumps(record), "")
        self.assertEqual(Path("sdk/libsqlx.rmeta"), policy.sqlx_artifact(result))
        for changed in [subprocess.CompletedProcess([], 1, "", "build failed"),
                        subprocess.CompletedProcess([], 0, "", ""),
                        subprocess.CompletedProcess([], 0, json.dumps({
                            **record, "filenames": ["sdk/one.rmeta", "sdk/two.rmeta"]}), "")]:
            with self.subTest(result=changed), self.assertRaises(RuntimeError):
                policy.sqlx_artifact(changed)


if __name__ == "__main__":
    unittest.main()
