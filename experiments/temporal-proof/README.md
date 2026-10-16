# Temporal M08 crash-recovery proof

Run `cargo test --locked --manifest-path experiments/temporal-proof/Cargo.toml` from the
repository root. This is an isolated Cargo workspace and does not change the
product workspace or deployable services.

The test owns and reaps an ephemeral Temporal CLI dev-server process. Before
startup, the test downloads the
official Temporal CLI `v1.8.3` tarball and verifies its SHA-256 against the
release's [published checksums][checksums]. It supports Linux x64 and Windows
x64; another platform requires an explicitly reviewed artifact hash. The
verified executable is launched directly on loopback with the default namespace
and no UI. Readiness requires a successful SDK connection including GetSystemInfo,
within the same five-second startup bound used by the SDK fixture. The process
is killed on abandoned ownership and explicitly reaped on startup failure or
normal shutdown; a local child-process test covers repeated cleanup after both
unwaited and already-completed children. The test needs network
access on each run, but no environment variables, manual CLI installation,
Docker, or separately started database.

The worker runs as a child process. Step A records one invocation and completes;
Step B writes a first-attempt marker and aborts the child. The harness observes
the process failure, starts a new worker, awaits the original workflow, asserts
that A ran exactly once, and replays the completed server history. This tests
resumption after a *recorded* activity completion. It does not establish
exactly-once external effects: a real provider request can be repeated if a
worker crashes after the provider acts but before activity completion is
recorded. Production activities still need provider idempotency keys or an
application-owned effect ledger.

## Dependency boundary

The SDK's optional `testing` feature enables `ephemeral-server`, which includes
its own ZIP downloader even when the caller supplies an existing executable.
Both SDK core 0.9.0 and 0.10.0 require `zip ^8.4` there; every published ZIP 8.x
release was yanked when this boundary was reviewed. A lock-only update cannot
select ZIP 9 across that incompatible requirement. This proof therefore leaves
`testing` disabled and keeps its existing SHA-256-verified tarball download.
No ZIP patch, advisory ignore, or SDK upgrade is needed. The resolved proof
graph excludes ZIP and its now-unused compression dependencies.

Duplicate-version warnings remain enabled. The upstream graph contains
incompatible requirements such as `base64 0.22/0.23`, `getrandom 0.2/0.3/0.4`,
`syn 2/3`, and `windows-sys 0.52/0.61`; code-generation dependencies also split
`logos`, `prettyplease`, `hashbrown`, and `foldhash`. The proof's direct Temporal
packages already use one coherent SDK release family. Do not force unrelated
major versions together, introduce unused direct dependencies to influence
resolution, or hide warnings with `skip-tree`. Reassess duplicates when their
owning SDK/code-generation dependencies change.

The existing dependency-policy gate audits all configured features and targets
before running the crash proof; the daily audit includes this independent
workspace. Run `cargo xtask verify --check m08-temporal-dependency-policy`
alongside the format, lint, and runtime proof checks. A successful locked
build or metadata resolution does not establish current yank/advisory status.

[checksums]: https://github.com/temporalio/cli/releases/download/v1.8.3/checksums.txt
