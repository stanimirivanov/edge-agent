# ADR-0092: Retain exact local strategy approval authority

- Status: Accepted
- Date: 2026-10-30
- Milestone: M05 - Strategy and policy
- Deciders: EdgeAgent maintainers
- Supersedes:
- Superseded by:

## TL;DR

- Retain immutable exact legacy profiles and append installed, approved, retired states.
- Derive local operator authority from schema ownership of the authenticated database session.
- Fence governed completion against retirement; retain exact original receipt recovery.

## Context

PR2 bundles declare features not yet loaded by Research. The maintainer explicitly
selected approval of exact M04 bindings first, with full bundle adoption later.

## Decision

Use private Research profile-v1 and approval tables, with immutable rows and three
consecutive transitions. Only the existing momentum/renderer baseline can bootstrap.
Duplicate latest actions recover exactly; retirement is terminal. Record session_user
and database transaction time. Only the current schema owner's session can administer.

Expose explicit local governed execution during staged rollout. Check approval before
owner reads and again with a profile share lock through completion. Retirement takes
the definition write lock. Keep external evidence reads outside transactions. Receipt
recovery returns previously committed results regardless of current retirement.

## Alternatives considered

Full bundle enforcement would prematurely absorb later feature/policy work. A caller
actor label is not authentication. Cached approval cannot fence concurrent retirement.

## Consequences

The local baseline now has usable persistent authority without claiming policy
qualification or production separation of duties. Shared owner credentials imply
shared authority. PR12 must enforce governance on every new publication path; PR16
adds general promotion evidence. Existing M04 modes are temporarily preserved.

## Compatibility and migration

Apply additive migration 0003 explicitly; no implicit approval or artifact rewrite.
Retain authority rows on rollback and stop governed intake before using older binaries
that cannot enforce retirement. Full PR2 configuration loading remains later work.

## Security and operations

Scope, implementation and configuration bind exact immutable bytes and numeric profile
settings. No user-supplied actor is accepted. Database-owner compromise can bypass local
controls; production identities and credential separation require later qualification.

## Validation

Real isolated PostgreSQL and separate Research processes prove unapproved refusal,
immutable definitions, retry/order conflicts, terminal retirement, receipt recovery,
nonowner refusal and commit fencing. Generate SQLx offline metadata against disposable
PostgreSQL; run the canonical metadata gate when tooling is available and report gaps.
