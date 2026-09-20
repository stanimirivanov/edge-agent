# Dependency and artifact supply chain

## TL;DR

- `Cargo.lock` is authoritative; build, lint, test, packaging, and policy checks reject lockfile drift.
- `deny.toml` rejects vulnerable, unsound, unmaintained, yanked, wildcard, unapproved-license, and unapproved-source dependencies according to explicit policy.
- CI emits a dependency-complete CycloneDX SBOM and a runtime-image CycloneDX SBOM for every deployable in `deploy/images.toml`.
- Default verification remains offline. The authoritative advisory refresh, tool installation, image builds, and artifact upload run in the supply-chain CI job.
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

## Artifact model

`deploy/images.toml` is the deployable inventory. For each declared binary, the
pipeline produces two complementary artifacts under `artifacts/sbom/`:

| Artifact | Generator | Scope |
| --- | --- | --- |
| `<binary>.source.cdx.json` | `cargo-cyclonedx` | Resolved Rust dependency graph, features, target conditions, licenses, and package identities |
| `<binary>.image.cdx.json` | Syft | Files and packages present in the final OCI runtime filesystem |

The Rust generator uses CycloneDX 1.5, strict SPDX parsing, all features, all
target-specific dependencies, and one document per binary. It derives the
timestamp from `SOURCE_DATE_EPOCH`, defaulting to the current Git commit time,
so repeated generation for one commit does not introduce a random serial or
wall-clock drift. Syft may emit CycloneDX 1.6; the verifier accepts only the
declared 1.5 and 1.6 envelopes.

Generated SBOMs are evidence artifacts, not source files. They remain ignored
under `artifacts/`, are validated before upload, and are retained by CI for 14
days. Release automation can later attach the same verified documents to signed
image digests without changing their generation boundary.

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
- A missing tool, Docker daemon, or network connection is an unavailable check,
  not a pass. Record it as not run and rely on the required CI job before merge.
- A partial artifact directory fails validation. Delete the disposable output
  directory and regenerate both artifact classes from a clean checkout.
- The generator removes its temporary local image tags even after a failed
  scan. It never pushes images or contacts a deployment environment.

Runtime image SBOMs do not replace source dependency SBOMs: statically linked
Rust binaries may expose less package detail to filesystem scanners. Neither
artifact proves build provenance or publisher identity. SLSA provenance,
keyless signing, registry attachment, and signature verification policy remain
explicit follow-up work.
