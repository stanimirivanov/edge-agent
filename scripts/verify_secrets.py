#!/usr/bin/env python3
"""Fail-closed secret scanning of source files and complete local Git history."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib

from install_gitleaks import SecurityToolError, ToolPolicy, load_policy


ROOT = Path(__file__).resolve().parents[1]
FINDINGS_EXIT_CODE = 23
SCAN_TIMEOUT_SECONDS = 300
MAX_SNAPSHOT_BYTES = 128 * 1024 * 1024
MAX_REPORT_BYTES = 16 * 1024 * 1024
CONFIG_CREDENTIAL_REGEX = (
    r'''(?i)\bEDGEAGENT_(?:POSTGRES_PASSWORD|S3_(?:ACCESS|SECRET)_KEY)[ \t]*[=:][ \t]*["']?([a-z0-9_./+=-]+)'''
)
FIXTURE_PATH_REGEX = r'''^(?:\.[/\\])?deploy[/\\]local[/\\]\.env\.example$'''
CANCELLATION_PATH_REGEX = r'''^(?:\.[/\\])?crates[/\\]outbox-relay[/\\]tests[/\\]cancellation\.rs$'''
STORAGE_DOC_PATH_REGEX = r'''^(?:\.[/\\])?crates[/\\]market-data-application[/\\]src[/\\]storage\.rs$'''
STORAGE_DOC_MATCH_REGEX = r"^access, missing[/]corrupt $"
SELF_TEST_CASE_IDS = (
    "clean", "built-in-rule", "canonical-fixture", "changed-fixture",
    "fixture-outside-allowlist", "nested-fixture-outside-allowlist", "inline-suppression",
    "idempotency-fixture", "changed-idempotency-fixture", "idempotency-fixture-outside-allowlist",
    "nested-idempotency-fixture-outside-allowlist",
    "historical-storage-rustdoc", "changed-storage-rustdoc", "storage-rustdoc-other-context",
    "storage-rustdoc-outside-allowlist", "nested-storage-rustdoc-outside-allowlist",
    "removed-secret-history",
    "removed-merge-only-secret-history",
)


@dataclass(frozen=True, slots=True)
class Finding:
    """Only safe metadata escapes the scanner's sensitive report boundary."""

    rule: str
    file_id: str
    line: int
    commit: str

    def describe(self) -> str:
        return f"rule={self.rule} file-sha256={self.file_id} line={self.line} commit={self.commit}"


def expected_config(version: str) -> dict:
    """The reviewed detector policy; undocumented suppression is not permitted."""

    return {
        "title": "EdgeAgent secret detection",
        "minVersion": f"v{version}",
        "extend": {"useDefault": True},
        "rules": [{
            "id": "edgeagent-config-credential",
            "description": "Literal EdgeAgent PostgreSQL or object-store credential",
            "regex": CONFIG_CREDENTIAL_REGEX,
            "secretGroup": 1,
            "keywords": ["edgeagent_postgres_password", "edgeagent_s3_access_key", "edgeagent_s3_secret_key"],
            "allowlists": [{
                "description": "Exact public local-only fixtures, only in their canonical fixture file",
                "condition": "AND",
                "paths": [FIXTURE_PATH_REGEX],
                "regexTarget": "secret",
                "regexes": [r"^edgeagent-local-(?:postgres|access|secret)$"],
            }],
        }, {
            "id": "generic-api-key",
            "allowlists": [{
                "description": "Exact public message idempotency identity in the cancellation fixture",
                "condition": "AND",
                "paths": [CANCELLATION_PATH_REGEX],
                "regexTarget": "secret",
                "regexes": [r"^cancellation-order-01$"],
            }, {
                "description": "Exact historical evidence-storage Rustdoc false positive",
                "condition": "AND",
                "paths": [STORAGE_DOC_PATH_REGEX],
                "regexTarget": "match",
                "regexes": [STORAGE_DOC_MATCH_REGEX],
            }],
        }],
    }


def validate_policy(root: Path) -> ToolPolicy:
    """Check configuration before executing any scanner, without network access."""

    policy = load_policy(root)
    try:
        manifest = tomllib.loads((root / "supply-chain/security-tools.toml").read_text(encoding="utf-8"))
        config = tomllib.loads((root / ".gitleaks.toml").read_text(encoding="utf-8"))
    except (OSError, UnicodeError, tomllib.TOMLDecodeError) as error:
        raise SecurityToolError("cannot read secret detector configuration") from error
    if set(manifest) != {"schema_version", "gitleaks"} or set(manifest["gitleaks"]) != {
        "version", "license", "release_url", "linux_x64", "windows_x64"
    } or any(set(manifest["gitleaks"][target]) != {"archive", "sha256", "executable"} for target in policy.archives):
        raise SecurityToolError("security tool policy contains unreviewed fields")
    if config != expected_config(policy.version):
        raise SecurityToolError("secret detector policy drift; preserve default rules and exact path/value fixture exceptions")
    if (root / ".gitleaksignore").exists():
        raise SecurityToolError("fingerprint ignore files are prohibited")
    return policy


def sanitized_environment(source: Path | None = None) -> dict[str, str]:
    """Ambient detector settings must not override the reviewed invocation."""

    environment = {key: value for key, value in os.environ.items() if not key.upper().startswith(("GITLEAKS_", "GIT_"))}
    environment["GIT_NO_REPLACE_OBJECTS"] = "1"
    if source is not None:
        # The Windows sandbox may use another account to own the checkout.
        # Trust only this invocation's already selected source, never '*', and
        # do not retain any ambient routing or configuration injection.
        environment["GIT_CONFIG_COUNT"] = "1"
        environment["GIT_CONFIG_KEY_0"] = "safe.directory"
        environment["GIT_CONFIG_VALUE_0"] = str(source.resolve())
    return environment


def captured_command(arguments: list[str], root: Path) -> subprocess.CompletedProcess[bytes]:
    """Capture only protocol output; never print subprocess diagnostics or errors."""

    try:
        return subprocess.run(
            arguments, cwd=root, env=sanitized_environment(root), stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, check=False, timeout=30,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise SecurityToolError("required security verification command could not complete") from error


def require_version(executable: str, policy: ToolPolicy, root: Path) -> None:
    """Use the documented `version` protocol and require the exact release pin."""

    output = captured_command([executable, "version"], root)
    if output.returncode != 0 or output.stdout.strip() != policy.version.encode("ascii"):
        raise SecurityToolError(f"secret scanning requires the pinned Gitleaks {policy.version}; run scripts/install_gitleaks.py")


def parse_report(path: Path, source: Path | None = None) -> list[Finding]:
    """Discard matches, secrets, authors, messages, and paths before reporting."""

    try:
        if path.stat().st_size > MAX_REPORT_BYTES:
            raise SecurityToolError("secret detector report exceeds the verification limit")
        document = json.loads(path.read_bytes())
    except (OSError, ValueError) as error:
        raise SecurityToolError("secret detector did not produce a valid report") from error
    if document is None:
        document = []
    if not isinstance(document, list):
        raise SecurityToolError("secret detector report has an invalid shape")
    findings = []
    for value in document:
        if not isinstance(value, dict):
            raise SecurityToolError("secret detector finding has an invalid shape")
        rule, file, line, commit = (value.get(key) for key in ("RuleID", "File", "StartLine", "Commit"))
        if (
            not isinstance(rule, str) or re.fullmatch(r"[a-z][a-z0-9-]{0,79}", rule) is None
            or not isinstance(file, str) or not file or type(line) is not int or line < 1
            or not isinstance(commit, str) or (commit and re.fullmatch(r"[0-9a-f]{40,64}", commit) is None)
            or value.get("Secret") != "REDACTED"
        ):
            raise SecurityToolError("secret detector finding violates the redacted evidence contract")
        normalized = file.replace("\\", "/")
        if source is not None:
            prefix = source.as_posix().rstrip("/") + "/"
            if normalized.startswith(prefix):
                normalized = normalized[len(prefix):]
        if normalized.startswith("./"):
            normalized = normalized[2:]
        findings.append(Finding(rule, hashlib.sha256(normalized.encode("utf-8")).hexdigest()[:16], line, commit[:12] or "working-tree"))
    return sorted(findings, key=lambda value: (value.commit, value.file_id, value.line, value.rule))


def scan(executable: str, root: Path, source: Path, mode: str, temporary: Path) -> list[Finding]:
    """No baseline, inline suppression, ambient config, raw output, or saved artifact."""

    report = temporary / "report.json"
    report.unlink(missing_ok=True)
    ignored = temporary / "empty-ignore-directory"
    ignored.mkdir(exist_ok=True)
    arguments = [
        executable, mode, "--config", str(root / ".gitleaks.toml"),
        "--gitleaks-ignore-path", str(ignored), "--ignore-gitleaks-allow", "--redact=100",
        "--no-banner", "--no-color", "--log-level=error", f"--exit-code={FINDINGS_EXIT_CODE}",
        "--report-format=json", "--report-path", str(report), f"--timeout={SCAN_TIMEOUT_SECONDS}",
    ]
    if mode == "git":
        arguments.append("--log-opts=--all --full-history --diff-merges=separate --no-ext-diff --no-textconv")
    elif mode != "dir":
        raise SecurityToolError("invalid secret detector source mode")
    # Relative detector paths give allowlists an exact repository namespace;
    # suffix matching absolute temporary paths would also admit nested copies.
    arguments.append(".")
    try:
        output = subprocess.run(
            arguments, cwd=source, env=sanitized_environment(source), stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False,
            timeout=SCAN_TIMEOUT_SECONDS + 10,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise SecurityToolError("secret detector could not complete; no passing evidence") from error
    if output.returncode not in (0, FINDINGS_EXIT_CODE):
        raise SecurityToolError("secret detector scan failed or was incomplete; raw output withheld")
    findings = parse_report(report, source)
    if (output.returncode == FINDINGS_EXIT_CODE) != bool(findings):
        raise SecurityToolError("secret detector exit status and finding report disagree")
    return findings


def source_paths(root: Path) -> list[str]:
    """Tracked current files plus nonignored new files, never build/cache discovery."""

    output = captured_command(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], root)
    if output.returncode != 0 or not output.stdout.endswith(b"\0"):
        raise SecurityToolError("cannot enumerate a nonempty Git source snapshot")
    try:
        paths = sorted(set(output.stdout[:-1].decode("utf-8").split("\0")))
    except UnicodeError as error:
        raise SecurityToolError("Git source paths must be UTF-8") from error
    for value in paths:
        path = PurePosixPath(value)
        if not value or path.is_absolute() or any(part in {"", ".", "..", ".git"} for part in path.parts) or "\\" in value or ":" in value:
            raise SecurityToolError("Git source snapshot contains an unsafe path")
        if path.name == ".gitleaksignore":
            raise SecurityToolError("fingerprint ignore files are prohibited")
    return paths


def snapshot(root: Path, destination: Path, paths: list[str]) -> int:
    """Bound copies to regular files inside the repository; reject symlink escapes."""

    total, copied = 0, 0
    resolved_root = root.resolve()
    for value in paths:
        source = root / value
        # Deleted tracked files have no current bytes; their committed bytes are
        # still covered by the independent history scan.
        if not source.exists() and not source.is_symlink():
            continue
        if any(parent.is_symlink() for parent in (source, *source.parents) if parent != resolved_root):
            raise SecurityToolError("Git source snapshot contains a symlink")
        try:
            source.resolve().relative_to(resolved_root)
        except ValueError as error:
            raise SecurityToolError("Git source snapshot escapes the repository") from error
        if not source.is_file():
            raise SecurityToolError("Git source snapshot contains a non-file entry")
        total += source.stat().st_size
        if total > MAX_SNAPSHOT_BYTES:
            raise SecurityToolError("Git source snapshot exceeds the verification limit")
        target = destination / value
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
        copied += 1
    if copied == 0:
        raise SecurityToolError("Git source snapshot contains no current files")
    return copied


def require_complete_history(root: Path) -> None:
    """Shallow or empty history cannot satisfy full-history evidence."""

    top = captured_command(["git", "rev-parse", "--show-toplevel"], root)
    try:
        expected_root = top.returncode == 0 and Path(top.stdout.strip().decode("utf-8")).resolve() == root.resolve()
    except (UnicodeError, OSError, ValueError):
        expected_root = False
    if not expected_root:
        raise SecurityToolError("secret scanning requires the intended repository root, not a nested or redirected checkout")
    for arguments, predicate in (
        (["rev-parse", "--is-shallow-repository"], lambda value: value == b"false"),
        (["rev-parse", "--verify", "HEAD"], lambda value: re.fullmatch(rb"[0-9a-f]{40,64}", value) is not None),
        (["rev-list", "--count", "--all"], lambda value: value.isdigit() and int(value) > 0),
    ):
        output = captured_command(["git", *arguments], root)
        if output.returncode != 0 or not predicate(output.stdout.strip()):
            raise SecurityToolError("secret scanning requires nonempty complete Git history; use fetch-depth: 0 in CI")


def self_test(executable: str, root: Path) -> int:
    """Prove the real detector works, without committing a complete secret fixture."""

    secret = "gh" + "p_" + "Q7a4C9e2G5i8K1m3O6q0S2u5W8y1Z4b7D0f3"
    fixture = "edgeagent-local-postgres"
    changed = "synthetic-password-Q7a4C9e2"
    # Generic detection deliberately excludes common words such as "password";
    # this case must exercise entropy detection, not an upstream stopword.
    changed_identity = secret[4:]
    cancellation = "cancellation-order-01"
    storage_path = "crates/market-data-application/src/storage.rs"
    # Assemble the historical prose so this scanner proof does not itself carry
    # the misleading assignment span in an unrelated source file.
    documentation_value = "missing/corrupt"
    documentation = "/// Reject invalid identity, denied/expired access, " + f"{documentation_value} bytes and I/O failures.\n"
    cases = (
        ("clean", "src/probe.txt", "non-sensitive source\n", None),
        ("built-in-rule", "src/probe.txt", f"access_token={secret}\n", "github-pat"),
        ("canonical-fixture", "deploy/local/.env.example", f"EDGEAGENT_POSTGRES_PASSWORD={fixture}\n", None),
        ("changed-fixture", "deploy/local/.env.example", f"EDGEAGENT_POSTGRES_PASSWORD={changed}\n", "edgeagent-config-credential"),
        ("fixture-outside-allowlist", "src/probe.txt", f"EDGEAGENT_POSTGRES_PASSWORD={fixture}\n", "edgeagent-config-credential"),
        ("nested-fixture-outside-allowlist", "nested/deploy/local/.env.example", f"EDGEAGENT_POSTGRES_PASSWORD={fixture}\n", "edgeagent-config-credential"),
        ("inline-suppression", "src/probe.txt", f"access_token={secret} # gitleaks:allow\n", "github-pat"),
        ("idempotency-fixture", "crates/outbox-relay/tests/cancellation.rs", f'idempotency_key: "{cancellation}"\n', None),
        ("changed-idempotency-fixture", "crates/outbox-relay/tests/cancellation.rs", f'idempotency_key: "{changed_identity}"\n', "generic-api-key"),
        ("idempotency-fixture-outside-allowlist", "src/probe.txt", f'idempotency_key: "{cancellation}"\n', "generic-api-key"),
        ("nested-idempotency-fixture-outside-allowlist", "nested/crates/outbox-relay/tests/cancellation.rs", f'idempotency_key: "{cancellation}"\n', "generic-api-key"),
        ("historical-storage-rustdoc", storage_path, documentation, None),
        ("changed-storage-rustdoc", storage_path, documentation.replace(documentation_value, changed_identity), "generic-api-key"),
        ("storage-rustdoc-other-context", storage_path, f'access_token="{documentation_value}"\n', "generic-api-key"),
        ("storage-rustdoc-outside-allowlist", "src/probe.rs", documentation, "generic-api-key"),
        ("nested-storage-rustdoc-outside-allowlist", f"nested/{storage_path}", documentation, "generic-api-key"),
    )
    if tuple(case[0] for case in cases) + SELF_TEST_CASE_IDS[-2:] != SELF_TEST_CASE_IDS:
        raise SecurityToolError("secret detector self-proof case inventory drift")
    for name, relative, contents, expected in cases:
        with tempfile.TemporaryDirectory(prefix="edgeagent-secret-proof-") as directory:
            temporary = Path(directory)
            source = temporary / "source"
            path = source / relative
            path.parent.mkdir(parents=True)
            path.write_text(contents, encoding="utf-8")
            findings = scan(executable, root, source, "dir", temporary)
            raw_report = (temporary / "report.json").read_text(encoding="utf-8")
            if secret in raw_report or changed in raw_report or changed_identity in raw_report:
                raise SecurityToolError(f"secret detector self-proof failed: {name} evidence is not redacted")
            if (expected is None and findings) or (expected is not None and expected not in {finding.rule for finding in findings}):
                raise SecurityToolError(f"secret detector self-proof failed: {name}")
        print(f"secret detector self-proof passed: {name}")
    # A removed token must still fail the historical sensor even though the
    # current tree is clean. Commits exist only in an automatically removed
    # temporary repository, never the contributor's checkout.
    with tempfile.TemporaryDirectory(prefix="edgeagent-secret-history-proof-") as directory:
        temporary = Path(directory)
        source = temporary / "source"
        source.mkdir()
        inert = temporary / "empty-hooks-and-template"
        inert.mkdir()
        path = source / "probe.txt"
        path.write_text(f"access_token={secret}\n", encoding="utf-8")
        identity = ["-c", "user.name=EdgeAgent synthetic proof", "-c", "user.email=proof@example.invalid", "-c", "commit.gpgsign=false", "-c", f"core.hooksPath={inert}"]
        for arguments in (["-c", f"init.templateDir={inert}", "init", "--initial-branch=main"], ["add", "--", "probe.txt"], [*identity, "commit", "--quiet", "-m", "Synthetic scanner proof"]):
            if captured_command(["git", *arguments], source).returncode != 0:
                raise SecurityToolError("secret detector history self-proof could not initialize")
        path.write_text("non-sensitive source\n", encoding="utf-8")
        for arguments in (["add", "--", "probe.txt"], [*identity, "commit", "--quiet", "-m", "Remove synthetic token"]):
            if captured_command(["git", *arguments], source).returncode != 0:
                raise SecurityToolError("secret detector history self-proof could not commit cleanup")
        require_complete_history(source)
        if scan(executable, root, source, "dir", temporary):
            raise SecurityToolError("secret detector history self-proof current tree was not clean")
        historical = scan(executable, root, source, "git", temporary)
        if "github-pat" not in {finding.rule for finding in historical}:
            raise SecurityToolError("secret detector history self-proof missed a removed token")
        if secret in (temporary / "report.json").read_text(encoding="utf-8"):
            raise SecurityToolError("secret detector history self-proof evidence is not redacted")
    print("secret detector self-proof passed: removed-secret-history")
    prove_merge_only_secret(executable, root, secret)
    print("secret detector self-proof passed: removed-merge-only-secret-history")
    return len(cases) + 2


def prove_merge_only_secret(executable: str, root: Path, secret: str) -> None:
    """Prove Git merge diffs cannot hide a secret absent from both parents."""

    with tempfile.TemporaryDirectory(prefix="edgeagent-secret-merge-proof-") as directory:
        temporary = Path(directory)
        source, inert = temporary / "source", temporary / "empty-hooks-and-template"
        source.mkdir()
        inert.mkdir()
        identity = ["-c", "user.name=EdgeAgent synthetic proof", "-c", "user.email=proof@example.invalid", "-c", "commit.gpgsign=false", "-c", f"core.hooksPath={inert}"]

        def git(*arguments: str) -> bytes:
            output = captured_command(["git", *identity, *arguments], source)
            if output.returncode != 0:
                raise SecurityToolError("secret detector merge-history self-proof could not create its fixture")
            return output.stdout.strip()

        git("-c", f"init.templateDir={inert}", "init", "--initial-branch=main")
        path = source / "probe.txt"
        path.write_text("non-sensitive source\n", encoding="utf-8")
        git("add", "--", "probe.txt")
        git("commit", "--quiet", "-m", "Clean synthetic history")
        git("checkout", "--quiet", "-b", "side")
        (source / "side.txt").write_text("independent side work\n", encoding="utf-8")
        git("add", "--", "side.txt")
        git("commit", "--quiet", "-m", "Independent side parent")
        git("checkout", "--quiet", "main")
        (source / "main.txt").write_text("independent main work\n", encoding="utf-8")
        git("add", "--", "main.txt")
        git("commit", "--quiet", "-m", "Independent main parent")
        git("merge", "--no-ff", "--no-commit", "side")
        path.write_text(f"access_token={secret}\n", encoding="utf-8")
        git("add", "--", "probe.txt")
        git("commit", "--quiet", "-m", "Synthetic merge-only token")
        merge = git("rev-parse", "HEAD").decode("ascii")
        path.write_text("non-sensitive source\n", encoding="utf-8")
        git("add", "--", "probe.txt")
        git("commit", "--quiet", "-m", "Remove synthetic merge token")
        require_complete_history(source)
        if scan(executable, root, source, "dir", temporary):
            raise SecurityToolError("secret detector merge-history self-proof current tree was not clean")
        findings = scan(executable, root, source, "git", temporary)
        if not any(finding.rule == "github-pat" and finding.commit == merge[:12] for finding in findings):
            raise SecurityToolError("secret detector history self-proof missed a removed merge-only token")
        if secret in (temporary / "report.json").read_text(encoding="utf-8"):
            raise SecurityToolError("secret detector merge-history self-proof evidence is not redacted")


def verify(executable: str, root: Path) -> tuple[int, list[Finding]]:
    require_complete_history(root)
    paths = source_paths(root)
    with tempfile.TemporaryDirectory(prefix="edgeagent-secret-scan-") as directory:
        temporary = Path(directory)
        source = temporary / "source"
        source.mkdir()
        copied = snapshot(root, source, paths)
        current = scan(executable, root, source, "dir", temporary)
        history = scan(executable, root, root, "git", temporary)
    return copied, [*current, *history]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--policy-only", action="store_true")
    group.add_argument("--self-test", action="store_true")
    parser.add_argument("--gitleaks", help="pinned executable path; not a shell command")
    options = parser.parse_args()
    try:
        policy = validate_policy(ROOT)
        if options.policy_only:
            print("Secret detector policy passed; no scanner or history proof is claimed.")
            return 0
        executable = options.gitleaks
        if executable is None:
            archive = policy.archives.get("windows_x64" if sys.platform == "win32" else "linux_x64")
            local = ROOT / f"artifacts/security-tools/gitleaks-{policy.version}" / archive.executable
            executable = str(local) if local.is_file() else "gitleaks"
        found = shutil.which(executable)
        if found is not None:
            executable = str(Path(found).resolve())
        require_version(executable, policy, ROOT)
        if options.self_test:
            count = self_test(executable, ROOT)
            print(f"Secret detector contracts passed: {count} real-scanner cases.")
            return 0
        count, findings = verify(executable, ROOT)
        if findings:
            print(f"Secret scanning failed: {len(findings)} findings; matched bytes, filenames and identity metadata withheld.", file=sys.stderr)
            for finding in findings[:20]:
                print(f"  {finding.describe()}", file=sys.stderr)
            return 1
    except SecurityToolError as error:
        print(f"Secret verification failed: {error}", file=sys.stderr)
        return 1
    except OSError:
        print("Secret verification failed: source or temporary evidence could not be read; raw diagnostics withheld.", file=sys.stderr)
        return 1
    print(f"Secret scanning passed: {count} current source files and complete local reachable Git history.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
