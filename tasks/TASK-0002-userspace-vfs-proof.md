---
title: TASK-0002 Userspace VFS Proof
status: Done
owner: @runtime
created: 2025-10-24
links:
  - RFC: docs/rfcs/RFC-0002-process-per-service-architecture.md
  - RFC: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - ADR: docs/adr/0017-service-architecture.md
---

## Context

We need to prove VFS from userspace via real IPC. Kernel placeholders must be removed; readiness should come from services; selftest must exercise real RPCs.

## Note

- If this task contradicts RFC-0002, **RFC-0002 wins** (this task is an implementation/evidence checklist, not an architecture spec).

## Goal

- Userspace-only VFS proof: `init-lite` (thin wrapper) is spawned and uses the kernel `exec` path to launch `packagefsd` + `vfsd`;
  `selftest-client` performs stat/open/read/EBADF via the `nexus-vfs` OS backend (no raw opcode frames in the app) and prints markers.

## Non-Goals

- Kernel VFS parsing or synthetic markers
- Performance tuning

## Steps (Completed)

1. Kernel: Remove/guard VFS placeholder markers (default disabled)
2. init orchestration: Start order includes packagefsd → vfsd; rely on each daemon's `"<service>: ready"`
3. selftest-client: Perform real nexus-vfs RPCs and print markers
4. Runner: scripts/qemu-test.sh gates on userspace markers; kernel markers not accepted
5. Postflight: Helper checks UART markers

## Acceptance Criteria

- QEMU UART shows markers matching the harness contract in `scripts/qemu-test.sh`, including (order with slack):
  - init markers (`init: start`, `init: start <service>`, `init: up <service>`, `init: ready`) where `<service>` is a literal placeholder
  - service readiness (`packagefsd: ready`, `vfsd: ready`)
  - VFS E2E markers from selftest-client:
    - `SELFTEST: vfs stat ok`
    - `SELFTEST: vfs read ok`
    - `SELFTEST: vfs real data ok` (read deterministic bytes via vfsd → packagefsd over IPC)
    - `SELFTEST: vfs ebadf ok`
  - Bundle image bring-up marker:
    - `SELFTEST: bundlemgrd v1 image ok`
- No kernel-emitted VFS markers remain
- No unwrap/expect added; no blanket allow(dead_code)

## Evidence

- UART logs captured by `just test-os` and postflight checker (`scripts/qemu-test.sh` exits 0).

## Historical blockers (resolved)

- Early bring-up versions did not always spawn userspace init or had runner embed mismatches. Today the harness requires `init: start` and
  the full userspace marker chain; kernel fallback markers are not accepted.

## Next Steps

- None (task is complete). If this breaks in the future, treat `scripts/qemu-test.sh` as the source of truth for required markers.
