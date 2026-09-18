from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest


MODULE_PATH = Path(__file__).parents[1] / "scripts" / "verify_repository.py"
SPEC = importlib.util.spec_from_file_location("verify_repository", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"could not load verifier from {MODULE_PATH}")
verify_repository = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = verify_repository
SPEC.loader.exec_module(verify_repository)


class VerifyRepositoryTests(unittest.TestCase):
    def test_long_document_without_tldr_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            document = root / "long.md"
            document.write_text("# Long\n\n" + "word " * 800 + "\n", encoding="utf-8")

            problems = verify_repository.check_tldr(root)

            self.assertEqual(1, len(problems))
            self.assertIn("requires '## TL;DR'", problems[0].message)

    def test_long_document_with_early_tldr_is_accepted(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            document = root / "long.md"
            document.write_text(
                "# Long\n\n## TL;DR\n\nSummary.\n\n" + "word " * 800 + "\n",
                encoding="utf-8",
            )

            self.assertEqual([], verify_repository.check_tldr(root))

    def test_missing_relative_link_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            document = root / "README.md"
            document.write_text("[missing](docs/missing.md)\n", encoding="utf-8")

            problems = verify_repository.check_links(root)

            self.assertEqual(1, len(problems))
            self.assertIn("broken relative link", problems[0].message)

    def test_external_and_anchor_links_are_not_resolved_locally(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            document = root / "README.md"
            document.write_text(
                "[web](https://example.com) [section](#section)\n",
                encoding="utf-8",
            )

            self.assertEqual([], verify_repository.check_links(root))

    def test_non_markdown_trailing_whitespace_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source.py"
            source.write_bytes(b"value = 1  \n")

            problems = verify_repository.check_format(root)

            self.assertEqual(1, len(problems))
            self.assertEqual(1, problems[0].line)


if __name__ == "__main__":
    unittest.main()
