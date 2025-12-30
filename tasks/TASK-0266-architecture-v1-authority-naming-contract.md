---
title: TASK-0266 Architecture v1: authority & naming contract (single source of truth, no drift)
status: Draft
owner: @runtime
created: 2025-12-30
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Keystone gates: tasks/TRACK-KEYSTONE-GATES.md
  - DevX CLI base (`nx`): tasks/TASK-0045-devx-nx-cli-v1.md
---

## Context

We want a **coherent OS**, not a set of parallel prototypes. The fastest way to avoid drift is to make
the system’s “single authorities” and naming rules explicit and binding **before** implementation.

Repo reality today includes placeholder binaries/services with non-canonical names. Because we are still
in planning/bring-up, we do **not** promise compatibility with placeholder names; we instead lock the
end-state architecture and require implementations to match it.

## Goal

Adopt and enforce the architecture contract in `tasks/TRACK-AUTHORITY-NAMING.md` as normative:

1. **Single authority per domain** (no competing daemons for the same role).
2. **Canonical names**:
   - daemons/services use `*d` suffix,
   - libraries do not use `*d`,
   - placeholders must not be extended; implementations replace/rename/remove them.
3. **Canonical CLI**:
   - `nx` is the single CLI entrypoint; new commands land as `nx <topic> ...`.
   - optional shims (e.g. `nx-image`) may exist only as thin wrappers forwarding to `nx image`.
4. **Canonical URI schemes** and **artifact formats** are registered in one place (no silent new schemes/formats).

## Non-Goals

- Kernel changes.
- Implementing any of the services listed in the registry (this is planning/contract only).
- Rewriting old tasks wholesale; we only patch tasks where naming/authority is ambiguous.

## Constraints / invariants (hard requirements)

- **No authority drift**: if a task introduces a new service name, it must be added to `TRACK-AUTHORITY-NAMING.md`
  and must not overlap an existing authority.
- **No CLI drift**: avoid creating new `tools/nx-foo/` binaries; use `tools/nx` subcommands.
- **No format/scheme drift**: no new URI schemes and no new on-disk contracts without an explicit registry entry.
- Determinism-first: contracts must remain QEMU-proof and host-testable.

## Concrete decisions (v1)

These are the binding decisions (see registry for full list):

- `policyd` is the single policy authority.
- `samgrd` is the single service registry authority (OHOS-aligned).
- `windowd` is the single compositor/present authority (no parallel compositor daemon).
- `imed` is the canonical IME authority (no parallel `ime` daemon).
- `powerd` and `batteryd` are canonical (no `*mgr` authorities).
- `nx` is the single CLI entrypoint (no parallel `nx-*` binaries with their own logic).

## Stop conditions (Definition of Done)

Planning-only completion criteria:

- All newly created/edited tasks in the current roadmap reference `TRACK-AUTHORITY-NAMING.md`.
- Any task that previously suggested a separate `nx-*` tool has been rewritten to use `nx <topic> ...`.
- Any task that references a non-canonical authority name has an explicit “placeholder must be replaced” note.

## Touched paths (allowlist)

- `tasks/TRACK-AUTHORITY-NAMING.md`
- `tasks/TRACK-KEYSTONE-GATES.md`
- selected tasks that refer to authority/CLI naming (minimal edits only)
