# ADR-0067: Bound WSS frames before provider decoding

- Status: Accepted
- Date: 2026-10-26
- Milestone: M03 - Market data fabric
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Add verified, bounded WSS mechanics to the existing transport adapter.
- Reserve before dial and confirm every complete frame before exposure.
- Keep fragments/controls visible instead of hiding them in a message collector.
- Stop on uncertainty; activate no vendor, runtime mode or evidence readiness.

## Context

Finite durable sessions and capture publication do not bound a WebSocket reader's
allocation, handshake, partial reads or implicit control processing. Message-level
collectors can obscure frame arrivals and bypass the existing accounting meaning.

## Decision

Use private fastwebsockets frame parsing, Hyper's bounded HTTP/1 upgrade and the
existing verified ring/native-root Rustls stack. Validate the nonce-derived accept
key explicitly; reject extensions, subprotocols, redirect/proxy authority, masked
server frames and malformed controls. No public trust bypass is exposed.

Require explicit upgrade/message/outgoing/poll limits plus the admitted session's
frame/connect/idle/lifetime limits. Reserve a durable start/generation before one
resolved-address TCP attempt. Recheck current authenticated catalogs and trusted
time through an application-owned boundary. Bound every operation by original
monotonic lifetime and operation time; partial progress cannot renew either.

Confirm every complete frame's ordinal and payload bytes before exposing it. Keep
raw fragment ordering/aggregate size bounded without reassembly; the provider
decoder validates complete-message text/schema. Controls and unselected data count.
Disable automatic protocol replies: an owed exact pong uses the outgoing budget.
Malformed oversized headers may terminate before a complete measurable arrival;
do not charge advertised unread bytes or refund the reserved start.

Every failure or abandoned operation consumes its local socket. No recovery or
automatic reconnect exists in the transport. Unknown accounting remains unknown;
only independent exact-generation quiescence can release account capacity.

## Alternatives considered

- Message collector: rejected because fragment/control arrivals disappear.
- Custom WebSocket parser: rejected in favor of an established frame codec.
- Unbounded default SDK configuration: rejected; limits are explicit and finite.
- Implicit retries, proxy or redirect following: rejected as authority expansion.
- Vendor/runtime composition now: remains PR19/PR20 and separately reviewed activation.

## Consequences

Raw charged frames require a provider decoder to maintain complete-message semantics;
they are not accepted observations or readiness. The wrapper owns the library's
caller-side upgrade/mask checks and conservative terminal cancellation behavior.
Local reads can prefetch into bounded codec/TLS/HTTP buffers, not a global memory SLA.
Resource limits are not provider billing or shared-entitlement certification.

## Compatibility and migration

Additive Rust APIs and private transport dependencies only; no migration or event
change. Existing HTTPS and daily bytes retain their meaning. Rollback stops new
transport use, leaving durable unresolved starts and counters intact.

## Security and operations

No vendor endpoint is dialed in verification. Synthetic loopback TLS peers use
ephemeral in-memory certificates; public construction uses verified native roots.
Payload/credential/endpoint diagnostics are redacted; explicit source-chain access
remains privileged. No new daemon mode, credentials, model calls or quiescence claims.
The [transport guide](../development/streaming-transport.md) records dependency
review, operations, limits and the PR19/PR20 replacement/composition boundaries.

## Validation

Default protected named tests and census mutation/route tests verify local TLS and
upgrade rejection, frame/fragment/control budgets, unknown accounting, abandoned
operations and controlled-clock deadlines. Full default verification and dependency
audits remain required; no synthetic proof constitutes live-source qualification.
