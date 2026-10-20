#!/usr/bin/env python3
"""Exercise checked-SQL adapter Clippy policies with offline compiler fixtures."""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[1]
RUNTIME_APIS = frozenset({
    "query", "query_as", "query_scalar", "query_with", "query_as_with",
    "query_scalar_with", "__query_with_result", "__query_scalar_with_result", "raw_sql",
})
UNCHECKED_MACROS = frozenset({
    "query_unchecked", "query_as_unchecked", "query_file_unchecked",
    "query_file_as_unchecked", "query_scalar_unchecked", "query_file_scalar_unchecked",
})


def fixture_manifest(root: Path) -> str:
    """Use the reviewed SQLx version/features rather than a second tool pin."""
    dependency = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))[
        "workspace"
    ]["dependencies"]["sqlx"]
    fields = ", ".join(f"{key} = {json.dumps(value)}" for key, value in dependency.items())
    return (
        '[package]\nname = "edgeagent-sqlx-query-policy-fixture"\n'
        'version = "0.0.0"\nedition = "2024"\n[workspace]\n'
        f"[dependencies]\nsqlx = {{ {fields} }}\n"
    )


def validate_policy(policy: dict) -> None:
    """Removing a restriction cannot silently remove its negative fixture."""
    for field, required in [("disallowed-methods", RUNTIME_APIS),
                            ("disallowed-macros", UNCHECKED_MACROS)]:
        entries = policy[field]
        actual = {entry["path"] for entry in entries}
        if actual != {f"sqlx::{name}" for name in required} or len(actual) != len(entries):
            raise RuntimeError(f"SQLx adapter {field} differs from the compiler-fixture inventory")


def compiler_errors(result: subprocess.CompletedProcess[str]) -> list[dict]:
    """Ignore artifact chatter; infrastructure/compiler failures cannot pass."""
    errors = []
    for line in (result.stdout + "\n" + result.stderr).splitlines():
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if record.get("reason") == "compiler-message":
            record = record["message"]
        # rustc emits a second, span-free error summarizing an already-reported
        # failure. Ignore only that exact summary, never an uncoded SQLx error.
        if (record.get("code") is None and not record.get("spans")
                and re.fullmatch(r"aborting due to \d+ previous errors?", record.get("message", ""))):
            continue
        if record.get("level") == "error":
            errors.append(record)
    return errors


def diagnostics(result: subprocess.CompletedProcess[str]) -> str:
    errors = compiler_errors(result)
    return "\n".join(error.get("rendered") or error["message"] for error in errors) if errors else result.stderr


def require_rejection(result: subprocess.CompletedProcess[str], lint: str, context: str) -> None:
    errors = compiler_errors(result)
    if (result.returncode != 1 or not errors
            or any((error.get("code") or {}).get("code") != f"clippy::{lint}" for error in errors)):
        raise RuntimeError(f"policy did not reject {context}:\n" + diagnostics(result))


def sqlx_artifact(result: subprocess.CompletedProcess[str]) -> Path:
    """Resolve the SDK from Cargo's actual locked build, not filename guessing."""
    if result.returncode:
        raise RuntimeError("cannot build the locked SQLx fixture dependency:\n" + diagnostics(result))
    paths = set()
    for line in result.stdout.splitlines():
        record = json.loads(line)
        if (record.get("reason") == "compiler-artifact"
                and record["target"]["name"] == "sqlx"
                and "lib" in record["target"]["kind"]):
            paths.update(Path(path) for path in record["filenames"] if path.endswith(".rmeta"))
    if len(paths) != 1:
        raise RuntimeError("expected exactly one locked SQLx metadata artifact")
    return paths.pop()


def negative_source(path: str, alias: bool, identity: str) -> str:
    imported = "use " + path + " as bypass;\n" if alias else ""
    name = "bypass" if alias else path
    if path.rsplit("::", 1)[-1] in UNCHECKED_MACROS:
        sql = '"queries/identity.sql"' if "file" in path else 'r#"' + identity + '"#'
        row = "Identity, " if "_as_" in path else ""
        return imported + "pub struct Identity { pub identical: bool }\n" + (
            f'pub fn rejected() {{ let _query = {name}!({row}{sql}, '
            '"source", "id", "type", "subject", b"{}"); }\n'
        )
    if "scalar" in path:
        arguments = "::<sqlx::Postgres, i32>"
    elif "query_as" in path:
        arguments = "::<sqlx::Postgres, (i32,)>"
    elif "raw_sql" in path:
        arguments = ""
    else:
        arguments = "::<sqlx::Postgres>"
    if path.endswith(("_with", "_with_result")):
        arguments = arguments[:-1] + ", sqlx::postgres::PgArguments>"
    extra = (", Default::default()" if path.endswith("_with") else
             ", Ok(Default::default())" if path.endswith("_with_result") else "")
    return imported + f'pub fn rejected() {{ let _query = {name}{arguments}("SELECT 1"{extra}); }}\n'


def verify(root: Path = ROOT) -> None:
    adapter = root / "crates" / "outbox-postgres"
    # Both capabilities must retain the exact independently enumerated bans.
    # Compiler fixtures below prove those paths, including renamed imports;
    # a removed simulation restriction cannot silently keep that proof green.
    for package in ("outbox-postgres", "simulation-postgres"):
        policy = tomllib.loads((root / "crates" / package / "clippy.toml").read_text(encoding="utf-8"))
        validate_policy(policy)
    build = subprocess.run(
        ["cargo", "check", "--locked", "--offline", "--workspace", "--all-targets",
         "--message-format=json"],
        cwd=root, capture_output=True, text=True, check=False,
    )
    sdk = sqlx_artifact(build)
    toolchain = tomllib.loads((root / "rust-toolchain.toml").read_text(encoding="utf-8"))["toolchain"]["channel"]
    # Isolated manifests/locks never edit the repository dependency graph.
    # Compile fixtures directly against Cargo's SDK artifact. A second Cargo
    # graph/cache would rebuild SQLx and thrash check/test fingerprints.
    with tempfile.TemporaryDirectory(prefix="edgeagent-sqlx-policy-") as directory:
        fixture = Path(directory)
        (fixture / "src").mkdir()
        (fixture / "Cargo.toml").write_text(fixture_manifest(root), encoding="utf-8", newline="\n")
        shutil.copyfile(root / "Cargo.lock", fixture / "Cargo.lock")
        shutil.copyfile(adapter / "clippy.toml", fixture / "clippy.toml")
        shutil.copytree(adapter / "queries", fixture / "queries")
        # Reuse an existing one-column describe result for every unchecked
        # macro variant; absent metadata fails closed, not as a policy pass.
        identity = next(
            data["query"] for path in sorted((root / ".sqlx").glob("query-*.json"))
            if 'AS "identical!"' in (data := json.loads(path.read_text(encoding="utf-8")))["query"]
        )
        (fixture / "queries" / "identity.sql").write_text(identity, encoding="utf-8", newline="\n")
        environment = os.environ.copy()
        environment.pop("DATABASE_URL", None)
        environment.pop("CLIPPY_CONF_DIR", None)
        environment.pop("CLIPPY_ARGS", None)
        environment["SQLX_OFFLINE"] = "true"
        environment["SQLX_OFFLINE_DIR"] = str(root / ".sqlx")
        environment["CARGO_MANIFEST_DIR"] = str(fixture)
        environment["CARGO_NET_OFFLINE"] = "true"
        (fixture / "build").mkdir()

        def compile_source(source: str) -> subprocess.CompletedProcess[str]:
            (fixture / "src" / "lib.rs").write_text(
                "#![deny(clippy::disallowed_methods, clippy::disallowed_macros)]\n"
                + source,
                encoding="utf-8", newline="\n",
            )
            return subprocess.run(
                ["rustup", "run", toolchain, "clippy-driver", "--crate-name",
                 "edgeagent_sqlx_query_policy_fixture", "--edition=2024", "--crate-type=lib",
                 "--emit=metadata", "--error-format=json", "--extern", f"sqlx={sdk}",
                 "-L", f"dependency={sdk.parent}", "--out-dir", str(fixture / "build"),
                 "-D", "warnings", str(fixture / "src" / "lib.rs")],
                cwd=root, env=environment, capture_output=True, text=True, check=False,
                timeout=60,
            )

        # A real checked macro must compile under the same policy. Its query
        # comes from the adapter, and its describe data comes from .sqlx.
        positive = compile_source(
            'pub fn checked() { let _query = sqlx::query_file!('
            '"queries/mark_published.sql", "source", "id", "owner", 1_i64); }\n'
        )
        if positive.returncode:
            raise RuntimeError("checked SQLx macro fixture failed:\n" + diagnostics(positive))

        # Merely referencing each resolved function/macro exercises Clippy's
        # path resolution, including renaming; no database or effect is needed.
        for field, lint in [("disallowed-methods", "disallowed_methods"),
                            ("disallowed-macros", "disallowed_macros")]:
            for entry in policy[field]:
                path = entry["path"]
                for alias in [False, True]:
                    negative = compile_source(negative_source(path, alias, identity))
                    require_rejection(negative, lint, f"{path} (alias={alias})")
        print("SQLx query policy: checked macro accepted; runtime/unchecked APIs and aliases rejected")


if __name__ == "__main__":
    verify()
