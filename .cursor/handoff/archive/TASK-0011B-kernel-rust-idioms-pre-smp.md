# Handoff Archive: TASK-0011B Kernel Rust idioms (pre-SMP) — COMPLETED

**Date**: 2026-02-10  
**Status**: Complete (Phases 0→5 done, proofs green)

---

## Session log

### 01 — Start TASK-0011B

- **Execution SSOT (task)**: `tasks/TASK-0011B-kernel-rust-idioms-pre-smp.md`
- **Seed contract (RFC)**: `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md`
- **Prep commit baseline**: `555d5a0` (main ahead of origin/main by 1)
- **Start slice**: Phase 0 docs-first ownership clarification in `docs/architecture/01-neuron-kernel.md`
- **Hard constraints carried forward**:
  - logic-preserving only (no runtime behavior change),
  - ABI-stable (syscall numbers/layouts/errno semantics unchanged),
  - marker-stable (QEMU marker strings/order unchanged),
  - deterministic (no unbounded loops/waits, no fake success).

### 02 — Complete TASK-0011B

- **Implemented**:
  - Phase 1: canonical `Pid`/`CapSlot` newtype migration + ASID/PID typing cleanup across kernel call sites
  - Phase 2: explicit pre-SMP `!Send/!Sync` boundaries via `PhantomData<*mut ()>` markers (`Scheduler`, `TaskTable`, `Router`, `AddressSpaceManager`, `CapTable`)
  - Phase 3: internal syscall error envelope normalization (`SyscallResult`) + `#[must_use]` discipline on kernel error enums
  - Phase 4: minimal typed endpoint capability wrapper (`EndpointCapRef`) integrated in IPC syscall hot paths
  - Phase 5: internal transfer intent centralization in `TaskTable` (`TransferMode`)
- **Proof gates**:
  - `cargo test --workspace` ✅
  - `just diag-os` ✅
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` ✅
- **Contract status**:
  - `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md` moved to `Status: Complete`
  - Phase checklist in RFC-0020 marked complete

---

## Next task handoff

1. `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md` (ready)
2. Use the explicit ownership + thread-boundary markers from TASK-0011B as the SMP split baseline
