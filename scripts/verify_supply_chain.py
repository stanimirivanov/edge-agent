#!/usr/bin/env python3
"""Verify EdgeAgent dependency policy and generated CycloneDX SBOM contracts."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
from pathlib import Path
import re
import sys
import tomllib


ALLOWED_LICENSES = {
    "Apache-2.0",
    "Apache-2.0 WITH LLVM-exception",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "MIT",
    "Unicode-3.0",
    "Zlib",
}
CRATES_IO_INDEX = "https://github.com/rust-lang/crates.io-index"


@dataclass(frozen=True, order=True)
class Problem:
    """A stable supply-chain verification failure."""

    path: str
    message: str

    def render(self) -> str:
        """Return a compiler-style failure description."""

        return f"{self.path}: {self.message}"


@dataclass(frozen=True)
class ToolPolicy:
    """Pinned tools and artifact policy loaded from repository configuration."""

    cargo_deny: str
    cargo_cyclonedx: str
    syft: str
    rust_spec: str
    accepted_specs: tuple[str, ...]
    retention_days: int
    cargo_deny_action: str
    cargo_deny_action_sha: str
    download_syft_action: str
    download_syft_action_sha: str
    upload_artifact_action: str
    upload_artifact_action_sha: str


@dataclass(frozen=True)
class ArtifactSpec:
    """Expected SBOM filenames for one independently deployable binary."""

    service: str
    binary: str

    @property
    def source_filename(self) -> str:
        """Return the Cargo dependency SBOM filename."""

        return f"{self.binary}.source.cdx.json"

    @property
    def image_filename(self) -> str:
        """Return the runtime image SBOM filename."""

        return f"{self.binary}.image.cdx.json"


def _load_toml(path: Path) -> dict[str, object]:
    with path.open("rb") as source:
        return tomllib.load(source)


def load_tool_policy(path: Path) -> tuple[ToolPolicy | None, list[Problem]]:
    """Load the pinned tool manifest with strict type and value checks."""

    relative = path.as_posix()
    if not path.is_file():
        return None, [Problem(relative, "tool manifest is missing")]
    manifest = _load_toml(path)
    problems: list[Problem] = []
    if manifest.get("schema_version") != 1:
        problems.append(Problem(relative, "schema_version must be 1"))

    string_fields = {
        "cargo_deny": manifest.get("cargo_deny"),
        "cargo_cyclonedx": manifest.get("cargo_cyclonedx"),
        "syft": manifest.get("syft"),
        "cyclonedx_rust_spec": manifest.get("cyclonedx_rust_spec"),
    }
    for name, value in string_fields.items():
        if not isinstance(value, str) or not value:
            problems.append(Problem(relative, f"{name} must be a non-empty string"))

    accepted = manifest.get("accepted_cyclonedx_specs")
    if not isinstance(accepted, list) or not accepted or not all(
        isinstance(value, str) and value for value in accepted
    ):
        problems.append(
            Problem(relative, "accepted_cyclonedx_specs must be a non-empty string list")
        )
    retention = manifest.get("artifact_retention_days")
    if not isinstance(retention, int) or not 1 <= retention <= 90:
        problems.append(Problem(relative, "artifact_retention_days must be between 1 and 90"))

    actions = manifest.get("actions")
    if not isinstance(actions, dict):
        problems.append(Problem(relative, "actions table is missing"))
        return None, problems
    cargo_deny_action = actions.get("cargo_deny")
    cargo_deny_action_sha = actions.get("cargo_deny_sha")
    download_syft = actions.get("download_syft")
    download_syft_sha = actions.get("download_syft_sha")
    upload_artifact = actions.get("upload_artifact")
    upload_artifact_sha = actions.get("upload_artifact_sha")
    for name, value in {
        "actions.cargo_deny": cargo_deny_action,
        "actions.download_syft": download_syft,
        "actions.upload_artifact": upload_artifact,
    }.items():
        if not isinstance(value, str) or not value.startswith("v"):
            problems.append(Problem(relative, f"{name} must be a pinned version tag"))
    for name, value in {
        "actions.cargo_deny_sha": cargo_deny_action_sha,
        "actions.download_syft_sha": download_syft_sha,
        "actions.upload_artifact_sha": upload_artifact_sha,
    }.items():
        if not isinstance(value, str) or re.fullmatch(r"[0-9a-f]{40}", value) is None:
            problems.append(Problem(relative, f"{name} must be a full commit SHA"))

    if problems:
        return None, problems
    return (
        ToolPolicy(
            cargo_deny=string_fields["cargo_deny"],  # type: ignore[arg-type]
            cargo_cyclonedx=string_fields["cargo_cyclonedx"],  # type: ignore[arg-type]
            syft=string_fields["syft"],  # type: ignore[arg-type]
            rust_spec=string_fields["cyclonedx_rust_spec"],  # type: ignore[arg-type]
            accepted_specs=tuple(accepted),  # type: ignore[arg-type]
            retention_days=retention,  # type: ignore[arg-type]
            cargo_deny_action=cargo_deny_action,  # type: ignore[arg-type]
            cargo_deny_action_sha=cargo_deny_action_sha,  # type: ignore[arg-type]
            download_syft_action=download_syft,  # type: ignore[arg-type]
            download_syft_action_sha=download_syft_sha,  # type: ignore[arg-type]
            upload_artifact_action=upload_artifact,  # type: ignore[arg-type]
            upload_artifact_action_sha=upload_artifact_sha,  # type: ignore[arg-type]
        ),
        [],
    )


def load_artifact_specs(path: Path) -> tuple[list[ArtifactSpec], list[Problem]]:
    """Load deployables from the shared OCI image inventory."""

    relative = path.as_posix()
    if not path.is_file():
        return [], [Problem(relative, "image manifest is missing")]
    images = _load_toml(path).get("images")
    if not isinstance(images, dict) or not images:
        return [], [Problem(relative, "images table must not be empty")]

    specs: list[ArtifactSpec] = []
    problems: list[Problem] = []
    for service, value in sorted(images.items()):
        binary = value.get("binary") if isinstance(value, dict) else None
        if not isinstance(service, str) or not isinstance(binary, str) or not binary:
            problems.append(Problem(relative, f"invalid image artifact entry: {service}"))
            continue
        specs.append(ArtifactSpec(service=service, binary=binary))
    return specs, problems


def check_deny_policy(path: Path) -> list[Problem]:
    """Ensure dependency policy remains fail-closed in security-sensitive areas."""

    relative = path.as_posix()
    if not path.is_file():
        return [Problem(relative, "cargo-deny policy is missing")]
    policy = _load_toml(path)
    problems: list[Problem] = []

    graph = policy.get("graph")
    if not isinstance(graph, dict) or graph.get("all-features") is not True:
        problems.append(Problem(relative, "graph.all-features must be true"))

    advisories = policy.get("advisories")
    expected_advisories = {
        "unmaintained": "all",
        "unsound": "all",
        "yanked": "deny",
        "ignore": [],
    }
    if not isinstance(advisories, dict):
        problems.append(Problem(relative, "advisories table is missing"))
    else:
        for name, expected in expected_advisories.items():
            if advisories.get(name) != expected:
                problems.append(Problem(relative, f"advisories.{name} must be {expected!r}"))

    licenses = policy.get("licenses")
    if not isinstance(licenses, dict):
        problems.append(Problem(relative, "licenses table is missing"))
    else:
        allowed = licenses.get("allow")
        if not isinstance(allowed, list) or set(allowed) != ALLOWED_LICENSES:
            problems.append(Problem(relative, "licenses.allow does not match the approved set"))
        confidence = licenses.get("confidence-threshold")
        if not isinstance(confidence, (float, int)) or confidence < 0.9:
            problems.append(Problem(relative, "license confidence threshold must be at least 0.9"))
        if licenses.get("include-dev") is not True or licenses.get("include-build") is not True:
            problems.append(Problem(relative, "development and build dependency licenses must be checked"))
        if licenses.get("unused-license-exception") != "deny":
            problems.append(Problem(relative, "unused license exceptions must be denied"))
        if licenses.get("exceptions") != []:
            problems.append(Problem(relative, "license exceptions require explicit review"))

    bans = policy.get("bans")
    if not isinstance(bans, dict) or bans.get("wildcards") != "deny":
        problems.append(Problem(relative, "bans.wildcards must be 'deny'"))
    elif bans.get("allow-wildcard-paths") is not True:
        problems.append(
            Problem(relative, "private workspace path dependencies must be explicitly allowed")
        )

    sources = policy.get("sources")
    if not isinstance(sources, dict):
        problems.append(Problem(relative, "sources table is missing"))
    else:
        expected_sources = {
            "unknown-registry": "deny",
            "unknown-git": "deny",
            "required-git-spec": "rev",
            "allow-registry": [CRATES_IO_INDEX],
            "allow-git": [],
        }
        for name, expected in expected_sources.items():
            if sources.get(name) != expected:
                problems.append(Problem(relative, f"sources.{name} must be {expected!r}"))
    return problems


def check_workflow(path: Path, tools: ToolPolicy) -> list[Problem]:
    """Reject drift between pinned tool policy and executable CI configuration."""

    relative = path.as_posix()
    if not path.is_file():
        return [Problem(relative, "repository quality workflow is missing")]
    workflow = path.read_text(encoding="utf-8")
    required = {
        f'CARGO_CYCLONEDX_VERSION: "{tools.cargo_cyclonedx}"': "cargo-cyclonedx version",
        f'SYFT_VERSION: "{tools.syft}"': "Syft version",
        f"EmbarkStudios/cargo-deny-action@{tools.cargo_deny_action_sha}": "cargo-deny action",
        f"# {tools.cargo_deny_action}": "cargo-deny action version annotation",
        f"anchore/sbom-action/download-syft@{tools.download_syft_action_sha}": "Syft action",
        f"# {tools.download_syft_action}": "Syft action version annotation",
        f"actions/upload-artifact@{tools.upload_artifact_action_sha}": "artifact action",
        f"# {tools.upload_artifact_action}": "artifact action version annotation",
        f"retention-days: {tools.retention_days}": "artifact retention",
        "arguments: --locked": "locked dependency policy execution",
        "scripts/generate_sboms.py rust": "Rust SBOM generation",
        "scripts/generate_sboms.py images": "image SBOM generation",
        "scripts/verify_supply_chain.py --artifacts": "SBOM validation",
    }
    return [
        Problem(relative, f"missing pinned {purpose}: {fragment}")
        for fragment, purpose in required.items()
        if fragment not in workflow
    ]


def check_sbom(path: Path, accepted_specs: tuple[str, ...]) -> list[Problem]:
    """Validate the stable envelope required of a CycloneDX JSON artifact."""

    relative = path.as_posix()
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        return [Problem(relative, f"cannot read CycloneDX JSON: {error}")]
    problems: list[Problem] = []
    if not isinstance(document, dict):
        return [Problem(relative, "SBOM root must be a JSON object")]
    if document.get("bomFormat") != "CycloneDX":
        problems.append(Problem(relative, "bomFormat must be 'CycloneDX'"))
    if document.get("specVersion") not in accepted_specs:
        problems.append(
            Problem(relative, f"specVersion must be one of {list(accepted_specs)!r}")
        )
    if document.get("version") != 1:
        problems.append(Problem(relative, "CycloneDX document version must be 1"))
    metadata = document.get("metadata")
    component = metadata.get("component") if isinstance(metadata, dict) else None
    if not isinstance(component, dict) or not isinstance(component.get("name"), str):
        problems.append(Problem(relative, "metadata.component.name is required"))
    return problems


def check_artifacts(
    directory: Path,
    specs: list[ArtifactSpec],
    tools: ToolPolicy,
) -> list[Problem]:
    """Require one source and one image SBOM for every declared deployable."""

    relative = directory.as_posix()
    if not directory.is_dir():
        return [Problem(relative, "SBOM artifact directory is missing")]
    expected = {
        filename
        for spec in specs
        for filename in (spec.source_filename, spec.image_filename)
    }
    actual = {path.name for path in directory.glob("*.cdx.json") if path.is_file()}
    problems = [
        Problem(relative, f"required SBOM is missing: {name}")
        for name in sorted(expected - actual)
    ]
    problems.extend(
        Problem(relative, f"unexpected SBOM artifact: {name}")
        for name in sorted(actual - expected)
    )
    for name in sorted(expected & actual):
        problems.extend(check_sbom(directory / name, tools.accepted_specs))
    return problems


def verify(root: Path, artifacts: Path | None = None) -> list[Problem]:
    """Verify checked-in policy and, when supplied, generated artifacts."""

    problems: list[Problem] = []
    tools, tool_problems = load_tool_policy(root / "supply-chain" / "tools.toml")
    specs, spec_problems = load_artifact_specs(root / "deploy" / "images.toml")
    problems.extend(tool_problems)
    problems.extend(spec_problems)
    problems.extend(check_deny_policy(root / "deny.toml"))
    if tools is not None:
        problems.extend(
            check_workflow(root / ".github" / "workflows" / "repository-quality.yml", tools)
        )
        if artifacts is not None:
            problems.extend(check_artifacts(artifacts, specs, tools))
    return sorted(set(problems))


def main(argv: list[str] | None = None) -> int:
    """Run the command-line verifier."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--artifacts",
        type=Path,
        help="also require and validate generated SBOMs in this directory",
    )
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="repository root to validate",
    )
    arguments = parser.parse_args(argv)
    root = arguments.root.resolve()
    artifacts = arguments.artifacts
    if artifacts is not None and not artifacts.is_absolute():
        artifacts = root / artifacts
    problems = verify(root, artifacts)
    if problems:
        for problem in problems:
            print(problem.render(), file=sys.stderr)
        print(f"FAILED: {len(problems)} supply-chain problem(s)", file=sys.stderr)
        return 1
    mode = "policy and SBOM" if artifacts is not None else "supply-chain policy"
    print(f"OK: {mode} verification passed for {root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
