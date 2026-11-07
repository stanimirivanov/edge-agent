# Four-process offline Research qualification

## TL;DR

- Build and exercise the actual Gateway, MarketData, Research and projector binaries.
- Use real PostgreSQL/NATS, independent owned schemas and invented retained evidence.
- Qualify HTTP submission/query, owner reads, broker recovery, replay and rebuild.
- The driver supplies the existing Gateway outbox relay; no Gateway relay daemon is implied.

## Qualification gate

With isolated loopback `EDGEAGENT_POSTGRES_URL` and `EDGEAGENT_NATS_URL`, run
`cargo xtask verify --check research-processes --python python3`.
The gate builds all four real binaries, resolves executable paths from Cargo build
artifacts, protects exact ignored-test discovery and runs the scenarios serially.
Missing infrastructure is not a passing result. Default verification checks the
ignored-test inventory but does not claim runtime qualification. The existing CI
local-platform job explicitly includes this gate in its discovered profile.

The process driver owns disposable Gateway, Research and projector schemas plus a
MarketData offline profile/schema, one file-backed stream and bounded durables.
Production code never reads peer tables. Fixture setup, resource observation and
fault injection use the test's own resources. All calculation, acquisition, owner
export, completion, lifecycle and query behavior comes from actual service binaries.
The driver hosts the existing Gateway outbox relay adapter to publish its exact
committed command; there is no dedicated Gateway relay command in this milestone.
This proves a process workflow, not an autonomous deployed daemon topology.

## Scenarios

The owner first seeds and acquires the invented six-session history through its
actual broker worker. Research's decision cutoff is selected afterward, preserving
capture/knowledge chronology. The later handoff reuses those exact retained bytes;
it does not relabel newly captured evidence as historically available.

The artifact scenario submits the same HTTP intent twice and verifies one durable
Gateway request. Its committed outbox command travels through the real relay,
MarketData evidence handoff and Research worker. A fixture SHARE lock permits
startup/receipt reads but blocks completion insertion. After observing the pending
write lock, kill the actual worker, verify absent completion, then release the
barrier. A fresh worker receives the original unacknowledged message after its real
60-second ACK wait and commits once. No shortened broker timer, synthetic success
or fabricated settlement substitutes for redelivery.

Gateway retrieves exact owner bytes through the live projector server. The raw
six-session endpoint return is 40502 ppm. Actual replay reproduces those bytes;
a duplicate incoming owner event creates no second completion/publication. Stop the
projector, reset only its disposable checkpoint/inbox and durable, then rebuild
from retained owner publication. Gateway first observes absence and then the same
exact result. An actual owner expiry command, relay and projection finally produce
HTTP 410 while retaining original artifact bytes.

A second four-process scenario requests more sessions than the installed owner
recipe contains and verifies a stable canonical abstention through Gateway.
The owner returns `InsufficientHistory` for this session shortfall, with the narrow
classification correction explicitly approved during PR15. Unsupported instruments
remain `UnsupportedUniverse`; malformed retained history remains `InvalidEvidence`.
Retained older abstention receipts remain immutable and keep their original reason.


## M05 policy qualification

The M05 scenario runs two isolated journeys against the same real service
boundaries. One selects a policy that admits the instrument. The other sets a
liquidity floor above the invented turnover and requires an evaluated
`LiquidityBelowMinimum` decision with a `NoEligibleCandidate` terminal result.
Both journeys use MarketData's retained owner evidence and exercise the Research
ledger and promotion CLI with invented trial and report evidence.

A fresh completion fails before approval and while the scope control is
disabled. The test checks for zero completion, decision, inbox and outbox rows
before enabling the scope. The broker worker then commits under the enabled
control revision. The actual projector serves the exact canonical decision bytes
and Gateway exposes the same decision digest in request metadata.

Replay reproduces each terminal result after the driver replaces the active
configuration with a different selection. The admitted journey then checks the
configured expiry rule at the retained cutoff. An exact retry returns the same
lifecycle event. Relay and projection advance Gateway's outcome to HTTP 410,
while the original terminal bytes and projected decision stay available for audit.

This qualifies the offline process path and the declared fixtures. The trial
and report evidence exercise ledger and approval binding rather than establish
statistical performance.

## Bounds and cleanup

Each process command is bounded, as are HTTP readiness, SQL barrier observation and
process termination. The artifact case deliberately spans the broker's real ACK
wait; the wrapper bounds the test subprocess at five minutes. Schema/profile/
stream names are unique. Scenarios stop owned servers, delete their stream/durables,
close pools and drop only owned schemas/profile roots even after a scenario panic.
No production credentials, vendor feeds, model calls or cloud resources are used.

A passing gate does not establish public authentication, current evidence rights,
production deployment, latency/throughput, autonomous surveillance or live execution.
The profile remains one trusted local operator with explicit owner authority.
This qualification extends the existing acceptance harness. Its private schemas
and streams are disposable.

## Packaged owner read

`cargo xtask verify image-smoke --python python3` also qualifies the Research
image's `offline-once` command against real retained MarketData evidence.
The Research build copies the matching MarketData executable from the same
builder output to `/edgeagent-owner/edgeagent-market-data`. Set
`EDGEAGENT_RESEARCH_OWNER_BINARY` to that absolute path. Other service images
have an empty owner directory.

The image gate owns isolated PostgreSQL and JetStream containers using the
local stack's pinned versions, random loopback ports and disposable storage.
The existing process fixture supplies Gateway admission, MarketData acquisition,
approved Research configuration and private schemas. It mounts only the invented
owner profile directory and canonical execution configuration into Research,
at their original absolute paths. The profile mount is writable for its existing
owner lock; the configuration mount is read-only. No host executable is mounted.

This Linux-only proof uses Docker host networking so the existing loopback URL
checks retain their meaning. It overrides the image's default non-root numeric
identity with the invoking non-root user's UID/GID to access the owned bind mounts.
Production identity, remote owner transport and container-network deployment
require their own profile. The Research root filesystem remains read-only,
capabilities are dropped, and privilege escalation is disabled.

The packaged owner produces the expected 40502 ppm artifact and exact retained
outcome. Missing executable input leaves no completion. A foreign acquisition
reference can refuse or produce an explicit owner-denial abstention, never an
artifact. Repeating completed input with an unavailable owner returns the same
receipt without a second completion/publication.

The protected `research-image-owner` check can also run against an explicitly
built `EDGEAGENT_RESEARCH_IMAGE`, isolated `EDGEAGENT_POSTGRES_URL` and
`EDGEAGENT_NATS_URL`. The wrapper supplies fresh host binary paths and numeric
identity, and the test owns only its schemas, streams and named Research containers.
The image-smoke supervisor removes its dependency containers and volumes on failure.
This packaged proof is separate from the original four-process crash/rebuild gate.
