---
title: TASK-0002 Userspace VFS Proof
status: Done
owner: @runtime
created: 2025-10-24
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0002-process-per-service-architecture.md
  - RFC: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - ADR: docs/adr/0017-service-architecture.md
---

## Context

We need to prove VFS from userspace via real IPC:

- readiness must come from services (not kernel placeholders),
- selftest must exercise real RPCs,
- the QEMU harness must gate on deterministic markers emitted by userland services.

## Goal

- Userspace-only VFS proof: `init-lite` is spawned and uses the kernel `exec` path to launch `packagefsd` + `vfsd`;
  `selftest-client` performs stat/open/read/EBADF via the `nexus-vfs` OS backend (no raw opcode frames in the app) and prints markers.

## Non-Goals

- Kernel VFS parsing or synthetic markers.
- Performance tuning.

## Constraints / invariants (hard requirements)

- **No fake success**: no `*: ready` / `SELFTEST: * ok` markers unless the real behavior happened.
- **Determinism**: markers are stable strings; no timestamps/random bytes in proof signals.
- **Rust hygiene**: no new `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **GREEN (confirmed assumptions)**:
  - If this task contradicts RFC-0002, **RFC-0002 wins** (this task is an implementation/evidence checklist, not an architecture spec).

## Contract sources (single source of truth)

- **QEMU marker contract**: `scripts/qemu-test.sh`
- **VFS client contract**: `userspace/nexus-vfs/` OS backend (no raw frames in apps)
- **Service bring-up contract**: `source/init/nexus-init/` (service order and init markers)

## Steps (Completed)

1. Kernel: Remove/guard VFS placeholder markers (default disabled)
2. init orchestration: Start order includes packagefsd → vfsd; rely on each daemon's `"<service>: ready"`
3. selftest-client: Perform real nexus-vfs RPCs and print markers
4. Runner: scripts/qemu-test.sh gates on userspace markers; kernel markers not accepted
5. Postflight: Helper delegates to canonical proofs (no independent “OK” semantics)

## Stop conditions (Definition of Done)

- **Proof (QEMU)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Required markers (as enforced by `scripts/qemu-test.sh`, order tolerant):
    - init markers:
      - `init: start`
      - `init: start <service>`
      - `init: up <service>`
      - `init: ready`
    - service readiness:
      - `packagefsd: ready`
      - `vfsd: ready`
    - VFS E2E markers from selftest-client:
      - `SELFTEST: vfs stat ok`
      - `SELFTEST: vfs read ok`
      - `SELFTEST: vfs real data ok`
      - `SELFTEST: vfs ebadf ok`
    - bring-up bundle marker:
      - `SELFTEST: bundlemgrd v1 image ok`

Notes:

- Postflight scripts are not proof unless they only delegate to the canonical harness/tests and do not invent their own “OK”.

## Touched paths (allowlist)

- `source/init/nexus-init/`
- `source/services/packagefsd/`
- `source/services/vfsd/`
- `userspace/nexus-vfs/`
- `source/apps/selftest-client/`
- `scripts/qemu-test.sh`

## Evidence

- QEMU: UART logs captured by `just test-os` with `scripts/qemu-test.sh` passing (marker-gated early exit).

## Historical blockers (resolved)

- Early bring-up versions did not always spawn userspace init or had runner embed mismatches. Today the harness requires `init: start` and
  the full userspace marker chain; kernel fallback markers are not accepted.

## Next Steps

- None (task is complete). If this breaks in the future, treat `scripts/qemu-test.sh` as the source of truth for required markers.
