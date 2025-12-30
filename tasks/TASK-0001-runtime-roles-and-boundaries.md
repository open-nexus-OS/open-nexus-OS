---
title: TASK-0001 Runtime roles & boundaries (Host + OS-lite): init/execd/loader single-authority, deprecations, and proof locks
status: Done
owner: @init-team @runtime
created: 2025-10-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0002-process-per-service-architecture.md
  - RFC: docs/rfcs/RFC-0004-safe-loader-guards.md
  - VFS proof (markers dependency): tasks/TASK-0002-userspace-vfs-proof.md
---

## Context

We need a clear, stable runtime split between **Host** and **OS-lite** that avoids duplicated “authorities”
and prevents silent drift:

- multiple “init/spawn/load” implementations are easy to accidentally fork,
- proof markers must remain sourced from real services and `scripts/qemu-test.sh`,
- the `cfg` split must stay consistent (`nexus_env="host"` / `nexus_env="os"`, OS-lite behind `feature="os-lite"` where applicable).

Repo reality (audit snapshot):

- `nexus-init` selects `std_server` vs `os_lite` (bring-up path).
- OS-lite bootstrap spawns services and emits init markers (`init: up <svc>`) with cooperative yields.
- `execd` already uses `userspace/nexus-loader` (`OsMapper`, `StackBuilder`) under `nexus_env="os"`.
- Kernel loader behavior is owned by kernel `exec` / `exec_v2` (see RFC‑0002/RFC‑0004); do not reintroduce a parallel “userspace real loader” path.
- `apps/init-lite` duplicates the init role and is being deprecated/wrapped.

## Goal

Establish and enforce a single-authority runtime model:

- **Init**: `source/init/nexus-init` is the canonical orchestrator (host + OS-lite).
- **Spawner**: `source/services/execd` is the canonical process spawner.
- **Loader**: `userspace/nexus-loader` is the canonical load/ELF/ABI library.
- **Kernel bridge**: kernel loader logic stays a thin ABI bridge only (no duplicated “real loader” logic).

## Non-Goals

- Kernel internals unrelated to user-space spawn/exec boundaries.
- Feature additions beyond role consolidation and deprecation hardening.
- Designing new ABIs (follow-up tasks/RFCs if needed).

## Constraints / invariants (hard requirements)

- **No fake success**: do not add “ready/ok” markers that don’t reflect real behavior.
- **Determinism**: markers are stable strings; no timestamps/randomness in proof signals.
- **Role single-authority**: no duplicate init/spawn/loader implementations shipping in parallel.
- **Cfg split consistency**: `nexus_env="host"` / `nexus_env="os"` remains canonical; OS-lite build gates remain explicit (e.g., `feature="os-lite"`).
- **Rust hygiene**: no `unwrap/expect` in daemons; avoid new `unsafe` (justify if unavoidable).

## Red flags / decision points (track explicitly)

- **RED (blocking / must decide now)**:
  - If kernel `user_loader` continues to implement a “real loader”, we must explicitly treat it as temporary and keep its scope minimal (RFC-0004 direction).
- **YELLOW (risky / likely drift / needs follow-up)**:
  - “Deprecate init-lite” must not break QEMU marker proofs; the deprecation path must preserve the init marker contract.
- **GREEN (confirmed assumptions)**:
  - `execd` already depends on `userspace/nexus-loader` in the OS path; we can lock this in with tests/guards rather than rewriting behavior.

## Contract sources (single source of truth)

- **QEMU marker contract**: `scripts/qemu-test.sh`
- **Runtime entrypoints**:
  - `source/init/nexus-init/src/lib.rs`
  - `source/services/execd/src/std_server.rs` and `source/services/execd/src/os_lite.rs`
  - `userspace/nexus-loader/src/lib.rs`
  - kernel `exec`/`exec_v2` path (see RFC‑0002/RFC‑0004)
- **VFS proof contract**: `tasks/TASK-0002-userspace-vfs-proof.md`

## Stop conditions (Definition of Done)

- **Proof (tests)**:
  - Command(s):
    - `cargo test --workspace`
- **Proof (QEMU)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Required behavior:
    - init marker chain remains intact (no regression relative to `TASK-0002`)

Notes:

- Postflight scripts are not proof unless they only delegate to the canonical harness/tests and do not invent their own “OK”.

## Touched paths (allowlist)

- `source/init/nexus-init/`
- `source/services/execd/`
- `userspace/nexus-loader/`
- `source/kernel/neuron/src/user_loader.rs` (boundary trimming only; no feature expansion)
- `source/apps/init-lite/` (deprecation/wrapper)
- `docs/adr/` + `docs/standards/` (role/boundary docs)

## Plan (small PRs)

1. Add `CONTEXT` headers and cross-links in the listed entrypoints (roles/owners/invariants).
2. Add (or confirm) ADR capturing runtime roles/boundaries and link it from headers.
3. Deprecate `apps/init-lite` (doc + wrapper behavior) without breaking QEMU marker proofs.
4. Add “drift guards” (lint/CI or compile-time asserts) that prevent duplicated loader/spawner logic from creeping in.

## Acceptance criteria (behavioral)

- The repo has a single, clearly documented authority for init/spawn/load (as listed in Goal).
- QEMU marker proofs remain green and do not rely on kernel fallback markers.
- The OS path uses `userspace/nexus-loader` (and tests/guards prevent regressions).

## Evidence (to paste into PR)

- Tests: `cargo test --workspace` (Exit 0)
- QEMU: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` (Exit 0)

## RFC seeds (for later, when the step is complete)

- Decisions made:
  - canonical runtime authorities (init/spawn/load) and their cfg gates
  - deprecation policy for `apps/init-lite`
- Open questions:
  - if/when kernel `user_loader` can be reduced further without breaking bring-up
- Stabilized contracts:
  - `scripts/qemu-test.sh` required marker set for init/service bring-up
