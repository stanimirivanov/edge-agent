# Terminal acquisition publication

## TL;DR

- Commit an immutable receipt, inbox marker and reference-only outbox event together.
- Recover before acquisition; verify storage outside transactions; ACK only confirmed commits.
- Ready publication checks current local authority, deadlines, correction heads and immutable observation versions.
- No provider worker, broker activation, cloud deployment or model call is introduced.

## Application boundary

`finish_acquisition` first calls `AcquisitionPublication::recover`. A retained
receipt skips the preparation closure entirely, including provider/storage work.
Otherwise prepare outside the transaction and call `commit`. This finite operation
does not own intake, broker settlement, parallelism or retry loops.

`TerminalProposal::ready` verifies complete admission and reuses authority/quality
freezing before verified immutable storage. It checks the returned descriptor and
rechecks cancellation, expiry and local publication authority. Uncertain writes
recover under the same identity, never by compensating deletion. Rejections use
existing typed reasons; the owner classifies terminal failure after bounded retries.

`PublicationAuthority` is a trusted synchronous local policy extension, distinct
from read permission. Resolve exact admission and latest authenticated local
rights/provider-review catalogs; deny changed pins, revocations and expiry. Do not
perform remote I/O inside the transaction. This does not promise instantaneous
global revocation visibility. Runtime composition must supply real policy; no
permissive product implementation is included.

## Atomicity and recovery

The adapter locks the command row, checks retained receipts and reuses existing
`PostgresInbox` and `PostgresOutbox` in one transaction with publication guards.
The stable logical consumer is `market-data-acquisition` across all replicas.
Use PostgreSQL `READ COMMITTED`; stronger isolation can require re-entering the
whole operation after a serialization failure, never retrying an individual write.
Identical terminal retries retain original event time, correlation, causation and
bytes. Different terminal outcomes conflict. A logical retry with a new message ID
gets its own inbox marker without another event. Changed inbox bytes fail closed,
as do missing receipt/outbox evidence. A historical receipt grants no object access.

Ready requires no pending call, no durable cancellation and valid original
command/retention deadlines. Check database time after locking and before commit,
and local authority at the final decision boundary. Terminal receipts prohibit new
attempts/calls, but exact call reconciliation remains possible after rejection.
Terminal rejection/cancellation never refunds or releases an unresolved permit.

Only confirmed commit/recovery permits broker ACK. Cancelled futures and lost
responses are uncertain: re-enter with the same command and recover before doing
work. Database proofs reconnect clients and model lost confirmations; actual worker
death/broker ACK-loss qualification remains later M03 work.

## Correction and version fences

The private v1 series key binds principal/account/source/instrument, audience,
storage, endpoint, logical rights ID, semantics, listing and range. Capture/request
and quality revision do not create another correction lane. Originals require an
absent head; corrections compare exact predecessor digest and nonregressing
capture/cutoff. Different ranges still share scoped observation-version guards.

Fingerprints bind raw row identity/revision and content, including receipt time.
Selected mapping/calendar/action references and transformation revision may evolve
without rewriting raw observations and are excluded. Stable global version lock
order and bounded arrays prevent per-row unbounded queries. Conflicts roll back
all publication writes. No cross-license physical deduplication occurs. Failed
publication can leave private orphan objects for PR12 cleanup, not usable receipts.

## Deployment and verification

Apply inbox/outbox migrations and `market-data-postgres::MIGRATIONS` in the same
private schema. The second migration is additive; code rollback leaves history
inert. Do not drop receipts or rewrite outbox envelopes as rollback. PR12 owns
physical deletion/retention, PR13 runtime, and later qualification real process faults.
Existing v1 message formats are unchanged.

After runtime activation, disable acquisition ingress/dispatch before rolling back
to a binary without terminal-receipt support: the older accounting adapter cannot
enforce the new terminal stop. PR12 retention must also cover private head/version
metadata and any licensed fingerprints, not only object-store bytes.

- `cargo xtask verify`: mandatory default acceptance and census mutation checks.
- `cargo xtask verify --check acquisition-terminal-contracts`: five model proofs,
  including lost/cancelled confirmation and preparation/authority prerequisites.
- `cargo xtask verify --check acquisition-terminal-postgres`: four exact PostgreSQL
  proofs via owned `EDGEAGENT_POSTGRES_URL` schemas, including rollback after other
  writes, competing decisions/corrections and immutable-version conflicts.
- `cargo xtask verify sqlx-metadata`: canonical isolated full-schema freshness.

Diagnostics redact receipt bytes, scoped hashes, identities and prices. Sources
remain deliberately inspectable, not safe to log without review.
