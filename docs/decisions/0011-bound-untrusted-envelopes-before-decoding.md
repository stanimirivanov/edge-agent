# ADR-0011: Bound untrusted envelopes before decoding

- Status: Accepted
- Date: 2026-10-02
- Milestone: M02 - Contracts and event spine
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

Reject raw structured CloudEvents input above 256 KiB before invoking JSON or
CloudEvents parsing. Enforce the existing 512-byte visible-ASCII metadata
contract for `type` and `dataschema` on producer, decoder, and static routing
definition paths. Keep the encoded-size routing check for producer-built
envelopes.

## Context

The routing layer limited canonical encoded envelopes to 256 KiB, but the
decoder parsed an arbitrary raw byte slice first. A padded or malformed input
could therefore consume parser memory and CPU before the routing limit applied.
The metadata validator bounded most fields, but `type` and `dataschema` lacked
the same bound. Static definitions could also declare values that producer or
consumer validation would reject.

## Decision

`MessageEnvelope::from_json` compares the raw byte length with
`MAX_PORTABLE_MESSAGE_BYTES` before deserialization. It returns a distinct
`EnvelopeTooLarge` error containing only actual and maximum byte counts.
Exactly 256 KiB is permitted when the input is otherwise valid; one byte more
is rejected regardless of content. This raw limit complements, rather than
replaces, `MessageDefinition::validate_envelope`'s encoded-size check.

Both envelope construction and decoding enforce a 512-byte visible-ASCII limit
on `type` and `dataschema`. Decoding inspects these supplied strings before
the CloudEvents SDK converts schema text to a URL; path normalization must not
shrink an overlong value into compliance. This bounded metadata pass parses
JSON once before SDK decoding, with no payload retained from the first pass.
Static routing definitions enforce the same byte limits before URI parsing or
subject derivation. Schema URI and message-type syntax validation remain in
effect.

## Alternatives considered

### Rely on the broker's maximum message size

- Benefits: No library contract change.
- Costs and risks: Direct calls, replay tools, and future broker profiles may
  bypass that setting; parsing still precedes application validation.
- Reason not selected: The trust boundary belongs to the shared decoder.

### Parse first and check the re-encoded size

- Benefits: Reuses the existing routing check.
- Costs and risks: Parser work and allocation occur first, and insignificant
  whitespace can disappear during canonical encoding.
- Reason not selected: It does not bound the untrusted input.

## Consequences

### Positive

- Oversized inputs fail before JSON or CloudEvents parsing.
- Producer, consumer, and static-definition metadata constraints agree.
- Oversize diagnostics contain counts, not untrusted payload bytes.

### Negative

- Previously accepted oversized raw JSON or long type/schema metadata now
  fail. Exhaustive matches on the public Rust error enum must handle the new
  variant.

### Neutral or follow-up

- The limit bounds total raw input, not parser complexity within that input.
- Valid inbound JSON is scanned twice; the first pass retains only the two
  bounded metadata strings, and the 256 KiB raw gate bounds both passes.
- Transport adapters must retain their own broker-level frame limits and
  quarantine policy; this decision does not choose a broker-specific setting.

## Compatibility and migration

No field name, encoding, or database schema changes. Deploy consumers with the
new bound before depending on the producer validation; verify that retained
messages and replay sources fit the limit. Producers that emitted oversized
raw envelopes or metadata must replace them with bounded messages or immutable
object references. Replaying rejected historical input requires an explicit
contract migration, not bypassing the decoder. Rollback restores acceptance
of inputs this decision now rejects.

## Security and operations

Classify `EnvelopeTooLarge` and metadata-limit failures as permanent contract
failures. Quarantine the exact raw bytes only through the existing bounded,
access-controlled evidence path; never include them in the error or logs.
Operators can use the byte-count error to distinguish oversize input from
malformed JSON without exposing message content. The size gate does not replace
upstream connection, broker, or request limits.

## Validation

- Unit tests cover valid exact-boundary raw JSON, one-byte-over padded JSON,
  oversized malformed bytes, and short malformed JSON.
- Producer and decoder tests cover exact and adjacent-over metadata lengths
  for both `type` and `dataschema`, including a schema URI whose normalized
  form is shorter than its supplied text.
- Static-definition tests enforce the same limits, and the existing routing
  test retains its encoded-size rejection.
- Repository, architecture, Clippy, and workspace tests run through
  `make verify`.
