# Research decision provenance

Use [`DecisionProvenance`](../../crates/contracts/src/research_decision.rs) for the
bounded `research.decision.v1` companion defined in [ADR-0100](../decisions/0100-project-versioned-research-decision-provenance.md).
It does not alter Research v1 artifacts or abstentions. Decode at byte boundaries;
raw Serde DTOs remain unvalidated. The canonical format is the existing sorted-key
JSON with one final newline, a `kind`/`data` envelope and a 16,384-byte limit.
Unknown fields/versions, alternate encodings and duplicate keys are refused.

The request pins scope/ID, implementation, configuration, cutoff and lookback.
The companion pins exact outcome digest, policy digest, explicit direction and
holding horizon, approval-profile digest and observed approval revision, and
validator revision. References identify retained owner facts; they are not URLs
or evidence that approval is still current. No revision is inferred for legacy data.

An evaluated assessment carries sorted unique eligible identities and stable
ordered reasons, with approved/narrowed/abstained eligibility. An evidence failure
instead carries not-evaluated plus the original abstention reason. No-eligible
candidate is an evaluated denial, not an evidence failure. Binding requires exact
request/outcome identity and matching terminal kind, eligible ranking or abstention
reason. Binding does not execute policy or certify formulas, account risk or
publication permission. The [independent validator](research-validation.md) remains
separate from these structural checks.

`ResearchProjection::apply_provenance` requires an independently authenticated
owner companion. Before the base outcome it returns Gap without state/progress
advance; retry after backfill. Exact duplicates are harmless; conflicting companions,
foreign anchors or wrong outcome digests refuse without mutation. Lifecycle and
provenance can arrive in either order after their base. `provenance_bytes` returns
exact bytes or None: absent means unobserved, including legacy outcomes, never
validated/approved. Provenance does not change artifact/lifecycle status or bytes.
Local revision counts one terminal, each lifecycle fact and one provenance fact.

Checkpoints retain EARP1/EARP2 exactly without provenance; EARP3 includes the
companion and uses the same checksum/anchored replay protections. The total bound
is `5 * MAX_RESEARCH_BYTES + 1024`. Checksum is integrity, not authentication or
rollback fencing. Readers must support EARP3 before producers activate. Old readers
refuse it; stop intake/serving and rebuild disposable projections with a compatible
reader on rollback, preserving authoritative owner facts.

This slice adds no publication, SQL migration, message route or HTTP field. PR12
must retain/publish provenance with validated completion and current owner checks,
and wire authenticated persistence/query adapters. Existing runtime queries expose
no new approval assertion. No dependency or external service is needed here.

Run `cargo test --locked -p edgeagent-contracts --test research_decision` and
`cargo xtask verify --check research-projection-contracts --python python3`.
The full default gate also exercises canonical bytes with `serde_json/preserve_order`.
