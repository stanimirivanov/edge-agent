# ADR-0063: Compose owner-authorized finite vendor qualification

- Status: Accepted
- Date: 2026-10-25
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Compose both existing daily protocols without changing their temporal meaning.
- Authenticate full immutable inputs through the independent PostgreSQL owner.
- Use durable account ownership with no timeout takeover or automatic uncertainty release.
- Recover receipts and accepted preparations before credentials or fresh acquisition.
- Require owner confirmation of exact retired inventories before deletion.
- Keep actual vendor qualification and calls outside automated verification.

## Context

ADR-0062 supplies owner approval and atomic accounting/publication guards, but no
vendor worker, complete reference loader or cleanup composition. The synthetic
worker cannot become vendor-capable by changing its source selector.

## Decision

Add separate finite qualification commands to the MarketData composition root.
An explicit canonical profile identifies one acquisition, not a capability. Read
complete bounded input from the independently owned SQL control relation. An
early-bound function prevents temporary-table substitution. Validate concrete
reference values, witnesses, selection, schedules, quality and observation identity
before acquisition. Input retries are byte-identical or conflict; retirement erases
their bytes. Rights withdrawals require durable scope denial and execution withdrawal
rather than editing immutable approved input.

Acquire a durable per-account worker generation before provider/write work. It has
no expiry or automatic takeover. Failed, cancelled or crashed runs retain this marker
and existing permit uncertainty. Only the independent owner can confirm physical
quiescence for an exact generation. Confirmation does not resolve pending calls, clear
retired account blocks or refund charges. Success releases the marker only after all
external futures returned and terminal COMMIT. PostgreSQL cannot cancel remote work:
operators must stop old processes and verify backend completion before confirmation.
Bind the claim into the ledger: reservations, preparation/inventory writes and first
publication lock/compare its generation inside their SQL transactions. Unbound or
stale execution views cannot commit effects; a previous read is not a mutation grant.

Recover receipts before input, preflight, credentials or acquisition. Recover accepted
native/precise preparations before any request. Precise recovery reconstructs canonical
observations and reruns whole-series quality with original capture time; presenting an
artifact cannot bypass quality. Stage exact preparations before PUT, then use existing
create-only storage and atomic receipt/inbox/outbox publication. Death before accepted
preparation grants no refetch authorization: incomplete traversals require owner
reconciliation and a separately admitted run. Never persist raw responses or allocate
replacement observation IDs to make recovery pass.

Keep page/call/byte limits, provider delay, account credits and cancellation in the
existing adapters/ledger. A separate bounded outbox relay publishes durable intent.
ACK only follows a confirmed terminal receipt. Default and offline commands remain
synthetic-only. No fallback provider, live broker or model integration is introduced.

Cleanup authenticates owner approval of exact retired inventory. Physical writer
quiescence is an external owner assertion, not inferred from a marker or timeout.
Delete approved descriptors, then erase inventory and private approval bytes after
confirmed deletion. Account unblocking stays separate. Restore/consumer compatibility
and licensed-copy erasure remain independently verified owner controls.

## Alternatives considered

- Self-approved files or hashes: cannot authenticate current owner authority.
- Reusing the synthetic recipe: invents references and changes offline isolation.
- Expiring leases as quiescence: cannot stop delayed provider/storage effects.
- Refetching on restart: spends quota and replaces source receipt/observation identity.
- Unchecked artifact publication: would bypass whole-series quality on recovery.

## Consequences

Conservative unresolved markers reduce availability. The finite profile supports
new observation identities, not corrections requiring a predecessor or partial-page
continuation after process death.

## Compatibility and migration

Migration 0006 precedes the new binary. Provision schema, restricted worker ACLs,
encrypted private persistent object volume and owner inputs independently; workers
never migrate or approve themselves. Never roll back to grant-ignoring binaries while
protected work can execute. Existing wire formats and synthetic commands are unchanged.

## Security and operations

Keep credentials separate from profiles, inputs and logs. SQL authentication establishes
who approved, not legal rights or physical quiescence. Current grant/scope denial checks
remain authoritative. Exact approved cleanup never grants account-unblock permission.

## Validation

Offline input/quality tests, independent LOGIN proofs, preparation recovery, existing
local protocol conformance and repository gates verify implementation. PR15C separately
records authorized live qualification/cleanup evidence; synthetic proofs certify neither
actual credentials nor legal rights. See [runtime operations](../development/qualification-runtime.md).
