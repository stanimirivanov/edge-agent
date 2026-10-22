# Exact bounded provider HTTP

## TL;DR

- `market-data-http` is an outbound adapter foundation, not a qualified vendor or deployed worker.
- Only reviewed, scope-bound HTTPS GET endpoints are public; credentials use sensitive headers.
- Explicit admitted budgets bound streamed bytes, calls/pages and monotonic deadlines.
- Identity encoding only: redirects, proxy inheritance, automatic retries and decompression are disabled.
- Raw JSON tokens preserve exact decimals; invalid evidence is rejected without payload diagnostics.

## Trust and authority

Configure `HttpTransport` with an authenticated `AccessScope`, trusted HTTPS
endpoint, optional cursor key, optional data-only bearer credential, positive
connect milliseconds and positive cancellation-poll milliseconds. There are no
default endpoints, credentials or durations. Connect is at most 5000 ms and
cancellation polling at most 100 ms. The connect setting must equal the admitted
request's limit; a pooled client cannot silently override a lower request limit.
The future operator/configuration owner authenticates endpoint-ID-to-URL and
instrument-to-query bindings. A URL cannot prove legal authority or read-only
semantics; never configure a brokerage credential or an order endpoint.

Public construction requires HTTPS without URL userinfo or fragments. Pagination
requires the same origin and exact normalized path, unchanged fixed query pairs,
and exactly one nonempty configured cursor. Duplicate query keys, unknown extra
parameters and common credential query keys fail closed. This is not a generic
secret detector: operators must never put any credential under an arbitrary
query name. Host DNS and network policy remain operator-owned; restrict egress
to reviewed data endpoints and deny metadata/private-network targets at deployment.
Configuration is trusted, whereas pagination links are not.

TLS uses the existing pinned Rustls/ring/native-root stack, with certificate and
hostname verification and no process-global crypto-provider installation.
Unsupported native trust anchors are ignored (narrowing trust); no usable roots
fails startup. Reqwest's version-sensitive preconfigured TLS boundary is pinned
to Rustls 0.23.46 and Reqwest 0.13.5 and must be reviewed together on upgrades.
HTTP/1 uses a 32-header limit and the library's bounded header buffer. The adapter
does not enable cookies, HTTP/2, proxy inheritance, referer forwarding, TLS key
logging or verbose connection logging. Do not enable payload/header/URL-level
SDK tracing in deployment. The sole HTTP loopback seam is crate-private and
compiled only for tests; it is not a runtime feature or configuration switch.

## Resource and cancellation semantics

Start a non-cloneable `HttpSession` from an `AuthorizedAcquisition` and a trusted
UTC sample. Capture mode only is supported: a fresh HTTP read cannot satisfy a
retained historical knowledge claim. Scope must match exactly. The monotonic
session deadline preserves the remaining original command/retention deadline;
each request also has its admitted whole-request ceiling, including body reads.
The public acquisition contracts already cap these at 300 seconds and 15 seconds.

`get` sends one page, never a hidden retry or automatic traversal. It reserves
an attempt before polling I/O. Failed or dropped futures do not refund attempts.
Calls/pages and observed command bytes remain consumed for the session. Check
cancellation before dispatch, during I/O at the configured polling interval,
and before accepting a result. Dropping a future only ends local polling;
it cannot prove that the provider did not execute the request.

Reject declared oversize bodies before reading and enforce the smaller of the
response and remaining-command byte limits before appending each chunk. The
admitted ceilings remain 1 MiB/response and 8 MiB/command. `observed_bytes` includes
one overflow chunk when a streaming limit is crossed; accepted buffers never
exceed the ceiling. This is an application read/acceptance bound, not a promise
to prevent already-buffered network bytes. Bodies must be UTF-8 application/json.
Request identity encoding and reject every nonidentity Content-Encoding before
parsing. Supporting gzip or another encoding requires a reviewed, tested bounded
decompression path; enabling a dependency feature must not silently enable it.

These counters are local, not a durable quota, account lock or distributed rate
limiter. Recreating a session does not authorize resetting a command budget.
The later M03 owner must reserve/persist budgets before I/O, coordinate replicas,
honor account limits, and recheck current rights/provider catalogs before accepting
results. This adapter never holds a database transaction or implements `DailyProvider`
without real provider-specific mapping.

## Exact data and failure contracts

`JsonDocument` validates the complete bounded JSON text without materializing
numbers through `Value`/floating point. It rejects duplicate keys (including
escaped equivalents), more than 32 nested containers, more than 65,536 values,
invalid UTF-8 and trailing input. Provider DTOs must borrow `RawValue` fields.
`JsonDocument::bytes()` retains the exact response body, including outer
whitespace, for authorized source-byte checksums; never checksum a reserialized
DTO or the trimmed root token. The root and original body use bounded copies,
not a promise that total working memory equals the response-byte ceiling.
The `price`/`volume` converters support JSON numbers or numeric strings, bounded
128-byte tokens and exponent magnitudes up to 128. They use integer coefficient/
scale arithmetic and existing domain constructors. Reject invalid signs, more
than 28 normalized coefficient digits and scale above 8 (price)/6 (volume), even
if excess-scale trailing zeros could be removed. No rounding or vendor semantics
are inferred. Parsing a document alone does not validate a provider's schema,
instrument, bucket, timestamps, quality, license or historical availability.

Only 200 responses are accepted as data. 401/403 are forbidden; 408/429/500/502/
503/504 are classified transient; other statuses are invalid. Error bodies are
not read. Optional Retry-After supports integer seconds or an HTTP date relative
to the trusted UTC sample plus monotonic elapsed time, never the untrusted Date
header. Past dates mean zero, fractional milliseconds round upwards, malformed
or duplicate headers reject, and a delay that cannot fit the original deadline
is a budget failure. Classification never performs a retry or sleep. The later
durable owner must persist decisions and account for any external uncertainty.

Configuration, transport, sessions, documents and public error formatting redact
URLs, tokens and data. Real request causes retain `Error::source()` after removing
the request URL; never automatically log source chains. Policy failures invent
no underlying error. HTTP/JSON parsing failures never expose provider body text.

## Verification and dependencies

```text
cargo xtask verify --check provider-http-contracts
cargo xtask verify
```

The protected six-test census uses invented responses and ephemeral in-process
loopback servers. It exercises exact numeric tokens, nested duplicates, origin/
path/query scope, credentials, redirects, streaming and declared overflow, local
budgets, incomplete bodies, cancellation, whole/original deadlines, dropped
requests, Retry-After and rejection of a plaintext TLS peer. Tests use request
barriers and virtual time for stalled I/O, not sleeps to guess request arrival.
They do not qualify a vendor, prove a public CA chain or claim a live-provider SLA.
The feature-unified gate enables serde_json/preserve_order deliberately.
It also enables `compression-unification-test` to compile Reqwest's gzip decoder
adversarially: a valid compressed JSON body must still be rejected at headers,
without reading or decoding it. This build-only stress feature never grants
compression authority and is unnecessary for production builds.

Reuse existing pinned Reqwest, Tokio, URL, Serde JSON RawValue, HTTP-date and the
Rustls/ring/native-cert crates rather than adding a vendor SDK or decimal engine.
Enable TLS only on this adapter and blocking HTTP only on existing conformance
tooling. New hyper-rustls/platform-verifier transitive edges require the normal
dependency/license audit on the repository's supported OS targets; lockfile-only
Android/Wasm paths do not add those targets to product support. No license
allowlist change is authorized by this slice.
