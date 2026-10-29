# ADR-0083: Correlate owner-approved research evidence

- Status: Accepted
- Date: 2026-10-29
- Milestone: M04 - Deterministic research
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- New Gateway admissions request evidence from MarketData before Research evaluation.
- The offline operator binds a trusted caller scope to an already admitted owner recipe.
- Persist pending correlation before acquisition, then commit correlated result/inbox/outbox.

## Context

Gateway admission and Research completion exist independently. A caller's research
universe, digest or cutoff cannot authorize provider execution, object storage or
rights. MarketData must supply authenticated owner evidence or an explicit refusal
while surviving lost acknowledgements and process restart.

## Decision

Add a MarketData-owned fulfil-research-evidence command using the existing Research
Request record and a reference-only research-evidence-decided event. The event
contains the full original request and either the exact owner ready decision or a
stable research abstention reason. Scoped subject/partition, idempotency and
correlation bind to the canonical request digest. Source claims require trusted
outer transport provisioning; digests and references are never access grants.

The explicit offline profile selects one preseeded acquisition ID and caller scope.
Only the existing compiled recipes can restore its admitted inputs. Check the
instrument universe, complete session lookback and owner knowledge time before
acquiring. Preserve the compiled recipe bytes and their configuration fingerprints.
Callers cannot supply source/provider/storage/rights or an acquisition identity.

Persist the first request, ingress context and owner admission as pending work.
Use existing acquisition authority, immutable storage, terminal receipt and outbox
APIs. Recover the original acquisition receipt before preparing again. Commit one
correlated result with inbox/outbox in a second owner transaction. A crash between
these commits leaves recoverable pending correlation, never a false acknowledgement.
Rejections cannot carry an object. Ready references require an exact existing owner
receipt and intact publication intent; later consumers must reauthorize object use.

## Alternatives considered

Direct Research reads of MarketData tables violate ownership. Caller-selected
acquisition/provider configuration grants unsupported capability. A new provider
adapter, workflow engine, broker deduplication window or generic orchestration
framework does not improve the existing bounded offline proof.

## Consequences

Two durable owner checkpoints bridge acquisition and research correlation. Retries
preserve original command/result bytes and first causation. Same scoped ID with
changed request conflicts. Denied selection never starts acquisition. Storage,
conflict and cancellation failures are not turned into invented ready evidence.
Compiled-profile restore failure stops work rather than accepting changed authority.
The finite JetStream consumer ACKs only after durable result or confirmed quarantine.
A bounded stdin one-shot mode supports separate-process offline proofs without a broker.

## Compatibility and migration

Apply additive MarketData migration 0011 before enabling either research consumer
mode. Existing profile bootstrap remains creation-only; upgrade a stopped private
schema explicitly. New Gateway requests use the MarketData route. Retained PR7
direct-evaluation rows keep their original route/bytes and remain acknowledgeable;
they are not silently rewritten or replayed onto the new route. Provision/drain
those original commands separately when the later Research worker is available.
Existing evidence and Research v1 record schemas do not change. Rollback stops new
intake and preserves correlation, acquisition, inbox/outbox and evidence objects.

## Security and operations

This is a trusted local operator profile with existing loopback/configuration locks,
no provider credentials or activation, and no public identity interface. Scope is
configured independently of the payload. Runtime SQL is checked; no peer database
is read. Retain active correlation and both owner outbox receipts together. Existing owner
retirement atomically erases the new private correlation and inbox/outbox metadata
and retains only an opaque scoped denial. Serialize intake/completion with that
denial so a different acquisition cannot resurrect a retired request. Confirmed
opaque-denial redelivery is acknowledged without reconstructing erased evidence.
Expired or revoked evidence references confer no present authority. Operator confirmation is
still required for uncertain-write cleanup; restart does not infer quiescence.

## Validation

Protect contract/schema discovery, separate-process pending/duplicate/denial/quality
proofs and real transaction statement/deferred-commit rollback. Test wrong request
correlation and fabricated ready data, Gateway retained-route compatibility and
missing retained outbox refusal. Run existing acquisition/research persistence gates
and the default repository acceptance suite. Report broker-only proof prerequisites
separately from the PostgreSQL/stdin process evidence.
