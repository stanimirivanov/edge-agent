#!/usr/bin/env python3
"""Verify immutable OCI release, attestation, and signing contracts."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import re
import sys
import tomllib

from prepare_release import ReleasePlanError, build_release_plan
from verify_supply_chain import load_artifact_specs, load_tool_policy


ACTION_REPOSITORIES = {
    "checkout": "actions/checkout",
    "setup_python": "actions/setup-python",
    "login": "docker/login-action",
    "setup_buildx": "docker/setup-buildx-action",
    "build_push": "docker/build-push-action",
    "attest": "actions/attest",
    "cosign_installer": "sigstore/cosign-installer",
}
SHA256_REFERENCE = re.compile(r"^.+@sha256:[0-9a-f]{64}$")
VERSION = re.compile(r"^v[0-9]+\.[0-9]+\.[0-9]+$")


@dataclass(frozen=True, order=True)
class Problem:
    """A stable release-contract verification failure."""

    path: str
    message: str

    def render(self) -> str:
        """Return a compiler-style failure description."""

        return f"{self.path}: {self.message}"


@dataclass(frozen=True)
class ReleasePolicy:
    """Pinned registry, identity, platform, tool, and action policy."""

    registry: str
    platform: str
    cosign: str
    oidc_issuer: str
    actions: dict[str, tuple[str, str]]


def _load_toml(path: Path) -> dict[str, object]:
    with path.open("rb") as source:
        return tomllib.load(source)


def load_release_policy(path: Path) -> tuple[ReleasePolicy | None, list[Problem]]:
    """Load release policy with a closed set of required actions."""

    relative = path.as_posix()
    if not path.is_file():
        return None, [Problem(relative, "release policy is missing")]
    manifest = _load_toml(path)
    problems: list[Problem] = []
    if manifest.get("schema_version") != 1:
        problems.append(Problem(relative, "schema_version must be 1"))

    expected_scalars = {
        "registry": "ghcr.io",
        "platform": "linux/amd64",
        "oidc_issuer": "https://token.actions.githubusercontent.com",
    }
    for name, expected in expected_scalars.items():
        if manifest.get(name) != expected:
            problems.append(Problem(relative, f"{name} must be {expected!r}"))
    cosign = manifest.get("cosign")
    if not isinstance(cosign, str) or VERSION.fullmatch(cosign) is None:
        problems.append(Problem(relative, "cosign must be a full vMAJOR.MINOR.PATCH"))

    raw_actions = manifest.get("actions")
    if not isinstance(raw_actions, dict):
        return None, problems + [Problem(relative, "actions table is missing")]
    expected_keys = {
        key
        for action in ACTION_REPOSITORIES
        for key in (action, f"{action}_sha")
    }
    if set(raw_actions) != expected_keys:
        problems.append(Problem(relative, "actions table does not match the required action set"))

    actions: dict[str, tuple[str, str]] = {}
    for action in ACTION_REPOSITORIES:
        version = raw_actions.get(action)
        sha = raw_actions.get(f"{action}_sha")
        if not isinstance(version, str) or not version.startswith("v"):
            problems.append(Problem(relative, f"actions.{action} must be a version tag"))
            continue
        if not isinstance(sha, str) or re.fullmatch(r"[0-9a-f]{40}", sha) is None:
            problems.append(Problem(relative, f"actions.{action}_sha must be a full commit SHA"))
            continue
        actions[action] = (version, sha)

    if problems or not isinstance(cosign, str):
        return None, problems
    return (
        ReleasePolicy(
            registry=expected_scalars["registry"],
            platform=expected_scalars["platform"],
            cosign=cosign,
            oidc_issuer=expected_scalars["oidc_issuer"],
            actions=actions,
        ),
        [],
    )


def check_build_inputs(root: Path) -> list[Problem]:
    """Require every externally resolved Docker build input to be immutable."""

    path = root / "deploy" / "images.toml"
    relative = path.relative_to(root).as_posix()
    manifest = _load_toml(path)
    build = manifest.get("build")
    if not isinstance(build, dict):
        return [Problem(relative, "build table is missing")]
    problems = []
    for field in ("dockerfile_frontend", "builder_image"):
        value = build.get(field)
        if not isinstance(value, str) or SHA256_REFERENCE.fullmatch(value) is None:
            problems.append(
                Problem(relative, f"build.{field} must use an immutable SHA-256 digest")
            )
    return problems


def check_workflow(
    path: Path,
    policy: ReleasePolicy,
    *,
    syft_action_sha: str,
    syft_action_version: str,
    syft_version: str,
) -> list[Problem]:
    """Reject weakened permissions, mutable inputs, and tag-based evidence."""

    relative = path.as_posix()
    if not path.is_file():
        return [Problem(relative, "release workflow is missing")]
    workflow = path.read_text(encoding="utf-8")
    required = {
        'tags: ["v*.*.*"]': "tag-only release trigger",
        "cancel-in-progress: false": "non-cancelling release concurrency",
        "persist-credentials: false": "non-persistent checkout credentials",
        "packages: write": "registry publication permission",
        "id-token: write": "OIDC signing permission",
        "attestations: write": "attestation permission",
        "artifact-metadata: write": "artifact metadata permission",
        f"REGISTRY: {policy.registry}": "pinned release registry",
        f"PLATFORM: {policy.platform}": "pinned release platform",
        f"COSIGN_VERSION: {policy.cosign}": "pinned Cosign version",
        f"SYFT_VERSION: {syft_version}": "pinned Syft version",
        "scripts/prepare_release.py": "manifest-derived release plan",
        '--github-output "$GITHUB_OUTPUT"': "GitHub step output boundary",
        "fail-fast: false": "independent deployable publication",
        "push: true": "registry publication",
        "provenance: false": "single authoritative provenance path",
        "sbom: false": "single authoritative SBOM path",
        "scripts/generate_sboms.py release": "digest-scoped release SBOM",
        "cosign sign --yes": "non-interactive keyless signature",
        "cosign verify": "signature verification",
        '--certificate-oidc-issuer "${OIDC_ISSUER}"': "OIDC issuer verification",
        "gh attestation verify": "attestation verification",
        "--predicate-type https://slsa.dev/provenance/v1": "provenance verification",
        "--predicate-type https://cyclonedx.org/bom": "SBOM verification",
    }
    problems = [
        Problem(relative, f"missing {purpose}: {fragment}")
        for fragment, purpose in required.items()
        if fragment not in workflow
    ]
    for action, repository in ACTION_REPOSITORIES.items():
        version, sha = policy.actions[action]
        fragment = f"uses: {repository}@{sha} # {version}"
        if fragment not in workflow:
            problems.append(Problem(relative, f"missing immutable action: {fragment}"))
    syft_fragment = (
        f"uses: anchore/sbom-action/download-syft@{syft_action_sha} "
        f"# {syft_action_version}"
    )
    if syft_fragment not in workflow:
        problems.append(Problem(relative, f"missing immutable action: {syft_fragment}"))
    attest_sha = policy.actions["attest"][1]
    if workflow.count(f"uses: actions/attest@{attest_sha}") != 2:
        problems.append(Problem(relative, "release must create provenance and SBOM attestations"))
    if workflow.count("push-to-registry: true") != 2:
        problems.append(Problem(relative, "both attestations must be attached to the image digest"))
    if workflow.count("timeout-minutes:") != 2:
        problems.append(Problem(relative, "release planner and publisher must have timeouts"))
    if "workflow_dispatch:" in workflow:
        problems.append(Problem(relative, "manual publication bypasses the signed tag trigger"))
    if ":latest" in workflow:
        problems.append(Problem(relative, "release workflow must not publish a latest tag"))
    return problems


def check_release_plan(root: Path, policy: ReleasePolicy) -> list[Problem]:
    """Prove the generated matrix covers the shared deployable inventory."""

    specs, spec_problems = load_artifact_specs(root / "deploy" / "images.toml")
    problems = [Problem(problem.path, problem.message) for problem in spec_problems]
    if problems:
        return problems
    try:
        version, matrix = build_release_plan(
            root,
            tag="v1.2.3-rc.1",
            repository="Example/Edge-Agent",
            registry=policy.registry,
        )
    except ReleasePlanError as error:
        return [Problem("scripts/prepare_release.py", str(error))]
    if version != "1.2.3-rc.1":
        problems.append(Problem("scripts/prepare_release.py", "release version is unstable"))
    expected = {
        (
            spec.service,
            spec.binary,
            f"{policy.registry}/example/edge-agent-{spec.service}",
        )
        for spec in specs
    }
    actual = {
        (entry.get("service"), entry.get("binary"), entry.get("image"))
        for entry in matrix.get("include", [])
    }
    if actual != expected:
        problems.append(
            Problem("scripts/prepare_release.py", "release matrix does not match deploy/images.toml")
        )
    return problems


def verify(root: Path) -> list[Problem]:
    """Verify the complete offline release contract."""

    policy, policy_problems = load_release_policy(root / "supply-chain" / "release.toml")
    problems = list(policy_problems)
    tools, tool_problems = load_tool_policy(root / "supply-chain" / "tools.toml")
    problems.extend(Problem(problem.path, problem.message) for problem in tool_problems)
    problems.extend(check_build_inputs(root))
    if policy is not None and tools is not None:
        problems.extend(check_release_plan(root, policy))
        problems.extend(
            check_workflow(
                root / ".github" / "workflows" / "release-images.yml",
                policy,
                syft_action_sha=tools.download_syft_action_sha,
                syft_action_version=tools.download_syft_action,
                syft_version=tools.syft,
            )
        )
    return sorted(set(problems))


def main(argv: list[str] | None = None) -> int:
    """Run offline OCI release verification."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
    )
    arguments = parser.parse_args(argv)
    root = arguments.root.resolve()
    problems = verify(root)
    if problems:
        for problem in problems:
            print(problem.render(), file=sys.stderr)
        print(f"FAILED: {len(problems)} release problem(s)", file=sys.stderr)
        return 1
    print(f"OK: release verification passed for {root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
