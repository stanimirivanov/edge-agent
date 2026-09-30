## TL;DR

State the one outcome delivered, the highest material risk, and the verification result.

## Goal

Describe the problem and observable result.

**Issue:**
**Milestone:**

## Scope and exclusions

- Included:
- Deliberately excluded:

## Design and compatibility

Describe material decisions, assumptions, contracts, event compatibility,
service ownership, data meaning, deployment profiles, and ADRs.

- Unresolved questions:
- Known limitations:
- Follow-up work:

## Architecture

- Owning capability and package role:
- Inbound caller or adapter:
- Consumed ports and outbound adapters:
- Internal dependency edges added, removed, or reclassified:
- End-to-end behavior proved:

## Verification

| Command or check | Outcome | Evidence or reason not run |
|---|---|---|
| `cargo xtask repository` | | |
| `cargo xtask architecture` | | |
| `make verify` | | |

**Checks not run and blocker:**

**Residual risk:**

## Risk and operations

Describe security, market-data integrity, model trust, event delivery/replay,
dry-run execution boundaries, migration, rollout, rollback, deployment
portability, and operational effects. Write “None” only after reviewing each area.

## Review checklist

- [ ] The change delivers one coherent capability.
- [ ] Repository and architecture policy checks pass.
- [ ] Package ownership and dependency changes are explicit and point inward.
- [ ] Important success, rejection, and failure paths are tested.
- [ ] Market facts and model output cross explicit validation boundaries.
- [ ] Stateful event handling is idempotent under duplicates and redelivery.
- [ ] Execution changes accept only dry-run operations and introduce no live broker path.
- [ ] Public contracts and documentation changed with the implementation.
- [ ] No secrets, licensed datasets, personal data, or generated local artifacts are committed.
