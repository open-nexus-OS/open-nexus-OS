# TASK-0009 Continuation: Persistence v1 (StateFS + VirtIO-blk)

**Date**: 2026-02-03
**Task**: `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
**Status**: BLOCKED - virtio-blk I/O timeout in QEMU (extensive debugging done)

---

## Summary

VirtIO-blk driver is fully configured but QEMU never processes queue entries.
All configuration parameters are correct; the issue appears to be QEMU-side.

## Debug Findings

```
virtio-blk: mmio legacy
virtio-blk: q_pa=0x8176b000 pfn=0x8176b
virtio-blk: q_max=00000400           <- Device reports max queue=1024, we use 8 ✓
virtio-blk: status=00000003          <- ACK | DRIVER ✓
virtio-blk: status_ok=00000007       <- + DRIVER_OK ✓
virtio-blk: buf_pa=0x8176c000        <- Buffer PA valid ✓
virtio-blk: cap=0x20000 sectors      <- 131072 sectors = 64MB ✓
virtio-blk: notify avail_idx=00000001 <- We added one entry ✓
virtio-blk: timeout last=00000000    <- used.idx stays 0 ✗
```

**Key observation**: virtio-net works with identical code patterns.
Both use legacy mode (version 1), same queue setup, same MMIO access.

## Attempted Fixes (all failed)

1. Queue memory zeroing order (before/after setup_queue)
2. Warmup read after device init
3. Volatile writes to virtqueue structures
4. Legacy mode feature negotiation (no FEATURES_SEL for high bits)
5. Initial queue kick after driver_ok
6. Delay between driver_ok and first I/O
7. Various debug instrumentation

## Hypothesis

The issue is likely one of:
1. **QEMU virtio-blk-device quirk** - different behavior from virtio-net in legacy mode
2. **icount timing** - QEMU's `-icount 1,sleep=on` may affect virtio-blk differently
3. **Backing file access** - QEMU may have issues with the raw file I/O

## What Works

- Host tests: 35 tests pass
- Device discovery: magic, version, capacity all correct
- Queue setup: PFN written, device acknowledges
- virtio-net: works with same code patterns
- SELFTEST markers (except persist tests):
  - `SELFTEST: reply loopback ok`
  - `SELFTEST: keystored capmove ok`
  - `SELFTEST: rng entropy ok`

## Recommended Next Steps

1. **Test outside icount mode**: Try `QEMU_ICOUNT=off` (requires script modification)
2. **Compare with known-good driver**: Check Linux's virtio-blk for legacy mode quirks
3. **QEMU monitor debugging**: Run with `-monitor stdio` and inspect device state
4. **Escalate to mini-task**: Create focused debugging task for virtio-blk

## Test Commands

```bash
# Host tests (pass):
cargo test -p statefs -p storage-virtio-blk -p updates_host

# QEMU test (virtio-blk fails, others work):
RUN_PHASE=mmio RUN_UNTIL_MARKER=1 RUN_TIMEOUT=120s INIT_LITE_WATCHDOG_TICKS=800 ./scripts/qemu-test.sh

# Check virtio-blk output:
grep -E "virtio-blk:|warmup|persist" uart.log
```

## Key Files

| File | Purpose |
|------|---------|
| `source/drivers/storage/virtio-blk/src/lib.rs` | Driver with debug instrumentation |
| `userspace/nexus-net-os/src/smoltcp_virtio.rs` | Working virtio-net for comparison |
| `source/services/statefsd/src/os_lite.rs` | StateFS service |

## Commits This Session

- `7264519`: Update TASK-0009 status - blocked on virtio-blk I/O timeout
- `3fe2987`: TASK-0009: StateFS persistence v1 + virtio-blk driver WIP
- `ae26c51`: virtio-blk: add debug instrumentation for queue setup
