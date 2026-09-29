from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


SCRIPTS = Path(__file__).parents[1] / "scripts"


def _load(name: str) -> object:
    path = SCRIPTS / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load module from {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


verify_supply_chain = _load("verify_supply_chain")
generate_sboms = _load("generate_sboms")


class GenerateSbomsTests(unittest.TestCase):
    def setUp(self) -> None:
        self.deployable = verify_supply_chain.ArtifactSpec(
            service="gateway",
            binary="edgeagent-gateway",
        )

    @staticmethod
    def _write_test_workspace(root: Path) -> None:
        (root / "Cargo.toml").write_text(
            '[workspace]\nmembers = ["services/gateway", "tools/maintenance"]\n',
            encoding="utf-8",
        )

    def test_rust_command_covers_workspace_binaries_reproducibly(self) -> None:
        command = generate_sboms.rust_sbom_command(
            cargo_cyclonedx="cargo-cyclonedx",
            spec_version="1.5",
        )

        self.assertIn("Cargo.toml", command)
        self.assertIn("binaries", command)
        self.assertIn("all", command)
        self.assertNotIn("--override-filename", command)

    def test_rust_generation_moves_only_deployables_and_cleans_tool_outputs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_test_workspace(root)
            output = root / "artifacts" / "sbom"
            service_output = (
                root
                / "services"
                / self.deployable.service
                / f"{self.deployable.binary}_bin.cdx.json"
            )
            tool_output = root / "tools" / "maintenance" / "maintenance_cdylib.cdx.json"

            def generate(*_args: object, **_kwargs: object) -> object:
                service_output.parent.mkdir(parents=True)
                service_output.write_text("service", encoding="utf-8")
                tool_output.parent.mkdir(parents=True)
                tool_output.write_text("tool", encoding="utf-8")
                return mock.Mock(returncode=0)

            with (
                mock.patch.object(generate_sboms, "_git_value", return_value="0"),
                mock.patch.object(generate_sboms, "_run", side_effect=generate) as runner,
            ):
                generate_sboms.generate_rust_sboms(
                    root,
                    output,
                    [self.deployable],
                    cargo_cyclonedx="cargo-cyclonedx",
                    spec_version="1.5",
                )

            self.assertEqual(1, runner.call_count)
            self.assertTrue((output / self.deployable.source_filename).is_file())
            self.assertFalse(service_output.exists())
            self.assertFalse(tool_output.exists())

    def test_rust_generation_cleans_partial_tool_output_after_failure(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_test_workspace(root)
            tool_output = root / "tools" / "maintenance" / "maintenance_cdylib.cdx.json"

            def fail(*_args: object, **_kwargs: object) -> object:
                tool_output.parent.mkdir(parents=True)
                tool_output.write_text("partial", encoding="utf-8")
                raise generate_sboms.GenerationError("simulated failure")

            with (
                mock.patch.object(generate_sboms, "_git_value", return_value="0"),
                mock.patch.object(generate_sboms, "_run", side_effect=fail),
                self.assertRaisesRegex(
                    generate_sboms.GenerationError,
                    "simulated failure",
                ),
            ):
                generate_sboms.generate_rust_sboms(
                    root,
                    root / "artifacts" / "sbom",
                    [self.deployable],
                    cargo_cyclonedx="cargo-cyclonedx",
                    spec_version="1.5",
                )

            self.assertFalse(tool_output.exists())

    def test_rust_generation_preserves_preexisting_package_sbom_and_does_not_run(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_test_workspace(root)
            preexisting = root / "tools" / "maintenance" / "manual.cdx.json"
            preexisting.parent.mkdir(parents=True)
            preexisting.write_text("evidence", encoding="utf-8")

            with (
                mock.patch.object(generate_sboms, "_run") as runner,
                self.assertRaisesRegex(
                    generate_sboms.GenerationError,
                    "refusing to overwrite",
                ),
            ):
                generate_sboms.generate_rust_sboms(
                    root,
                    root / "artifacts" / "sbom",
                    [self.deployable],
                    cargo_cyclonedx="cargo-cyclonedx",
                    spec_version="1.5",
                )

            runner.assert_not_called()
            self.assertEqual("evidence", preexisting.read_text(encoding="utf-8"))

    def test_rust_generation_rejects_output_inside_a_workspace_package(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self._write_test_workspace(root)
            output = root / "services" / "gateway" / "artifacts"

            with (
                mock.patch.object(generate_sboms, "_run") as runner,
                self.assertRaisesRegex(
                    generate_sboms.GenerationError,
                    "outside workspace package directories",
                ),
            ):
                generate_sboms.generate_rust_sboms(
                    root,
                    output,
                    [self.deployable],
                    cargo_cyclonedx="cargo-cyclonedx",
                    spec_version="1.5",
                )

            runner.assert_not_called()
            self.assertFalse(output.exists())

    def test_image_command_forces_local_daemon_and_cyclonedx_json(self) -> None:
        output = Path("artifacts/sbom/edgeagent-gateway.image.cdx.json")

        command = generate_sboms.image_sbom_command(
            self.deployable,
            syft="syft",
            output=output,
            spec_version="1.6",
        )

        self.assertIn("docker:edgeagent-ci/gateway:sbom", command)
        self.assertIn(f"cyclonedx-json@1.6={output}", command)
        self.assertNotIn(f"cyclonedx-json={output}", command)

    def test_published_image_command_scans_an_immutable_registry_digest(self) -> None:
        output = Path("artifacts/sbom/edgeagent-gateway.image.cdx.json")
        digest = "sha256:" + ("a" * 64)

        command = generate_sboms.published_image_sbom_command(
            self.deployable,
            image="ghcr.io/example/edge-agent-gateway",
            digest=digest,
            syft="syft",
            output=output,
            spec_version="1.6",
        )

        self.assertIn(
            f"registry:ghcr.io/example/edge-agent-gateway@{digest}",
            command,
        )
        self.assertIn(f"cyclonedx-json@1.6={output}", command)

    def test_published_image_command_rejects_a_mutable_tag(self) -> None:
        with self.assertRaisesRegex(
            generate_sboms.GenerationError,
            "untagged lowercase GHCR",
        ):
            generate_sboms.published_image_sbom_command(
                self.deployable,
                image="ghcr.io/example/edge-agent-gateway:latest",
                digest="sha256:" + ("a" * 64),
                syft="syft",
                output=Path("gateway.cdx.json"),
                spec_version="1.6",
            )


if __name__ == "__main__":
    unittest.main()
