# Native daily acceptance

## TL;DR

- PR11F-A accepts complete native-date snapshots without creating source timestamps.
- Supply frozen selection, authenticated references, explicit observation versions and quality policy.
- `freeze_native_evidence` returns a separate `NativeDailyArtifact`, not storage or publication permission.
- Native scoped storage, durable preparation/retention and atomic terminal publication are available as owner-composed library operations; vendor activation stays blocked.
- Run `cargo xtask verify --check native-daily-acceptance`, then the complete default gate.

## Producer inputs and quality

Resolve a `DateOnlySchedule` before acquisition and bind its configuration digest
into the native admission. Preserve the original `DateOnlyDailyArtifact`, including
its exact response hash, native labels, receipt, derivation and bucket witnesses.
Authenticate the retained response and full reference records outside these pure
functions. A canonical DTO or checksum alone proves neither authenticity nor rights.

Supply `NativeEvidenceFreeze` with one owner-assigned `ObservationVersion` per row,
an explicit row transformation and the admitted `DailyQualityPolicy`. IDs are not
derived from dates, prices or hashes. References include every requested calendar
date, point-in-time listing mappings, corporate actions and a separate authenticated,
unexpired action-completeness attestation. An empty action history alone is insufficient.

`freeze_native_evidence` rechecks current rights/provider review, cancellation,
original admission, schedule configuration, request scope, timestamps and embargo.
It reconstructs native domain observations against the frozen full rule/calendar
records without inventing `event_at`, provider observation or publication times.
Actual local receipt precision is preserved. The derivation clock remains explicit
millisecond UTC, matching the temporal contract.

`DailyQualityPolicy::check_native` requires exactly one row for each requested open
date and none for closed dates. It checks complete calendars, identity continuity,
reference freshness, supported actions, exact bucket witnesses, nonoverlap, the
existing daily profile's twelve-hour completed-bucket age, and exact consecutive
raw-close basis-point thresholds. Splits do not waive outliers; annotations do not
adjust prices. No independent-source comparison is claimed in this profile, although
the policy's source-difference threshold is retained for replay.

## Identity and corrections

Original snapshots require original observation revisions. A correction supplies an
owner-authorized predecessor artifact, with the same licensed series scope, native
semantics and date range, a distinct request and nonregressing acceptance time.
Each row retains its owner identity. Changed revisions must name the exact retained
previous revision and cannot regress receipt time. Reusing an observation revision
requires identical native row content, including temporal provenance; mapping/action
selection and row transformation remain separate derived metadata.
This initial correction profile keeps the same row membership; additions/removals
fail closed rather than inventing a lineage bridge across changed session coverage.

The pure producer validates this supplied predecessor. `TerminalProposal::ready_native`
additionally reads it through retained scoped storage and builds format-separated
native series/observation fences for the existing atomic publication transaction.
The transaction compares the exact current head and immutable raw fingerprints.
An object digest is never permission to fetch its predecessor.

## Wire format and compatibility

`edgeagent.evidence.native-daily.v1` embeds the unchanged native capture, owner
lineage, selected reference witnesses, exact quality parameters, actual acceptance
time, exclusive deletion deadline and explicit nullable predecessor digest.
Retention cannot exceed the original admission, current rights or the existing
24-hour daily-evidence ceiling. It is not retained historical knowledge.

The contract seals recursively sorted JSON plus one LF, with SHA-256 and a 4 MiB
ceiling. Decoding bounds before parsing and rejects alternate encodings, duplicate
or unknown fields, incomplete provenance, invalid chronology and lineage. Structural
validation is not a substitute for producer domain quality or authenticated authority.
The local schema and invented golden artifact have independent immutable baselines.

`DailyObject::NativeAccepted` explicitly dispatches the new format. Old capture and
precise decoders reject it; a raw native capture cannot masquerade as accepted
evidence. Existing schema bytes, goldens, admissions, terminal messages, precise
storage APIs and the offline worker remain unchanged. Adding the public enum variant requires
in-workspace exhaustive readers to account for it; no permissive fallback is allowed.

## Persistence, recovery and retirement

Compose `RetainedEvidenceStorage` with `ImmutableEvidenceStore` and
`PostgresAcquisitionLedger`. Its explicit `NativeEvidenceStorage` operations share
the precise adapter's bounded create-only/read-back mechanics; raw captures cannot
enter PUT. The unchanged `EvidenceObject` describes accepted bytes, not source-time
precision. The legacy physical `evidence/daily-v1` prefix is opaque; explicit object
format dispatch determines decoding. Precise-only/native-only readers reject the
other format. Upgrade consumers before enabling native publication.

Call `finish_acquisition` (terminal recovery) before preparation. Inside preparation,
call `recover_native(command, access)` before new provider work or identity allocation.
If present, reconstruct exact capture, owner versions, transformation and full
authenticated references from retained inputs. `ready_native` rechecks these against
the original acceptance time. It never advances that time after an uncertain PUT.
Changed bytes conflict rather than allocating another preparation. Current rights,
denial, command deadline, cancellation and access remain current checks, not historical
acceptance checks. References not sufficient to reconstruct authenticated full records
must cause abstention; checksums/DTOs alone are not authority.

The retention transaction saves exact canonical preparation bytes and pre-PUT inventory
before storage I/O. One preparation per admission bounds this to 4 MiB. A cancelled or
uncertain write may already exist; recover and retry exact bytes, never compensate
by deletion. Publication reuses the existing receipt/inbox/outbox transaction and
requires registered storage, head compare-and-swap and immutable observation guards.
Only a confirmed receipt permits ACK; a lost confirmation re-enters recovery.

Apply migration 0004 after 0001–0003 before these APIs. Scope revocation immediately
denies reads, preparation recovery and staging. Retirement cascades preparation erasure
with admission, retaining opaque denial and exact deletion inventory until separately
confirmed writer quiescence and physical cleanup. Unresolved calls remain account-blocked.
SQL deletion does not erase WAL/media/backups; cloud and downstream retention remain
separate qualification. Stop native intake before rollback; retain the additive table
and use an erasure-aware maintenance binary. Do not revive erased admissions.

## Verification and operations

The default `native-daily-acceptance` gate protects six named offline tests with an
independent discovery census and mutation checks. It covers deterministic success,
gaps, changed scope/provenance, missing references, action rejection, exact threshold
boundaries, current authority, cancellation, explicit revisions and strict decoding.
`evidence-contracts` protects schema/golden freshness and old-format compatibility,
including feature-unified map order. Synthetic calendars are invented, not provider
or exchange qualification.

`native-publication-contracts` protects preparation/uncertain-PUT recovery and current
denial/predecessor permission models. `evidence-storage-contracts` includes native
format separation, corrupt collision, I/O revocation/expiry and scoped deletion.
`native-publication-postgres` protects real isolated SQL preparation restart,
terminal rollback/ACK loss, concurrent correction and revocation/retirement erasure.
Independent discovery censuses reject missing, renamed, duplicated and extra proofs.
Run these focused gates, SQLx metadata freshness and `cargo xtask verify`.

PR15 still requires authenticated runtime/account/rights qualification and vendor
activation review. The offline worker remains precise-only. No provider, credential,
model or live broker operation is enabled. See
[ADR-0059](../decisions/0059-accept-native-daily-evidence-with-explicit-owner-lineage.md)
and [ADR-0060](../decisions/0060-persist-native-preparations-and-publish-through-shared-terminal-fences.md).
