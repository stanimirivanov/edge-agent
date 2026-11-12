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
Dependency startup and port-discovery failures report the failed operation and
exit status with the final 4000 characters of captured Docker stderr. Timeouts
retain bounded partial diagnostics, including byte output. Cleanup still runs
for every owned dependency, including a container whose startup failed.

This packaged proof is separate from the original four-process crash/rebuild gate.

## M06 explanation qualification

The protected `research-processes` gate includes three named journeys under
`process_explanations`: `scheduled_fallback_queries_and_replay_preserve_original_artifact`,
`controlled_generation_queries_and_revocation_preserve_original_artifact`, and
`controlled_injection_is_rejected_and_falls_back_through_queries`.

All three submit through Gateway, acquire actual invented MarketData evidence,
publish the original Research artifact, and admit a durable explanation job
through the real scheduler consumer. The production Research worker exercises
its generation-disabled deterministic fallback path. A separate host-only Cargo
example, `qualify_explanation_process`, exercises controlled generation and
injection/fallback. The process wrapper builds that example and obtains its
executable path from Cargo artifacts alongside the four service binaries.

The example requires explicit `EDGEAGENT_SYNTHETIC_EXPLANATION_PROOF=1`, the
owned invented-fixture marker, synthetic provider configuration, a fresh exact
job, and fixed invented deployment identities. It independently replays retained
Research using actual owner evidence, spends a durable attempt under an installed
Research model-disclosure grant, and uses the existing bounded HTTP model adapter
against the process driver's owned synthetic loopback server. Valid output selects a
supported presentation variant. An injected instruction field is rejected and
falls back without retaining the rejected response. The example commits through
the existing sealed validation and atomic completion APIs.

The owned fixture explicitly asserts invented-data model-disclosure and completion
owner rights. This is a test seam, not MarketData's production model-disclosure
authority or a real-runtime/weights attestation. Actual serving checks still run
through both Research and MarketData processes. These integration results do not
qualify model quality, license rights, real deployment identity or production
model-disclosure policy. Production model generation remains disabled; no runtime
flag, public service command or container-image executable is added for it.

The real relay/projector path exposes the separately bound companion through
Gateway while preserving exact original artifact bytes. Each journey recovers the
same terminal receipt, verifies one companion/outbox intent, performs read-only
replay, and checks unchanged retained rows. Controlled cases observe one actual
HTTP model request; the server remains live across receipt recovery and replay,
which add none. Revoking MarketData's
serving review after projection creation denies the next explanation query while
the original artifact remains available. Missing synthetic opt-in refuses before
spending an attempt. The rejected injection sentinel cannot appear in retained
Research, explanation or outbox rows. The crash/rebuild matrix remains M06-PR20.

## M06 crash, redelivery and rebuild qualification

The protected gate also runs the three `process_explanations::recovery` proofs:

- `death_at_reservation_request_response_and_commit_recovers_one_terminal`
  kills actual qualification processes after durable reservation, during an
  observed HTTP request, after validated response, during a blocked PostgreSQL
  completion commit, and after committed completion before stdout delivery.
- `late_model_response_cannot_replace_recovered_fallback` keeps a real HTTP
  response pending past the retained five-second attempt deadline. Another
  Research process commits uncertain-attempt fallback. The server then returns
  a valid response within the fixture's longer HTTP timeout; the fixture observes
  successful validation before allowing the late commit, which is refused.
- `relay_death_duplicate_delivery_and_rebuild_preserve_current_authorization`
  receives and queries the actual broker publication while mark-published is
  blocked, kills the relay, waits its real 30-second lease, and restarts it.
  An additional publish without deduplication headers exercises actual duplicate
  delivery. Rebuilding the disposable projector from all retained deliveries
  restores exact checkpoint bytes, preserves current owner denial, and returns
  identical companion bytes after a new owner approval.

Observation files and response holding belong only to the explicitly opted-in
synthetic fixture. PostgreSQL barriers are installed only in owned disposable
schemas. The tests observe ready states before killing; exceptions do not stand
in for process death, and clocks, deadlines and leases are not rewritten. Losing
an invocation during COMMIT can leave its outcome ambiguous: recovery accepts the
first durable generated result or one uncertain fallback, never partial terminal
state or an additional attempt. A killed pre-send reservation remains spent even
when the live model witness observed zero requests.

Each recovery verifies one attempt, one terminal companion, one explanation
publication intent, unchanged original artifact bytes, and no extra model request.
The late result cannot alter retained rows. Relay retry is at least once; broker
publish deduplication does not establish exactly-once external execution. The
projector restart retains its original Research authority URL and audience so
missing configuration cannot masquerade as preserved revocation. Production
model generation and the PR19 synthetic-rights/identity limitations are unchanged.
