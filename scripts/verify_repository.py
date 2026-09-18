#!/usr/bin/env python3
"""Verify EdgeAgent's repository-level documentation and text contracts."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import re
import sys
from urllib.parse import unquote


IGNORED_PARTS = {
    ".git",
    ".idea",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".venv",
    ".vscode",
    "__pycache__",
    "build",
    "dist",
    "node_modules",
}
TEXT_SUFFIXES = {
    ".css",
    ".html",
    ".js",
    ".json",
    ".jsonc",
    ".md",
    ".py",
    ".toml",
    ".ts",
    ".tsx",
    ".txt",
    ".yaml",
    ".yml",
}
TEXT_FILENAMES = {".editorconfig", ".gitignore", "Makefile"}
REQUIRED_PATHS = {
    ".editorconfig",
    ".gitignore",
    "AGENTS.md",
    "CODE_OF_CONDUCT.md",
    "CONTRIBUTING.md",
    "LICENSE",
    "Makefile",
    "README.md",
    "SECURITY.md",
    "docs/architecture/system-overview.md",
    "docs/architecture/deployment-portability.md",
    "docs/decisions/README.md",
    "docs/development/engineering-standards.md",
    "docs/product/vision.md",
    "docs/roadmap/milestones.md",
}
SELECTIVE_GUIDES = {
    "CONTRIBUTING.md",
    "SECURITY.md",
    "docs/architecture/system-overview.md",
    "docs/development/engineering-standards.md",
    "docs/product/vision.md",
    "docs/roadmap/milestones.md",
}
LINK_PATTERN = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")
SECOND_LEVEL_HEADING = re.compile(r"^##\s+", re.MULTILINE)
WORD_PATTERN = re.compile(r"\b[\w’'-]+\b", re.UNICODE)


@dataclass(frozen=True, order=True)
class Problem:
    """A stable, printable verification failure."""

    path: str
    message: str
    line: int = 0

    def render(self) -> str:
        """Return a compiler-style problem description."""

        location = self.path if self.line == 0 else f"{self.path}:{self.line}"
        return f"{location}: {self.message}"


def _is_ignored(path: Path) -> bool:
    return any(part in IGNORED_PARTS for part in path.parts)


def iter_text_files(root: Path) -> list[Path]:
    """Return repository text files in deterministic order."""

    files = []
    for path in root.rglob("*"):
        relative = path.relative_to(root)
        if path.is_file() and not _is_ignored(relative):
            if path.suffix.lower() in TEXT_SUFFIXES or path.name in TEXT_FILENAMES:
                files.append(path)
    return sorted(files)


def check_format(root: Path) -> list[Problem]:
    """Check encoding, line endings, final newlines, and trailing whitespace."""

    problems: list[Problem] = []
    for path in iter_text_files(root):
        relative = path.relative_to(root).as_posix()
        raw = path.read_bytes()
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError as error:
            problems.append(Problem(relative, f"is not valid UTF-8: {error}"))
            continue
        if raw.startswith(b"\xef\xbb\xbf"):
            problems.append(Problem(relative, "must not contain a UTF-8 byte-order mark"))
        if b"\r" in raw:
            problems.append(Problem(relative, "must use LF line endings"))
        if raw and not raw.endswith(b"\n"):
            problems.append(Problem(relative, "must end with a newline"))
        if path.suffix.lower() != ".md":
            for line_number, line in enumerate(text.splitlines(), start=1):
                if line.endswith((" ", "\t")):
                    problems.append(
                        Problem(relative, "contains trailing whitespace", line_number)
                    )
    return problems


def _requires_tldr(relative: str, text: str) -> bool:
    words = len(WORD_PATTERN.findall(text))
    headings = len(SECOND_LEVEL_HEADING.findall(text))
    return relative in SELECTIVE_GUIDES or words >= 800 or headings > 5


def check_tldr(root: Path) -> list[Problem]:
    """Require a discoverable TL;DR in long or selectively read documents."""

    problems: list[Problem] = []
    for path in sorted(root.rglob("*.md")):
        relative_path = path.relative_to(root)
        if _is_ignored(relative_path):
            continue
        relative = relative_path.as_posix()
        text = path.read_text(encoding="utf-8")
        if _requires_tldr(relative, text):
            lines = text.splitlines()
            try:
                index = lines.index("## TL;DR")
            except ValueError:
                problems.append(Problem(relative, "long/selective document requires '## TL;DR'"))
                continue
            if index > 12:
                problems.append(
                    Problem(relative, "'## TL;DR' must appear near the document start", index + 1)
                )
    return problems


def _link_target(raw_target: str) -> str | None:
    target = raw_target.strip()
    if target.startswith("<") and ">" in target:
        target = target[1 : target.index(">")]
    elif " " in target:
        target = target.split(" ", 1)[0]
    target = unquote(target).split("#", 1)[0]
    if not target or target.startswith(("http://", "https://", "mailto:")):
        return None
    return target


def check_links(root: Path) -> list[Problem]:
    """Check repository-relative Markdown links without making network calls."""

    problems: list[Problem] = []
    resolved_root = root.resolve()
    for path in sorted(root.rglob("*.md")):
        relative_path = path.relative_to(root)
        if _is_ignored(relative_path):
            continue
        relative = relative_path.as_posix()
        text = path.read_text(encoding="utf-8")
        for match in LINK_PATTERN.finditer(text):
            target = _link_target(match.group(1))
            if target is None:
                continue
            candidate = (
                resolved_root / target.lstrip("/")
                if target.startswith("/")
                else path.parent.resolve() / target
            ).resolve()
            line = text.count("\n", 0, match.start()) + 1
            try:
                candidate.relative_to(resolved_root)
            except ValueError:
                problems.append(Problem(relative, f"link escapes repository: {target}", line))
                continue
            if not candidate.exists():
                problems.append(Problem(relative, f"broken relative link: {target}", line))
    return problems


def check_required_paths(root: Path) -> list[Problem]:
    """Check that the repository's governance entry points exist."""

    return [
        Problem(relative, "required repository file is missing")
        for relative in sorted(REQUIRED_PATHS)
        if not (root / relative).is_file()
    ]


def verify(root: Path, *, formatting_only: bool = False) -> list[Problem]:
    """Run deterministic repository verification."""

    problems = check_format(root)
    if not formatting_only:
        problems.extend(check_required_paths(root))
        problems.extend(check_tldr(root))
        problems.extend(check_links(root))
    return sorted(set(problems))


def main(argv: list[str] | None = None) -> int:
    """Run the command-line verifier."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--format-check",
        action="store_true",
        help="run only encoding and text-format checks",
    )
    args = parser.parse_args(argv)
    root = Path(__file__).resolve().parents[1]
    problems = verify(root, formatting_only=args.format_check)
    if problems:
        for problem in problems:
            print(problem.render(), file=sys.stderr)
        print(f"FAILED: {len(problems)} repository problem(s)", file=sys.stderr)
        return 1
    mode = "format" if args.format_check else "repository"
    print(f"OK: {mode} verification passed for {root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
