from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import unittest


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

    def test_rust_command_covers_workspace_binaries_reproducibly(self) -> None:
        command = generate_sboms.rust_sbom_command(
            cargo_cyclonedx="cargo-cyclonedx",
            spec_version="1.5",
        )

        self.assertIn("Cargo.toml", command)
        self.assertIn("binaries", command)
        self.assertIn("all", command)
        self.assertNotIn("--override-filename", command)

    def test_image_command_forces_local_daemon_and_cyclonedx_json(self) -> None:
        output = Path("artifacts/sbom/edgeagent-gateway.image.cdx.json")

        command = generate_sboms.image_sbom_command(
            self.deployable,
            syft="syft",
            output=output,
        )

        self.assertIn("docker:edgeagent-ci/gateway:sbom", command)
        self.assertIn(f"cyclonedx-json={output}", command)


if __name__ == "__main__":
    unittest.main()
