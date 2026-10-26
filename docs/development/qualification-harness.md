# Opt-in daily qualification harness

## TL;DR

- `scripts/qualify_daily.py` supervises existing commands; it grants no authority.
- Default invocation validates local file pins, reports `not_run`, exits 2 and starts nothing.
- Explicit execution selects one bounded phase; never chain acquisition to cleanup.
- Command success is not live qualification, consumer acceptance or licensed-copy erasure.
- PR15C live qualification remains **not run**; this slice uses invented offline tests only.

## Owner prerequisites

Follow [runtime provisioning](qualification-runtime.md) and
[independent owner control](qualification-owner.md). Before real execution, the owner
must supply current rights, authenticated complete reference input, exact durable profile
approval, exclusive non-resetting account credits, data-only credentials, private evidence
storage, compatible consumers and restore/erasure reviews. A profile digest, report or
command-line flag supplies none of these facts. Default validation checks file integrity,
not profile semantics; the existing Rust CLI owns semantic and fresh authority checks.

Provide a trusted built `edgeagent-market-data` or `edgeagent-market-data.exe`, an absolute
private profile path and independently reviewed SHA-256 pins for both. Do not build or
download a binary from inside the harness. Keep pinned files immutable and inaccessible
to untrusted writers throughout execution: the local hash check is not protection against
a privileged process replacing a file between validation and execution.

Choose a fresh absolute report path outside the checkout, in an existing owner-private
directory. The harness creates no directories and never overwrites a report. On POSIX it
requests mode 0600; on Windows the operator must provision restrictive inherited ACLs.
Reports still contain sensitive profile/binary digests and must follow owner retention.
No paths, credentials, inherited environment values, child output or exception text enter
the report or console. No provider payload is collected as qualification evidence.

## Invocation and isolation

Run `python scripts/qualify_daily.py --help` for the fixed argument interface. Every
invocation requires `--phase`, `--profile`, `--profile-sha256`, `--binary`,
`--binary-sha256` and `--report`. Use absolute local paths and 64 lowercase hexadecimal
digests. Without `--execute-profile`, the command reads only the pinned local files and
writes a report: it does not read credential environment variables, start a child, contact
PostgreSQL/NATS or call a provider. Missing/invalid files report a blocked outcome rather
than a successful qualification.

Actual execution additionally requires `--execute-profile` equal to the exact profile
digest. Acquisition also requires `USE_MOCK` entirely absent and the existing
`EDGEAGENT_QUALIFICATION_LIVE_OPT_IN` equal to that digest. The wrapper never sets or
removes those variables for the operator. All real environment/credential handling and
owner checks remain in the existing runtime. Use independently provisioned environments
for worker versus owner cleanup; do not place owner credentials in a worker environment.
The selected environment is inherited by the one child but never retained in a report.

| Phase | Existing command | Supervisor timeout | Meaning of zero exit |
|---|---|---|---|
| `preflight` | `qualification-preflight` | 60 seconds | Current preflight command returned successfully; no permission token created. |
| `acquire` | `qualification-worker` | 420 seconds | Worker command returned successfully; it might have recovered a receipt or seen no delivery. |
| `publish` | `qualification-relay` | 300 seconds | Bounded relay returned; it might have found no work. |
| `cleanup` | `qualification-maintain` | 420 seconds | Maintenance returned; zero matching approved objects is possible. |

The fixed argument vector uses no shell, takes no arbitrary hooks or endpoint overrides,
and executes exactly one command. There is no automated phase chain, retry, account
release, refund, role provisioning or approval. The existing consumer must already contain
the intended exact command; this wrapper does not enqueue new acquisitions.

## Reports and failure recovery

`edgeagent.qualification-command-report.v1` contains phase/timeout, harness/profile/binary digests,
static status/reason, bounded return code, live-qualification/cleanup verification state
and external-effect uncertainty. Sorted JSON plus LF makes identical offline outcomes
byte-reproducible; elapsed time, host identities and payloads are deliberately excluded.
The report is reserved and flushed before validation; `running`/unknown state is flushed
before child launch. Failure to persist that state prevents launch. A crash or interrupted final
write can leave an incomplete or `running` report: never interpret either as success.

Exit 2 means default `not_run` or blocked input/intent, 1 means nonzero child exit, 3 means
unknown execution/interrupt or unrecordable report I/O, and 0 means **only** `command_succeeded`. Executed reports
always keep `live_qualification=not_verified`; cleanup execution likewise remains
`cleanup_verification=not_verified`. No report value certifies a whole qualification.
Reports and hashes are observations, never inputs accepted by an authorization API.

Child stdout/stderr/stdin are disconnected. On timeout Python terminates/waits for the
direct child; the trusted runtime has no subprocess tree for this wrapper to supervise.
That cannot stop or prove completion of remote provider/storage work. Preserve the report,
charged calls, unresolved worker generation and account blocks. Follow independent
exact-generation reconciliation before any separately authorized next invocation.
Never rerun automatically, infer quiescence from elapsed time or invoke cleanup as a
failure handler. A timeout during maintenance may leave partial deletion and requires
the existing exact-inventory recovery/approval path; it does not prove erasure.

## Evidence needed to complete PR15C

Keep actual owner evidence in the independently controlled private operational record:

1. Authorization for the exact provider/profile/window and budget, rights/data-only
   credential review, binary revision and authenticated original references.
2. Durable call/credit usage before and after the run, an actual accepted ready receipt
   for the intended command and object integrity/lineage checks. Receipt recovery or an
   empty queue cannot stand in for a newly observed provider acquisition.
3. Outbox confirmation plus authorized consumer acceptance for the exact accepted format;
   relay exit status or a progress count alone does not establish either.
4. Immediate read denial following authorized retirement/revocation, independent physical
   provider/writer quiescence, exact cleanup approval and verified object/inventory removal.
5. Licensed metadata/copy/WAL/backup/restore obligations and unresolved uncertainty,
   including any independently approved deployment-specific exceptions.

Do not commit private profiles, rights documents, provider responses, credentials or those
operational records. Missing evidence leaves qualification incomplete, not implicitly passed.
One qualified daily provider completes only the existing daily criterion, not a second
provider, streaming, live trading or M03 as a whole.

## Verification

`python -m unittest discover -s tests -p test_qualify_daily.py` runs offline supervision
proofs. `cargo xtask verify` includes these through the existing Python `harness-tests`
gate and remains mandatory. No Docker, credentials or provider access is needed for this
slice. See [the acquisition plan](plans/m03-acquisition.md) and
[ADR-0063](../decisions/0063-compose-owner-authorized-finite-vendor-qualification.md).
