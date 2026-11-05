# Signed OCI releases

## TL;DR

- A pushed `vMAJOR.MINOR.PATCH` or SemVer prerelease tag publishes all deployables declared in `deploy/images.toml`.
- Each Linux AMD64 image is built once, published to GHCR, addressed by digest, and never rebuilt per cloud.
- GitHub OIDC creates provenance and CycloneDX SBOM attestations; Cosign adds and immediately verifies a keyless signature.
- Release automation grants write access only to packages, attestations, artifact metadata, and the ephemeral OIDC token.
- Consumers and deployment profiles must verify the digest, workflow identity, issuer, provenance, and SBOM before promotion.

## Release contract

`supply-chain/release.toml` pins the registry, platform, Cosign version, and
the immutable commits of every release action. `deploy/images.toml` remains
the deployable inventory. `scripts/prepare_release.py` rejects non-SemVer tags
and derives the complete matrix rather than maintaining a second service list.

The Dockerfile frontend and Rust builder image use SHA-256 digests. The final
runtime remains `scratch` and runs as the declared non-root identity. The
initial release platform is intentionally `linux/amd64`; multi-architecture
manifests require a later reproducibility and runtime-conformance increment.

## Publication flow

The `Release OCI images` workflow runs only for pushed tags matching
`v*.*.*`; the planner then applies strict SemVer validation before a registry
write occurs. Before publishing, GitHub Actions must complete the repository
quality workflow and the six-workspace dependency audit successfully. Both
local workflow calls resolve from the release commit and check out the release
event's source, with read-only repository access. Failed, skipped or cancelled
prerequisites block every publish job. Use a tag on a revision containing this
gate. For each deployable the publish job:

1. builds the tagged source with digest-pinned build inputs;
2. publishes version and commit-SHA tags under
   `ghcr.io/<owner>/<repository>-<service>`;
3. retains the build output digest as the canonical artifact identity;
4. scans that exact registry digest with pinned Syft and emits CycloneDX 1.6;
5. attaches GitHub build-provenance and SBOM attestations to the digest;
6. signs the digest keylessly with Cosign and GitHub OIDC; and
7. verifies the signature and both attestation predicate types before success.

The workflow never publishes a `latest` tag. Tags are discovery aliases only;
deployment configuration must pin the verified `sha256:` digest.

## Trust and permissions

The signing certificate identity is the tagged invocation of
`.github/workflows/release-images.yml` in this repository. The accepted issuer
is `https://token.actions.githubusercontent.com`. No long-lived signing key or
registry password is stored: GHCR authentication uses the job-scoped
`GITHUB_TOKEN`, while Cosign and GitHub attestations use the job-scoped OIDC
token.

The publish job receives `contents: read`, `packages: write`,
`id-token: write`, `attestations: write`, and
`artifact-metadata: write`. Checkout credentials are not persisted. Normal
pull-request and branch workflows retain read-only permissions and cannot
publish.

## Consumer verification

Replace the example version and digest with values from a completed release:

```text
cosign verify \
  --certificate-identity "https://github.com/stanimirivanov/edge-agent/.github/workflows/release-images.yml@refs/tags/v1.2.3" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  "ghcr.io/stanimirivanov/edge-agent-gateway@sha256:<digest>"

gh attestation verify \
  "oci://ghcr.io/stanimirivanov/edge-agent-gateway@sha256:<digest>" \
  --repo stanimirivanov/edge-agent \
  --predicate-type https://slsa.dev/provenance/v1

gh attestation verify \
  "oci://ghcr.io/stanimirivanov/edge-agent-gateway@sha256:<digest>" \
  --repo stanimirivanov/edge-agent \
  --predicate-type https://cyclonedx.org/bom
```

Successful tag discovery without these checks is insufficient promotion
evidence.

## Failure, recovery, and rollback

A matrix failure can leave a published image without its full evidence set.
Such an image is not releasable. Diagnose the failed service and rerun only
failed jobs from the same tag and commit; never deploy an image until the
workflow's signature and both attestation verifications succeed.

If a tag was created for the wrong commit, do not reuse it. Publish a new SemVer
tag and treat the earlier digest as withdrawn. Rollback means selecting a prior
verified digest in deployment configuration; it never rebuilds source or moves
an old version tag.

Repository administrators must protect release tags and restrict package-write
permissions. Those external settings cannot be enforced by repository code and
remain part of repository administration.

## Current limitations

- No release has been published by this workflow yet.
- Only Linux AMD64 images are produced.
- The attached runtime SBOM complements rather than replaces the Cargo dependency SBOM retained by repository-quality CI.
- Admission control and cloud-profile promotion enforcement belong to M09.

## Native TLS roots

The common scratch runtime includes the unmodified Mozilla CA bundle from the
digest-pinned Rust/Alpine builder at `/etc/ssl/certs/ca-certificates.crt`.
Its MPL license and source notice are installed under
`/usr/share/licenses/ca-certificates/`. The bundle is updated by reviewing
the builder digest, source package and notices together. Cargo dependency license
policy applies to Rust packages; the certificate data has its own redistribution
notice in `supply-chain/licenses/`.

`cargo xtask verify image-smoke --python python3` builds a test-only derivative
of the actual MarketData image. It adds the HTTP adapter's Rust test executable,
runs as the runtime's non-root user with a read-only root filesystem, and grants
only a bounded temporary filesystem. Docker networking is disabled. Loopback TLS
peers and fresh child processes qualify the shipped native store, a generated
trusted root, an untrusted peer, hostname mismatch and missing-store refusal.
The generated roots exist only in the temporary test filesystem.

The shipped store is loaded before test roots are injected. The trusted local
handshake uses an explicit generated test root, while the untrusted case uses
the shipped store. This qualifies adapter behavior and packaging without a
public-CA endpoint, vendor request or a new production diagnostic command.
The test executable is absent from released images.
