# Local Gateway Research queries

## TL;DR

- Gateway GETs use the projector HTTP API, never its tables or implementation types.
- One trusted configured local operator scope governs admission and queries.
- Pending, gap and not-observed are observations, not rejection or current validity.
- Configure an optional literal loopback projector origin; absent/unavailable means 503.

## Activation and API

Keep the [Gateway admission configuration](gateway-admission.md) and prepared
Gateway schema. Optionally set `EDGEAGENT_GATEWAY_PROJECTOR_URL` to a literal HTTP
loopback origin, for example `http://127.0.0.1:8081` or `http://[::1]:8081`.
Credentials, DNS names (including localhost), paths other than `/`, query strings,
fragments, HTTPS and nonloopback IPs are rejected before startup. A malformed
explicit setting fails startup; an absent setting preserves admission-only startup
and both query routes return 503. Descriptor commands remain configuration-free.

Start the [projector internal server](projector-runtime.md) independently with the
same operator scope. No automatic topology, schema, tracking or projector startup
occurs. Gateway GETs do not require a Gateway admission row: authenticated owner
outcomes may independently establish the projector view. Gateway admission does
not automatically create a projector pending anchor. A freshly accepted request
may therefore be not-observed until trusted tracking or its terminal event arrives.

| GET path | Behavior |
|---|---|
| `/v1/research-requests/{scope}/{id}` | 200 scoped projector metadata |
| `/v1/research-requests/{scope}/{id}/outcome` | 200 exact canonical artifact/abstention record bytes |

Metadata preserves `scope`, `request_id`, `projection_revision`, `status` and
`applied_delivery_key`. Before lifecycle, projection revisions are zero/one;
status is pending/artifact/abstained/gap. The applied key is opaque evidence of the
first successful application, not a stream cursor, freshness clock or watermark.
A terminal/gap revision of one requires that key; pending has revision zero and
no key. [Lifecycle changes](research-lifecycle.md) add optional last `lifecycle` fact
and closed states at projection revision two/three; Gateway validates the owner
revision, scope/ID and state relation. Closed outcome GETs return 410 with their
status; correction digests remain metadata and are never followed automatically.

A pending outcome returns 202 `{"status":"pending"}`; a gap returns 409
`{"status":"gap"}`. Unknown IDs return 404 `{"status":"not_observed"}` on
both routes, never authoritative rejection. Absent, timed-out, inconsistent or
unavailable upstream state returns 503 `{"status":"unavailable"}`. A corrupt
projector also returns 503. No response claims current validity, entitlement or
current owner lifecycle state; a delayed projector can lag indefinitely.
All query responses disable caching with `Cache-Control: no-store`.

## Boundaries and failure behavior

The accepted local-operator trust profile still applies: all local processes act
as the configured operator. No public login, multi-user authorization, reverse
proxy, CORS or tunnel is supported. Reject Origin, nonlocal Host and foreign scope
before any projector I/O; forwarded identity and authorization headers are never
sent upstream. IDs use the existing 1–64 ASCII letter/digit/period/underscore/hyphen
token profile, with standalone `.` and `..` refused to prevent URL normalization.
Invalid IDs return 400 and scope/Host/Origin refusals return 403.

The Gateway constructs only the two fixed internal GET routes at the configured
origin. Reqwest is already pinned and license-reviewed in the workspace; this
adapter adds no package/version and borrows the existing transport's no-proxy,
no-redirect, no-retry policy. Supply the existing explicit Rustls/ring configuration
with an empty root store to survive workspace `rustls-no-provider` feature unification;
HTTPS origins remain refused and no global crypto-provider setting is changed. It uses a one-second connection timeout, four-second
request/body deadline and five-second outer query bound, HTTP/1 with at most 32
response headers, at most eight concurrent upstream requests (503 on saturation),
and two retained idle connections per upstream. No incoming
headers, query strings, credentials or response headers are forwarded. Compression
is disabled. Work occurs outside admission transactions and has no automatic retry.

Only the documented status/body combinations and `application/json` are accepted.
Metadata/error bodies are capped at 4096 bytes; canonical outcomes at 131072 bytes.
Bounds apply to declared lengths and streamed chunks. Metadata must match the
requested scope/ID and supported status/revision/key invariants. A successful
outcome must be exactly canonical Research artifact/abstention bytes bound to that
scope/ID; Gateway does not render, mutate or reinterpret numeric research facts.
Unexpected schemas, redirects, content encodings/types or malformed/foreign
records become 503 without exposing upstream diagnostics or URLs.

## Verification and rollback

`cargo xtask verify --check gateway-http --check gateway-query-contracts --python python3`
runs credential-free real loopback HTTP tests for admission compatibility, scoped
query refusal, exact artifact/abstention bytes, observation states, upstream shape/
size/timeout/saturation failures and destination restrictions. The focused gate is part of
default acceptance and protects exact test discovery. Existing real PostgreSQL
admission proofs remain `cargo xtask verify --check gateway-admission --python python3`.
These adapter tests include lifecycle metadata and 410 responses and use a controlled HTTP projector double; they do not claim a
full multi-service owner-to-Gateway acceptance run, which remains later work.

No migrations or persistent state changes are introduced. Rollback stops serving
and deploys the prior Gateway; admissions and projector state remain intact.
Unset the endpoint to return safely to admission-only operation. Public authentication remains separately reviewed work.
