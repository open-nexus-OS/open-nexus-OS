---
title: TASK-0050 Recovery Mode v1a: boot target + minimal recovery service graph + safe TTY shell + diag bundle
status: Draft
owner: @reliability
created: 2025-12-23
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Process-per-service: docs/rfcs/RFC-0002-process-per-service-architecture.md
  - Config broker (read-only in recovery): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Observability v1 (logd): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Crashdump v2 (optional diag inputs): tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md
  - Crashdump v2 (OS pipeline): tasks/TASK-0049-crashdump-v2b-os-crashd-retention-correlation-policy.md
  - DevX CLI: tasks/TASK-0045-devx-nx-cli-v1.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a deterministic, operator-safe recovery path that:

- boots into a **single-user** environment,
- starts only a minimal service set with tight budgets,
- exposes a constrained shell (no arbitrary execution),
- can collect a diagnostic bundle for offline analysis.

This task is **Recovery v1a**: the boot target + orchestration + safe shell + diagnostics.
Deep repair/OTA/slot operations are deferred to `TASK-0051` to keep scope controlled.

## Goal

Deliver:

1. A boot target selector `nexus.target={normal|recovery}` with deterministic handoff.
2. A recovery orchestrator (`recovery-init`) that starts a minimal service graph.
3. A UART/TTY shell (`recovery-sh`) with **safe built-ins**:
   - `diag`, `help`, `reboot`, `poweroff`
4. Deterministic markers for QEMU proof and bounded selftests (once gated deps exist).

## Non-Goals

- Kernel changes.
- Full repair and OTA workflow (TASK-0051).
- Multi-user and networking in recovery (optional future).
- Image flashing or provisioning (handled by `TASK-0260`/`TASK-0261` as an extension; this task focuses on boot target, minimal shell, and diagnostics).

## Constraints / invariants (hard requirements)

- Kernel untouched.
- Recovery must be **read-only by default**.
- Strictly bounded memory and I/O budgets in recovery.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No interactive “escape hatch”: recovery shell is built-ins only.

## Red flags / decision points

- **RED (OS gating dependencies)**:
  - If `logd`/`configd` do not exist yet, recovery must still boot and provide markers, but diag contents will be reduced.
  - `/state` persistence (TASK-0009) is required if we want to write diag bundles to disk.
- **YELLOW (single-user semantics)**:
  - “Single-user” is enforced by **not starting execd / normal service graph** and by only starting the minimal set from `recovery-init`.
  - This is a userspace contract; it does not prevent accidental background tasks if recovery-init spawns them.

## Contract sources (single source of truth)

- Boot target string: `nexus.target` (`normal` default).
- Recovery markers are part of the test contract (`scripts/qemu-test.sh`) once enabled.

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `recovery: boot target selected`
- `recovery: init start`
- `recovery: services ready`
- `recovery: shell ready`
- `recovery: diag written (bytes=<n>)` (if `/state` is available)
- `SELFTEST: recovery boot ok`
- `SELFTEST: recovery diag ok` (if `/state` is available)

## Touched paths (allowlist)

- `source/init/` or equivalent early userspace routing (handoff to recovery-init)
- `source/apps/recovery-init/` (new)
- `source/apps/recovery-sh/` (new)
- `docs/recovery/index.md` (new)
- `tools/nx/` (follow-up task adds `nx recovery`; see TASK-0051)
- `scripts/qemu-test.sh` (gated marker list)
- `tools/postflight-recovery*.sh` (must delegate to canonical proof)

## Plan (small PRs)

1. **Boot target selection + handoff**
   - Parse `nexus.target` (default `normal`).
   - Route `recovery` to `recovery-init` (do not start normal graph).
   - Emit:
     - `recovery: boot target selected`
     - `recovery: init start`

2. **Minimal recovery service graph (v1a)**
   - Start only what’s needed for diag and basic introspection:
     - `logd` (if available; small ring; UART mirror)
     - `configd` (read-only; no 2PC apply) if available
     - `packagefsd` (RO) if required for binaries/assets
   - Emit: `recovery: services ready`

3. **Recovery shell (safe built-ins only)**
   - UART/TTY line editor (minimal, bounded).
   - Commands:
     - `help`
     - `diag` (writes a tar.zst to `/state/recovery/` if available; otherwise prints “not available” and emits marker with bytes=0)
     - `reboot`, `poweroff`
   - Emit: `recovery: shell ready`

4. **Selftest (recovery target)**
   - When booted with `nexus.target=recovery`:
     - `SELFTEST: recovery boot ok`
     - trigger `diag` (if `/state` is available) → `SELFTEST: recovery diag ok`

5. **Docs**
   - `docs/recovery/index.md`: recovery guarantees, minimal graph, shell commands, how to boot into recovery in QEMU.
