# Current Handoff: TASK-0012 Kernel SMP v1 (per-CPU runqueues + IPIs) — ACTIVE

**Date**: 2026-02-10  
**Status**: Ready for implementation (contract sync completed)

---

## Session log

### 01 — Archive and transition from TASK-0011B

- Previous handoff moved to archive:
  - `.cursor/handoff/archive/TASK-0011B-kernel-rust-idioms-pre-smp.md`
- Completed baseline remains authoritative:
  - Task: `tasks/TASK-0011B-kernel-rust-idioms-pre-smp.md`
  - RFC seed: `docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md`
  - Key completion commit: `2109b14`

### 02 — TASK-0012 contract sync (anti-drift)

- **Execution SSOT (task)**: `tasks/TASK-0012-kernel-smp-v1-percpu-runqueues-ipis.md`
- **Normative policy**: `tasks/TASK-0277-kernel-smp-parallelism-policy-v1-deterministic.md`
- **Updated in TASK-0012**:
  - Added `enables` + `follow-up-tasks` to keep extension boundaries explicit (`TASK-0013`, `TASK-0042`, `TASK-0247`, `TASK-0283`, both driver/network tracks)
  - Resolved RED decision: baseline secondary-hart boot on QEMU `virt` uses SBI HSM `hart_start`
  - Clarified that `TASK-0247` extends this baseline and must not create parallel SMP authority
  - Added modern-MMIO determinism requirement and explicit SMP marker-gating requirement in harness semantics
  - Aligned stop conditions with allowlist and expanded acceptance criteria to match real proof expectations

---

## Next implementation slice

1. Start `TASK-0012` Phase 1:
   - CPU discovery + online mask
   - typed CPU/Hart ID usage (no raw integer plumbing)
2. Keep single-hart behavior unchanged (`SMP=1` stays green)
3. Preserve TASK-0011B ownership/Send-Sync contracts while introducing per-CPU scheduling boundaries

## Proof commands for active slice

- `cargo test --workspace`
- `just diag-os`
- `SMP=2 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` (SMP marker gate enabled)
- `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
