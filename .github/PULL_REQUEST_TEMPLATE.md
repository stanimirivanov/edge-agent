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

## Verification

| Command or check | Outcome | Evidence or reason not run |
|---|---|---|
| `make verify` | | |

## Risk and operations

Describe security, market-data integrity, model trust, event delivery/replay,
dry-run execution boundaries, migration, rollout, rollback, deployment
portability, and operational effects. Write “None” only after reviewing each area.

## Review checklist

- [ ] The change delivers one coherent capability.
- [ ] Important success, rejection, and failure paths are tested.
- [ ] Market facts and model output cross explicit validation boundaries.
- [ ] Stateful event handling is idempotent under duplicates and redelivery.
- [ ] Execution changes accept only dry-run operations and introduce no live broker path.
- [ ] Public contracts and documentation changed with the implementation.
- [ ] No secrets, licensed datasets, personal data, or generated local artifacts are committed.
