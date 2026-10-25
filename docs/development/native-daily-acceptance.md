# Native daily acceptance

## TL;DR

- PR11F-A accepts complete native-date snapshots without creating source timestamps.
- Supply frozen selection, authenticated references, explicit observation versions and quality policy.
- `freeze_native_evidence` returns a separate `NativeDailyArtifact`, not storage or publication permission.
- PR11F-B still owns scoped storage, retention and atomic terminal publication; vendor activation stays blocked.
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

The producer validates this supplied predecessor; it does not authenticate its read
or decide the globally current head. PR11F-B must apply the durable compare-and-swap
and observation fingerprint fences, preserve uncertainty and recover before repeating
work. An object digest is never permission to fetch its predecessor.

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
evidence. Existing schema bytes, goldens, admissions, terminal messages, storage APIs
and the offline worker remain unchanged. Adding the public enum variant requires
in-workspace exhaustive readers to account for it; no permissive fallback is allowed.

## Verification and operations

The default `native-daily-acceptance` gate protects six named offline tests with an
independent discovery census and mutation checks. It covers deterministic success,
gaps, changed scope/provenance, missing references, action rejection, exact threshold
boundaries, current authority, cancellation, explicit revisions and strict decoding.
`evidence-contracts` protects schema/golden freshness and old-format compatibility,
including feature-unified map order. Synthetic calendars are invented, not provider
or exchange qualification.

This foundation performs no I/O, database migration, storage operation, publication,
provider call, credential discovery or model/broker operation. PR11F-B consumes it;
PR15 remains blocked by that integration and authenticated runtime/account/rights
qualification. Reverting before activation leaves current runtime behavior unchanged
and must not delete or rewrite retained evidence. See
[ADR-0059](../decisions/0059-accept-native-daily-evidence-with-explicit-owner-lineage.md).
