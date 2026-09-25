from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest


SCRIPTS = Path(__file__).parents[1] / "scripts"
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))
MODULE_PATH = SCRIPTS / "prepare_release.py"
SPEC = importlib.util.spec_from_file_location("prepare_release", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"could not load release planner from {MODULE_PATH}")
prepare_release = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = prepare_release
SPEC.loader.exec_module(prepare_release)


class PrepareReleaseTests(unittest.TestCase):
    def test_plan_covers_every_deployable_with_lowercase_ghcr_names(self) -> None:
        root = Path(__file__).parents[1]

        version, matrix = prepare_release.build_release_plan(
            root,
            tag="v1.2.3-rc.1",
            repository="Example/Edge-Agent",
            registry="ghcr.io",
        )

        self.assertEqual("1.2.3-rc.1", version)
        self.assertEqual(5, len(matrix["include"]))
        self.assertEqual(
            "ghcr.io/example/edge-agent-audit-projector",
            matrix["include"][0]["image"],
        )

    def test_plan_rejects_non_semver_tag_before_publication(self) -> None:
        with self.assertRaisesRegex(
            prepare_release.ReleasePlanError,
            "vMAJOR.MINOR.PATCH",
        ):
            prepare_release.build_release_plan(
                Path(__file__).parents[1],
                tag="latest",
                repository="example/edge-agent",
                registry="ghcr.io",
            )

    def test_plan_rejects_numeric_prerelease_with_a_leading_zero(self) -> None:
        with self.assertRaisesRegex(
            prepare_release.ReleasePlanError,
            "vMAJOR.MINOR.PATCH",
        ):
            prepare_release.build_release_plan(
                Path(__file__).parents[1],
                tag="v1.2.3-01",
                repository="example/edge-agent",
                registry="ghcr.io",
            )

    def test_github_outputs_are_compact_and_newline_safe(self) -> None:
        matrix = {"include": [{"service": "gateway", "image": "ghcr.io/x/y"}]}
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "github-output"

            prepare_release.write_github_outputs(
                output,
                version="1.2.3",
                matrix=matrix,
            )

            values = dict(
                line.split("=", 1)
                for line in output.read_text(encoding="utf-8").splitlines()
            )
        self.assertEqual("1.2.3", values["version"])
        self.assertEqual(matrix, json.loads(values["matrix"]))


if __name__ == "__main__":
    unittest.main()
