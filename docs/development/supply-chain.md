# Dependency and artifact supply chain

## TL;DR

- `Cargo.lock` is authoritative; build, lint, test, packaging, and policy checks reject lockfile drift.
- `deny.toml` rejects vulnerable, unsound, unmaintained, yanked, wildcard, unapproved-license, and unapproved-source dependencies according to explicit policy.
- CI emits a dependency-complete CycloneDX SBOM and a runtime-image CycloneDX SBOM for every deployable in `deploy/images.toml`; repository-only tools are excluded from release artifacts.
- Default verification remains offline. The authoritative advisory refresh, tool installation, image builds, and artifact upload run in the supply-chain CI job.
- Daily maintenance and a read-only scheduled audit cover every independent Cargo workspace, including the M08 proofs.
- Provenance attestations and image signatures require a release identity and registry; they remain a separate M01 increment.

## Policy boundary

The workspace resolves dependencies only through its committed `Cargo.lock`.
All normal Cargo checks pass `--locked`; the offline metadata check fails before
compilation if a manifest would change the resolution. Path dependencies remain
valid for workspace members.

`deny.toml` applies these controls to every feature and supported build target:

- RustSec vulnerability and unsound advisories fail the build;
- unmaintained direct or transitive packages and yanked releases fail the build;
- only the declared permissive SPDX licenses are accepted;
- only crates.io is an approved registry, and Git dependencies are denied;
- wildcard version requirements are denied; and
- duplicate versions are reported for review without automatically blocking a
  justified transition.

The repository carries no standing advisory ignores, license exceptions, Git
source exceptions, or alternate registries. A necessary exception must change
both policy and verification in a focused pull request. That review must state
the package, exact version, current use, license or advisory analysis,
replacement path, owner, and removal condition. Suppressing a check only in CI
is not an accepted exception mechanism.

## Independent workspaces and dependency maintenance

The root workspace and five M08 proof workspaces intentionally have separate
resolutions. A production lock update does not update an experiment lock, and
the experiments do not share a common SDK dependency graph. Keep those
boundaries; do not force every lockfile to resolve identical versions.

[Dependabot configuration](../../.github/dependabot.yml) checks all six roots
daily, explicitly includes Cargo's indirect dependencies, and proposes
lockfile-only updates. Version updates are grouped by dependency name across
directories; security-update grouping is a separate GitHub feature. Updates
require normal review and CI; nothing auto-merges. Lockfile-only maintenance
cannot fix a version constraint that excludes a patched release. In that case,
review the affected manifests and regenerate their own locks deliberately.

The [dependency audit workflow](../../.github/workflows/dependency-audit.yml)
checks current advisories at 03:27 UTC daily and supports a no-input manual
dispatch. Its six independent jobs use the same root/experimental policy
boundaries as pull-request CI. They retain `--locked`, read-only permissions,
credential-free checkout, and fail-closed advisory checks. This lightweight
workflow does not run SDK builds, crash proofs, image builds, or SBOM generation.
GitHub controls schedule delivery; this cadence is not an execution-time SLA.

Default verification performs a source census of `[workspace]` manifests,
requires their adjacent locks, and rejects orphan locks. The semantic workflow
sensor compares discovered lock roots with Dependabot directories, the local
audit inventory, and the scheduled audit matrix, and compares experiment roots
with the M08 proof matrix. A new independent workspace must enter maintenance and verification
coverage in the same change. Workspace members, including Flawless's nested
workflow crate, do not get independent locks. These structural checks run
without contacting GitHub, crates.io, or RustSec; they do not claim fresh
advisory results or valid dependency resolution.

Run the complete current-advisory check locally with the pinned `cargo-deny`:

```text
cargo xtask verify dependency-audit
```

This profile runs the structural guards and all six locked dependency audits,
not runtime proofs or SBOM generation. Each experiment also has a named
`m08-<candidate-label>-dependency-policy` check in `cargo xtask checks`; routing
an experiment change recommends its own audit instead of the root-only audit.

These controls take effect after merging into the default branch. Maintainers
must separately enable GitHub Dependabot alerts/security updates for
security-triggered pull requests and monitor scheduled failures and blocked
updates. Repository checks cannot establish those GitHub settings or guarantee
that a patch exists. New advisories may still correctly block a pull request
before remediation is reviewed and merged.

## Artifact model

`deploy/images.toml` is the deployable inventory. For each declared binary, the
pipeline produces two complementary artifacts under `artifacts/sbom/`:

| Artifact | Generator | Scope |
| --- | --- | --- |
| `<binary>.source.cdx.json` | `cargo-cyclonedx` | Resolved Rust dependency graph, features, target conditions, licenses, and package identities |
| `<binary>.image.cdx.json` | Syft | Files and packages present in the final OCI runtime filesystem |

The Rust generator uses CycloneDX 1.5, strict SPDX parsing, all features, all
target-specific dependencies, and one document per binary. It runs
`cargo-cyclonedx` once against the workspace, publishes only the service outputs
selected by `deploy/images.toml`, and removes every transient package-adjacent
`*.cdx.json` document after success or failure. Non-deployable packages such as
`edgeagent-xtask` remain governed workspace dependencies but do not become source
or image SBOM release artifacts. The generator derives the timestamp
from `SOURCE_DATE_EPOCH`, defaulting to the current Git commit time, so repeated
generation for one commit does not introduce a random serial or wall-clock
drift. Syft emits CycloneDX 1.6 for image evidence; the verifier accepts only
the declared 1.5 source and 1.6 image envelopes. The Syft command pins
`cyclonedx-json@1.6` explicitly so a newer default cannot silently change the
image evidence contract.

Generated SBOMs are evidence artifacts, not source files. They remain ignored
under `artifacts/`, are validated before upload, and are retained by CI for 14
days. Tagged releases scan each published digest without rebuilding it and
attach the resulting image SBOM as a signed GitHub attestation.

## Commands

Install the pinned Cargo tools when running the full pipeline locally:

```text
cargo install --locked cargo-deny --version 0.20.2
cargo install --locked cargo-cyclonedx --version 0.5.9
```

Install Syft 1.52.0 through its official distribution, ensure Docker is
available, then run:

```text
make supply-chain
```

The individual stages are also available:

```text
python scripts/verify_supply_chain.py
cargo deny --locked check
cargo xtask verify dependency-audit
make sbom-rust
make sbom-images
python scripts/verify_supply_chain.py --artifacts artifacts/sbom
```

`make verify` runs the offline policy-structure and lockfile checks but does not
claim that the mutable RustSec database is current. The CI supply-chain job is
authoritative for refreshed advisories, complete SBOM generation, and image
inspection.

## Failure and recovery

- A lockfile mismatch stops before build. Regenerate `Cargo.lock`, review the
  full dependency delta, and rerun policy checks; never disable `--locked`.
- An advisory or license failure blocks merge. Upgrade, remove, or replace the
  package. Escalate a time-bounded exception only through the focused policy
  review described above.
- An advisory failure is not evidence of lockfile drift. The RustSec database
  can classify an unchanged locked version as vulnerable after a new disclosure.
  `--locked` preserves that resolution; it does not remediate it. Diagnose the
  failed step and advisory before regenerating locks. Patch every affected
  independent resolution and run `cargo xtask verify dependency-audit`; do not
  freeze the advisory database, weaken policy, or regenerate unrelated graphs.
- A missing tool, Docker daemon, or network connection is an unavailable check,
  not a pass. Record it as not run and rely on the required CI job before merge.
- A partial artifact directory fails validation. Delete the disposable output
  directory and regenerate both artifact classes from a clean checkout.
- A generated document with an unexpected CycloneDX version fails with both
  the observed and required versions. Update the declared contract deliberately
  or correct the generator; do not widen validation only to make CI pass.
- The generator removes its temporary local image tags even after a failed
  scan. It never pushes images or contacts a deployment environment.

Runtime image SBOMs do not replace source dependency SBOMs: statically linked
Rust binaries may expose less package detail to filesystem scanners. The
tagged-release workflow separately establishes build provenance and publisher
identity for the published digest. Its operational contract is documented in
[Signed OCI releases](releases.md).
