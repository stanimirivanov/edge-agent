from __future__ import annotations

import importlib.util
import shutil
from pathlib import Path
import sys
import tempfile
import unittest


MODULE_PATH = Path(__file__).parents[1] / "scripts" / "verify_images.py"
SPEC = importlib.util.spec_from_file_location("verify_images", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"could not load verifier from {MODULE_PATH}")
verify_images = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = verify_images
SPEC.loader.exec_module(verify_images)


class VerifyImagesTests(unittest.TestCase):
    def fixture(self, directory: str) -> Path:
        source = Path(__file__).parents[1]
        root = Path(directory)
        paths = ["Dockerfile", ".dockerignore", "rust-toolchain.toml", "deploy/images.toml"]
        paths.extend(f"services/{service}/Cargo.toml" for service in verify_images.EXPECTED_SERVICES)
        for relative in paths:
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source / relative, target)
        return root

    def test_sqlx_build_inputs_cannot_be_removed_commented_or_copied_after_compilation(self) -> None:
        for instruction, reason in [
            ("COPY .cargo/config.toml ./.cargo/config.toml", "Cargo config"),
            ("COPY .sqlx ./.sqlx", "SQLx query cache"),
            ("ENV SQLX_OFFLINE=true", "SQLx offline mode"),
        ]:
            for mutation in ["remove", "comment", "late"]:
                with self.subTest(instruction=instruction, mutation=mutation), tempfile.TemporaryDirectory() as directory:
                    root = self.fixture(directory)
                    path = root / "Dockerfile"
                    original = path.read_text(encoding="utf-8")
                    replacement = f"# {instruction}\n" if mutation == "comment" else ""
                    changed = original.replace(f"{instruction}\n", replacement)
                    if mutation == "late":
                        changed += f"\n{instruction}\n"
                    path.write_text(changed, encoding="utf-8")
                    _, _, problems = verify_images.verify(root)
                    self.assertTrue(any(reason in problem.message for problem in problems), problems)

    def test_sqlx_context_inputs_cannot_be_excluded(self) -> None:
        for pattern in ["!.cargo/", ".cargo/*", "!.cargo/config.toml", "!.sqlx/", ".sqlx/*", "!.sqlx/query-*.json"]:
            with self.subTest(pattern=pattern), tempfile.TemporaryDirectory() as directory:
                root = self.fixture(directory)
                path = root / ".dockerignore"
                path.write_text(path.read_text(encoding="utf-8").replace(f"{pattern}\n", ""), encoding="utf-8")
                _, _, problems = verify_images.verify(root)
                self.assertTrue(any(pattern in problem.message for problem in problems), problems)

    def test_compiler_context_cannot_expose_sibling_credentials_or_shadow_exclusions(self) -> None:
        for directory in [".cargo", ".sqlx"]:
            for mutation in ["late_exclusion", "broad_allow", "extra_file"]:
                with self.subTest(directory=directory, mutation=mutation), tempfile.TemporaryDirectory() as fixture:
                    root = self.fixture(fixture)
                    path = root / ".dockerignore"
                    original = path.read_text(encoding="utf-8")
                    if mutation == "late_exclusion":
                        changed = original.replace(f"{directory}/*\n", "") + f"{directory}/*\n"
                    else:
                        filename = "**" if mutation == "broad_allow" else "credentials.toml"
                        changed = original + f"!{directory}/{filename}\n"
                    path.write_text(changed, encoding="utf-8")
                    _, _, problems = verify_images.verify(root)
                    self.assertTrue(any(directory in problem.message for problem in problems), problems)

    def test_sqlx_online_build_mode_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = self.fixture(directory)
            path = root / "Dockerfile"
            path.write_text(path.read_text(encoding="utf-8").replace("SQLX_OFFLINE=true", "SQLX_OFFLINE=false"), encoding="utf-8")
            _, _, problems = verify_images.verify(root)
            self.assertTrue(any("SQLx offline mode" in problem.message for problem in problems), problems)

    def test_later_env_assignments_cannot_override_offline_mode(self) -> None:
        for override in [
            "ENV SQLX_OFFLINE=false",
            "ENV SQLX_OFFLINE=",
            "ENV OTHER=value SQLX_OFFLINE=false",
            "ENV SQLX_OFFLINE false",
        ]:
            with self.subTest(override=override), tempfile.TemporaryDirectory() as directory:
                root = self.fixture(directory)
                path = root / "Dockerfile"
                changed = path.read_text(encoding="utf-8").replace(
                    "RUN cargo build ", f"{override}\nRUN cargo build ", 1
                )
                path.write_text(changed, encoding="utf-8")
                _, _, problems = verify_images.verify(root)
                self.assertTrue(any("overridden" in problem.message for problem in problems), problems)

    def test_current_image_contract_is_valid(self) -> None:
        root = Path(__file__).parents[1]

        contract, specs, problems = verify_images.verify(root)

        self.assertIsNotNone(contract)
        self.assertEqual(5, len(specs))
        self.assertEqual([], problems)

    def test_descriptor_component_mismatch_is_rejected(self) -> None:
        spec = verify_images.ImageSpec(
            service="gateway",
            package="edgeagent-gateway",
            binary="edgeagent-gateway",
            component="edgeagent.gateway",
            capabilities=("command-intake", "query-composition"),
        )
        output = "\n".join(
            [
                "contract=edgeagent.component.v1",
                "component=edgeagent.research",
                "summary=Wrong component.",
                "capabilities=command-intake,query-composition",
            ]
        )

        problems = verify_images.check_descriptor(spec, output)

        self.assertEqual(1, len(problems))
        self.assertIn("component", problems[0].message)

    def test_build_inputs_are_digest_pinned(self) -> None:
        root = Path(__file__).parents[1]

        contract, _, problems = verify_images.verify(root)

        self.assertEqual([], problems)
        self.assertIsNotNone(contract)
        self.assertRegex(contract.dockerfile_frontend, r"@sha256:[0-9a-f]{64}$")
        self.assertRegex(contract.builder_image, r"@sha256:[0-9a-f]{64}$")

    def test_descriptor_with_unknown_field_is_rejected(self) -> None:
        spec = verify_images.ImageSpec(
            service="research",
            package="edgeagent-research",
            binary="edgeagent-research",
            component="edgeagent.research",
            capabilities=("artifact-publication", "deterministic-research"),
        )
        output = "\n".join(
            [
                "contract=edgeagent.component.v1",
                "component=edgeagent.research",
                "summary=Research.",
                "capabilities=artifact-publication,deterministic-research",
                "extra=unexpected",
            ]
        )

        problems = verify_images.check_descriptor(spec, output)

        self.assertEqual(1, len(problems))
        self.assertIn("fields", problems[0].message)


if __name__ == "__main__":
    unittest.main()
