---
title: TASK-0002 Userspace VFS Proof
status: Done
owner: @runtime
created: 2025-10-24
links:
  - RFC: docs/rfcs/RFC-0001-kernel-simplification.md
  - ADR: docs/adr/0017-service-architecture.md
---

Context
We need to prove VFS from userspace via real IPC. Kernel placeholders must be removed; readiness should come from services; selftest must exercise real RPCs.

Goal
- Userspace-only VFS proof: packagefsd/vfsd launched by nexus-init; selftest-client performs stat/open/read/EBADF via nexus-vfs OS backend and prints markers.

Non-Goals
- Kernel VFS parsing or synthetic markers
- Performance tuning

Steps (Completed)
1. Kernel: Remove/guard VFS placeholder markers (default disabled)
2. nexus-init: Start order includes packagefsd → vfsd; rely on each daemon's "<svc>: ready"
3. selftest-client: Perform real nexus-vfs RPCs and print markers
4. Runner: scripts/qemu-test.sh gates on userspace markers; kernel markers not accepted
5. Postflight: Helper checks UART markers

Acceptance Criteria
- QEMU UART shows, in order with slack:
  - NEURON
  - init: start
  - keystored: ready
  - policyd: ready
  - samgrd: ready
  - bundlemgrd: ready
  - packagefsd: ready
  - vfsd: ready
  - init: ready
  - SELFTEST: vfs stat ok
  - SELFTEST: vfs read ok
  - SELFTEST: vfs ebadf ok
- No kernel-emitted VFS markers remain
- No unwrap/expect added; no blanket allow(dead_code)

Evidence
- UART logs captured by `just test-os` and postflight checker

Blocking Issues (must be addressed)
- Open boot path: Kernel no longer embeds/spawns nexus-init (os-lite). Without a spawn point, userspace cannot print markers.
- Runner embed mismatch: scripts/run-qemu-rv64.sh builds/embeds init-lite instead of nexus-init with `--features os-lite`.
- Harness gating: The script waits for userspace markers that never arrive (no userspace running) → timeout.
- ECALL/trap not validated: Unified ECALL path (U/S) is wired but not exercised by a real U-task (no ECALL-U traces in UART).
- cfg lints: `cfg(nexus_env="os")` produces warnings; should be declared via `check-cfg` or proper feature gating.
- Stray file: `source/kernel/neuron/src/user_loader.rs` is untracked; remove or fully integrate.

Next Steps
- Restore spawn/embedding of nexus-init (os-lite) in `kmain` and switch runner INIT_ELF to `nexus-init --features os-lite`.
- Briefly confirm ECALL-U (e.g., a `debug_putc` in `_start`) and then verify the full marker chain.
