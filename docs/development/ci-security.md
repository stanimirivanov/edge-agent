# CI security and secret detection

## TL;DR

- `cargo xtask workflow-security` checks parsed GitHub workflow policy without
  a scanner, token, database, or network.
- The default verification gate checks workflow and secret-detector policy;
  it does not install Gitleaks or claim that source and history are secret-free.
- Install the checksum-verified Gitleaks 8.30.1 binary explicitly, then run
  `cargo xtask verify ci-security` for the real-scanner proofs and repository scan.
- Preserve upstream detectors and the exact public local-fixture exceptions.
  Do not add baselines, fingerprint ignores, or broad path suppressions.
- Rotate or revoke suspected exposed credentials and use private reporting;
  removing a value from the current tree does not remove historical exposure.

## Scope and ownership

This M01 harness protects reviewed CI capabilities and detects recognizable
credential patterns in repository source and locally reachable Git history.
[ADR-0021](../decisions/0021-enforce-ci-security-and-secret-scanning.md) records
the decision. [SECURITY.md](../../SECURITY.md) owns private reporting and the
project threat model; the [coding harness](harness.md) owns feedback tiers.

The checks do not prove absence of every secret, personal data, licensed
payload, prompt injection, or unsafe runtime capability. Regex detection is
not privacy or licensing certification. Dry-run domain and image enforcement
remain separate M07 work. No model call, cloud credential, or live-trading
capability belongs to this gate.

## Workflow capability policy

`cargo xtask workflow-security` parses each reviewed workflow as YAML 1.2
with the tooling-only `yaml-rust2` dependency. It checks semantic fields,
not comments, indentation patterns, or a substring claiming that a check ran.
Malformed mappings, duplicate keys, multiple documents, aliases, anchors,
merge keys, and explicit tags fail closed. Input size and nesting are bounded.
Diagnostics identify a policy location without printing scalar values or
parser output that could contain credentials.

The exact workflow inventory and action-to-commit map live in
[workflow security policy](../../crates/xtask/src/workflow_security/policy.rs).
An action must match its reviewed full commit SHA and allowed job. A tag that
resolves to the same code still fails; an arbitrary 40-character SHA is not an
approved action. Review updates to the action map together with every affected
workflow and its negative tests.

Validation workflows use read-only `contents` permissions. Checkout disables
credential persistence and cannot override its token, repository, ref, SSH,
or PR protection. The scanner checkout uses `fetch-depth: 0`; a shallow
history cannot satisfy the historical scan. Unreviewed workflows, privileged
triggers, reusable workflows, runners, credentials, and optional validation
steps require an explicit policy change, not a convenient bypass.

Only the tag-triggered `release-images.yml` publish job grants write capability:

| Permission | Purpose |
|:--|:--|
| `contents: read` | Read tagged source and verify repository evidence |
| `packages: write` | Publish images and signatures to GHCR |
| `id-token: write` | Obtain short-lived identity for keyless signing and attestations |
| `attestations: write` | Create provenance and SBOM attestations |
| `artifact-metadata: write` | Record attested artifact identity |

The release exception admits the GitHub token only at the reviewed registry
login and attestation-verification boundaries. It does not grant PR validation
secrets or allow inherited secret bundles. Validation and mandatory scanner
steps cannot use conditions, `continue-on-error`, or command suffixes that
mask failure. The narrow failure-diagnostic and always-run Compose cleanup
steps remain allowed; security hardening must not remove resource cleanup.

Repository-quality runs the workflow sensor before other repository-owned
checks. Its secret-detection job then gates heavier verification, container,
local-platform, and supply-chain jobs. Branch protection is GitHub-side state:
maintainers must add the `Secret scanning` check to the applicable ruleset or
branch-protection requirements alongside existing quality checks. A local
policy sensor cannot establish that setting or stop an authorized reviewer
changing policy.

## Pinned scanner installation

[Security tool policy](../../supply-chain/security-tools.toml) pins the exact
MIT-licensed Gitleaks release and SHA-256 digests of the official Linux x64 and
Windows x64 archives. The Python-standard-library installer verifies the
archive before extraction, selects only the expected executable, and installs
it into the ignored `artifacts/security-tools/gitleaks-8.30.1/` directory.
Other operating systems or architectures fail explicitly rather than using an
unreviewed binary. The scanner uses no licensed GitHub Action or license token.

From the repository root:

```text
python scripts/install_gitleaks.py
cargo xtask verify ci-security
```

Installation requires network access to the official release asset. The
verification profile itself runs offline against local files and Git refs.
The installer prints the executable path and, in GitHub Actions, appends its
directory to the runner-provided `GITHUB_PATH`. `--destination PATH` selects
another install directory; direct scanner invocation accepts `--gitleaks PATH`
as one executable path, never a shell command. It still checks the exact version.
The wrapper and runner discover the default repository-local installation
before searching `PATH`; an alternate destination must be made discoverable
through `PATH` for the canonical runner.
Use `--python` on the xtask runner when Python has a different executable name.

The default acceptance gate never installs or downloads the scanner. The
runner reports a missing scanner as **not run** and exits unsuccessfully for
an incomplete selected profile. Tool availability is not detection evidence.

## Detection and exception policy

[.gitleaks.toml](../../.gitleaks.toml) inherits every upstream default detector
and adds a rule for literal EdgeAgent PostgreSQL and object-store credentials,
including low-entropy values that a generic detector may skip.

The only custom-rule exception requires both an exact canonical fixture path,
`deploy/local/.env.example`, and one of the public local-only fixture values.
A changed credential at that path and the same fixture credential at another
path must fail. This is not a directory allowance or production configuration.
The inherited `generic-api-key` rule also exempts the public synthetic
idempotency key `cancellation-order-01` only in
`crates/outbox-relay/tests/cancellation.rs`. Its detector remains enabled;
changed values, other files, and nested lookalike paths are not exempt.
The detector policy sensor rejects drift, global ignores, and fingerprint
ignore files. Scanner invocation disables inline `gitleaks:allow` suppression,
uses an empty ignore source, and discards ambient Gitleaks and Git routing
settings so external configuration cannot redirect the source or history scan.

The source scan includes tracked current files and nonignored untracked files;
it does not recursively scan build output, ignored local secrets, or caches.
The independent Git scan checks all locally reachable refs and full history,
including removed values and separate merge-commit diffs. A real-scanner proof
introduces a token only during a merge and removes it afterward; the historical
scan must still detect it. Both scans require successful completion. Empty or
shallow repositories, unsafe snapshot paths, symlinks, exceeded resource
bounds, malformed reports, and scanner errors cannot yield a passing result.
Remote branches not fetched locally and unreachable Git objects are not
claimed as inspected.

## Verification evidence

The executable inventory owns exact argv and order. Inspect it with
`cargo xtask checks`; focused checks establish only their stated boundary.

| Check | Tier | Evidence |
|:--|:--|:--|
| `workflow-security` | T1, default gate | Parsed workflow capability policy and mutation-tested rejection paths |
| `secret-policy` | T1, default gate | Exact pinned tool and detector configuration; no scanner execution claimed |
| `secret-scanner-contracts` | T3, `ci-security` | Actual pinned scanner detects synthetic secrets, rejects inline suppression, preserves only exact fixture exceptions, and detects a removed historical token |
| `secret-scan` | T3, `ci-security` | Current Git source snapshot and complete locally reachable Git history finish without detector findings |

The direct commands are useful for diagnosis:

```text
python scripts/verify_secrets.py --policy-only
python scripts/verify_secrets.py --self-test
python scripts/verify_secrets.py
```

`--self-test` creates isolated temporary files and Git history, never commits a
test credential to the contributor's checkout, and removes its temporary
evidence. Python tests cover configuration, installer authentication, report
validation, and redaction without requiring a scanner or download. Rust
mutation tests cover action pins, permissions, checkout boundaries, triggers,
credentials, and removal or bypass of mandatory gates. Run the complete
`cargo xtask verify` default gate as well as the specialized profile before
handoff when these boundaries change.

## Findings, recovery, and rollout

Scanner stdout and stderr remain private to the wrapper. Temporary reports
request full redaction; public diagnostics retain only safe rule identifiers,
a hashed file identifier, line number, and bounded commit identity. Matches,
raw filenames, authors, commit messages, and secret-bearing report artifacts
are not printed or uploaded. A successful scan is evidence for this detector
and scope, not proof that diagnostic sources elsewhere are safe.

If a real credential may have been exposed, revoke or rotate it first, follow
[private reporting](../../SECURITY.md#reporting-a-vulnerability), and resolve
the finding privately. Do not paste the value into an issue or add a baseline
to make CI green. A history-only finding remains actionable after current-file
cleanup. Maintainers must review any history remediation and its operational
impact; the harness never rewrites Git history automatically.

Tool upgrades change release pins, authenticated digests, detector expectations,
real-scanner proofs, and documentation together. Rollback reverts the complete
policy/tool slice; it must not silently relax checkout privileges or suppress
findings. An unavailable release asset, failed checksum, missing executable,
timeout, or incomplete scan is an explicit blocker, never a passing fallback.
