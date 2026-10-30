# Owner-bound policy evidence

## TL;DR

- The opt-in policy reader retains exact daily OHLC/volume and complete lineage.
- `policy-history-v1` declares an explicitly invented complete event calendar.
- All previous recipes return event coverage `unknown`, never an inferred empty calendar.

## Export and trust

`offline-research-policy-read <owner-acquisition-id>` accepts the same correlated
owner-ready envelope and operator scope as `offline-research-read`. It uses the same
retained correlation export, owner lock, restored admission, rights and immutable
object read. Revocation or retention denial still returns 77 without bytes. Scope,
owner acquisition, daily digest and receipt linkage must agree. No client URL or peer
private-table access is added. Existing daily reader output remains unchanged.

The [companion contract](../../crates/contracts/src/policy_evidence.rs) embeds the
complete sealed daily artifact, original Research request and explicit event coverage.
Prices remain exact USD/share decimals, volume remains reported shares; calendar,
source, mapping, action/quality references, event/receipt/capture times and rights
scope remain in those exact daily bytes. No derived feature or policy decision is
created. Canonical bounded bytes reject unknown fields, duplicate keys, changed
request/digest and future knowledge. Request cutoff must precede retention expiry.
Coverage uses half-open Unix-millisecond intervals and a sorted bounded event census.
A complete interval containing decision time does not prove it covers a later policy's
entire exclusion window; the policy consumer must check that window explicitly.

Research's opt-in `OwnerLoader::load_policy` uses the installed owner binary, bounds
its output to daily capacity plus 64 KiB, rechecks retention around the read and binds
exact request/daily lineage. `convert_policy` alone does not authenticate bytes.
Existing close-only workers and replay do not automatically consume the companion;
feature conversion and policy enforcement belong to later M05 slices.

## Synthetic coverage and compatibility

The dedicated `policy-history-v1` seed recipe has six invented daily sessions and an
explicit empty synthetic event calendar from the first daily bucket through retention.
Its known-at time is admission. The [event specification](../../services/market-data/fixtures/offline-policy-history-v1/events.json)
bytes enter the new recipe fingerprint and their digest identifies coverage revision.
This is affirmative invented coverage, not a claim about real earnings/news or licensed
provider events. Missing coverage always remains unknown for other recipes.

The previous history recipe source is frozen byte-for-byte in
[the retained snapshot](../../services/market-data/fixtures/offline-history-v1/fixture.rs),
as the earlier single-session recipe already was. Existing admission fingerprints
therefore restore under the new binary. Their daily artifact bytes and evidence
meaning are unchanged. Source changes require explicit new recipe/revision review.
See [ADR-0094](../decisions/0094-export-owner-bound-policy-evidence.md).

## Verification and rollback

Run `cargo test --locked -p edgeagent-contracts --test policy_evidence`,
`cargo test --locked -p edgeagent-research --test policy_evidence`, and
`cargo xtask verify --check research-evidence-processes --python python3` with isolated
PostgreSQL. Tests cover exact daily bytes, binding, chronology, coverage, legacy
fingerprint retention, repeated reads and actual owner revocation. Process fixtures
use an explicit future synthetic decision cutoff so capture does not masquerade as
historically available evidence. Run the full default gate as well.

No database migration, new dependency, live feed or broker route is added. Deploy the
owner reader before opting Research into policy reads. Rolling back removes support
for new policy-history acquisitions; retain the newer owner binary for their recovery,
or quiesce them before rollback. Legacy acquisitions remain supported in either binary.
