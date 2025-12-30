---
title: TASK-0268 DevX v1: nx CLI convergence (subcommands only, eliminate nx-* tool drift)
status: Draft
owner: @devx
created: 2025-12-30
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Authority & naming registry: tasks/TRACK-AUTHORITY-NAMING.md
  - Authority decision (binding): tasks/TASK-0266-architecture-v1-authority-naming-contract.md
  - Base nx CLI: tasks/TASK-0045-devx-nx-cli-v1.md
---

## Context

The planning direction is “one CLI”: `nx <topic> ...`.

Repo/task reality still contains many references to `nx-foo` and/or separate `tools/nx-foo/` binaries.
Even if they are just stubs, that creates long-term drift: duplicated parsing, inconsistent output,
and inconsistent proof markers.

This task closes “CLI drift” as a keystone-quality integration issue: we converge on a single `nx` entrypoint.

## Goal

Make `nx` the **only** CLI binary for Nexus OS development tooling:

1. **Subcommands only**:
   - all functionality lives under `nx <topic>` (e.g. `nx net`, `nx display`, `nx io`, `nx sec`).
2. **No duplicated logic**:
   - if a legacy `nx-foo` binary exists, it must be a *thin wrapper* that forwards to `nx foo` (or removed).
3. **Single marker namespace**:
   - markers use `nx: <topic> ...` consistently (no parallel `nx-foo:` markers).
4. **Docs converge**:
   - docs live under `docs/tools/nx-*.md` only as *topic docs* for `nx <topic>`, not separate binaries.

## Non-Goals

- Implementing any specific feature (net/display/audio/etc). This is a tooling/naming convergence task.
- Forcing all subcommands to ship at once.

## Constraints / invariants (hard requirements)

- Deterministic output formats for CLIs:
  - stable ordering,
  - bounded lists,
  - explicit `--json` when machine-readable.
- No fake success: wrappers must forward exit codes faithfully.

## Stop conditions (Definition of Done)

- `tools/nx` owns the CLI entrypoint.
- For every topic referenced in tasks as `nx-foo` / `tools/nx-foo/`, either:
  - the task text is updated to `nx <topic>`, or
  - it is explicitly documented as a wrapper-only crate that forwards to `nx <topic>` with no custom logic.

## Touched paths (allowlist)

- `tools/nx/**`
- `docs/tools/**`
- `tasks/**` (text-only convergence edits)
