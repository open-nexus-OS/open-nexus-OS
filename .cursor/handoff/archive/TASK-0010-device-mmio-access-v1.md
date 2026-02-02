# Handoff Archive: TASK-0010 Device MMIO Access v1

**Completed**: 2026-02-02
**Status**: Done

## Summary
Implemented capability-gated MMIO access model for userspace drivers. Kernel enforces W^X boundary, init distributes capabilities (policy-gated + audited), per-device windows proven.

## Contracts delivered
- `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md` (Status: Done)
- Kernel syscalls: `SYSCALL_DEVICE_CAP_CREATE`, `SYSCALL_CAP_TRANSFER_TO`
- Init-controlled distribution (replaced kernel name-checks)
- Policy enforcement via policyd (deny-by-default)

## Proof artifacts
- **Kernel negative tests**: `test_reject_mmio_*` (unaligned VA/offset, overlap, no-cap, wrong-kind, exec-attempt)
- **QEMU markers (all green)**:
  - `SELFTEST: mmio map ok`
  - `rngd: mmio window mapped ok`
  - `virtioblkd: mmio window mapped ok` (virtio-blk consumer proof)
  - `SELFTEST: mmio policy deny ok` (policy deny-by-default proof)
- **Fast local test**: `just test-mmio` (RUN_PHASE=mmio, ~60s)
- **All gates green**: fmt, lint, deny, test-host, test-e2e, arch-check, dep-gate, build-kernel, test-os, make test/build/run

## Key decisions
1. **Init-controlled distribution**: Kernel creates caps on demand, init distributes via policy checks
2. **Per-device windows**: Dynamic virtio-mmio slot probing, separate caps for net/rng/blk
3. **Deterministic slot 48**: MMIO caps assigned to fixed slot for early boot determinism
4. **W^X enforcement**: MMIO mappings are USER|RW only, never executable (page table level)

## Unblocked tasks
- ✅ TASK-0009: Persistence v1 (virtio-blk MMIO access proven)

## Follow-ups
- DMA capability model (separate RFC)
- IRQ delivery to userspace (separate RFC)
- virtio virtqueue operations beyond MMIO probing
- Dynamic device enumeration (device tree parsing for real hardware)
