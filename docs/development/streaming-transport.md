# Bounded streaming transport

## TL;DR

- PR18 is an offline-qualified WSS library, not an activated stream or daemon mode.
- Commit a durable start/generation before one TCP/TLS/HTTP upgrade attempt.
- Bound allocation, connect, idle, original lifetime, outgoing work and cancellation.
- Charge every complete frame before routing; unknown results stop the local handle.
- Local disconnect, timeout and Drop never certify provider quiescence or free slots.

## Ownership and configuration

`market-data-http` owns both HTTPS and WSS mechanics. `StreamingAuthority` is the
application-owned consumer boundary for authenticated current reference histories
and trusted owner time. Trusted composition resolves the exact admitted endpoint
ID to `WssConfig.endpoint` and authenticates the complete pinned configuration.
Constructors, hashes, synthetic catalogs and caller-provided clocks grant no live
permission. PR19 owns the provider decoder and exact subscription mapping; PR20
owns supervision, reference refresh/restore fences, recovery and readiness.

Configuration has no operational defaults. Explicitly select polling (1..100 ms),
HTTP upgrade buffer (8192..65536 bytes), aggregate fragmented-message bytes
(1..4 MiB), attempted writes and total outgoing payload bytes (1..4 MiB).
The admitted session supplies connect/idle, frame, total byte/frame and lifetime
limits. Per-frame payload must fit the configured message ceiling. Outgoing
allowances apply to one connection generation; the durable session/account start
ceilings bound the number of generations. Vendor activation must review actual
shared provider entitlements; these are resource limits, not billing certification.

## Transport and accounting order

1. Check sealed session/scope/current authority before reserving. Commit the
   existing ledger's start/generation before any dial. Unknown reserve results
   prevent dialing. No automatic reconnect, retry, refund or release API exists.
2. Resolve DNS and choose only the first address; attempt one TCP connection.
   Verify TLS certificates and hostname using native roots and ring. No proxy,
   redirect, compression, cookies, client certificate or SSL key logging is used.
3. Bound TCP/TLS/HTTP upgrade together by admitted connect time and original
   remaining session lifetime. The HTTP parser bounds receive allocation/headers.
   Verify status, upgrade headers and the exact nonce-derived accept key. Reject
   unsolicited extensions and subprotocols. No public configurable trust bypass
   exists; tests privately inject an ephemeral CA for loopback peers.
4. Read one codec-bounded raw frame. Disable automatic control replies and implicit
   fragment collection. Reject oversized advertised lengths before payload
   allocation. A fixed 8192-byte codec read buffer plus bounded growth/read-ahead,
   TLS and HTTP buffers remain; the payload cap is not a whole-process memory SLA.
5. Confirm its exact session-local ordinal and actual payload bytes in the durable
   ledger before exposing bytes. Controls, empty frames, duplicates, fragments and
   unselected symbols all count. Payload byte counters exclude TLS/HTTP/WebSocket
   framing. Invalid/oversized headers rejected before a complete arrival do not
   produce fabricated received-byte counters; the start remains spent/unresolved.
6. Verify frame mask/control shape, fragment ordering and aggregate message bytes.
   Return raw charged fragments without reassembly or provider deserialization.
   The provider decoder must validate complete-message UTF-8 and its wire schema
   before routing observations. Fragments are not separately usable messages.
7. A ping requires an exact bounded pong through `send` before further intake;
   debit attempted writes before I/O. Protocol replies consume the same outgoing
   allocation. A peer close stops locally without retaining its reason in logs.

The final in-budget frame at an exact cap can be returned, but the socket is then
closed. A received over-budget frame never reaches the decoder. Fragment progress
and control traffic cannot extend the absolute session deadline or market freshness.
Idle is anchored at upgrade and each complete received frame, including controls.
Caller pauses, outgoing writes, partial reads and accounting waits cannot renew it.
No background intake queue is created; idle expiry is enforced before the next I/O
or by an in-flight operation timer, not by a detached socket-reaper task.

## Failure, recovery and rollout

Cancellation and current authority are checked before/after I/O and at the explicit
poll interval. The original monotonic deadline never expands; backward trusted UTC
samples fail closed. Any failed or abandoned send/receive consumes the socket,
including unknown ledger commits. The scoped HTTP driver aborts with its future;
no detached driver survives handshake cancellation. DNS resolver internals and OS
buffers may finish after cancellation; no local result proves provider quiescence.

Reconcile uncertain accounting/start ownership through the existing independent
owner, not by replaying a local socket operation. Callers must preserve the
distinct `Accounting` (storage/unresolved), `Authority`,
`Cancelled`, `Timeout` and `Budget` outcomes; uncertainty is not confirmed denial.
PR20 must obtain fresh authority
and a new durable generation for any approved reconnect, preserving usage. No
timeout, EOF, cancellation, Drop or successful close can independently release
account capacity. This slice adds no migrations, runtime mode, vendor activation,
licensed fixtures or model calls. Rollback stops using the additive API without
deleting durable reservations or resetting counters. Daily contracts stay unchanged.

## Verification and dependencies

Run `cargo xtask verify --check wss-transport-contracts`, then `cargo xtask verify`.
The protected census covers synthetic TLS success/trust/hostname faults, invalid
and oversized upgrades, frame/fragment/control ceilings, accounting confirmation
loss, cancellation, abandonment, outgoing budgets and controlled-clock deadlines.
Tests generate ephemeral certificates in memory and bind only loopback ephemeral
ports; no Docker, vendor access or credentials are required. These proofs do not
qualify any source, cloud platform, physical provider disconnect or evidence readiness.
The existing HTTP crate-wide census also owns these nine cases under JSON-order
and compression-feature unification; no WSS skip filter narrows that proof.

The adapter privately pins Apache-2.0 `fastwebsockets` 0.10.0 (default features off)
for raw RFC6455 frames, without its message collector, SIMD or HTTP-upgrade helper.
Its [source](https://github.com/denoland/fastwebsockets) and license were reviewed;
our wrapper supplies accept-key validation and server-mask rejection. Existing
MIT Hyper/Hyper-util/HTTP-body-util and MIT OR Apache-2.0 Tokio-rustls are pinned
as direct minimal-feature dependencies, reusing the resolved HTTP/TLS stack.
MIT OR Apache-2.0 Base64 and SHA-1 are used only for RFC6455's prescribed challenge,
never artifact authentication/digests. Test-only MIT OR Apache-2.0 `rcgen` 0.14.10
(ring, defaults off) generates disposable in-memory certificates. Review current
advisories with `cargo xtask verify dependency-audit`; maintenance is not inferred
from a passing build. All SDK types stay private and the transport module plus
protected proofs are the replacement boundary. No dependency enters domain policy.
