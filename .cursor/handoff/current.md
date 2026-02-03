# TASK-0009 Continuation: Persistence v1 (StateFS + VirtIO-blk)

**Date**: 2026-02-03
**Task**: `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
**Status**: BLOCKED - virtio-blk I/O timeout in QEMU

---

## Blocking Issue

**VirtIO-blk device doesn't respond to queue notifications.**

```
virtio-blk: mmio legacy
virtio-blk: q_pa=0x8176a000 pfn=0x8176a
virtio-blk: buf_pa=0x8176b000
virtio-blk: timeout last=00000000 polls=0000b041
virtio-blk: warmup failed
```

### What Works
- Device discovery (magic, ID, legacy version detection)
- Queue setup (PFN written correctly)
- Feature negotiation (fixed for legacy mode)
- Memory allocation (VMO, physical addresses)

### What Fails
- First I/O request times out after ~2s
- `used.idx` stays at 0 (device never processes request)
- Same behavior for both read and write operations

### Attempted Fixes (all failed, same error)
1. Queue memory zeroing before `setup_queue()` + `driver_ok()`
2. Warmup read after device init
3. Volatile writes to virtqueue structures
4. Legacy mode feature negotiation (no FEATURES_SEL for high bits)

### Diagnosis Needed
- QEMU device tree verification (`info qtree`)
- Compare virtio-blk slot assignment vs virtio-net (which works)
- Check if QEMU's legacy virtio-blk has specific quirks

---

## Current State

### Working ✓

**Host tests (35 tests):**
```bash
cargo test -p statefs -p storage-virtio-blk -p updates_host
```

**QEMU markers achieved:**
- `statefsd: ready`
- `SELFTEST: statefs put ok`
- `SELFTEST: reply loopback ok`
- `SELFTEST: keystored capmove ok`
- `SELFTEST: rng entropy ok`
- `net: virtio-net up` (virtio-net works!)

### Not Working ✗

| Issue | Cause | Location |
|-------|-------|----------|
| `SELFTEST: statefs persist FAIL` | virtio-blk timeout | statefsd |
| `SELFTEST: device key persist FAIL` | virtio-blk timeout | keystored |
| `virtio-blk: warmup failed` | Device doesn't respond | virtio-blk driver |

---

## Key Observations

1. **virtio-net works, virtio-blk doesn't** - same MMIO probing, same feature negotiation code
2. **Legacy mode (version 1)** - both devices are legacy, but net works
3. **Queue addresses look valid** - PFN 0x8176a, buf at 0x8176b000
4. **Device never processes** - `used.idx` stays 0 through ~45000 polls

---

## Infrastructure Created

### 1. `slot_map.rs` - Centralized slot definitions
**Path**: `source/init/nexus-init/src/slot_map.rs`

### 2. `slot_probe.rs` - Early validation  
**Path**: `source/libs/nexus-abi/src/slot_probe.rs`

### 3. `statefsd` - StateFS service
**Path**: `source/services/statefsd/`

### 4. `statefs` + `storage` userspace crates
**Path**: `userspace/statefs/`, `userspace/storage/`

---

## Test Commands

```bash
# Host tests (these pass):
cargo test -p statefs -p storage-virtio-blk -p updates_host

# QEMU with increased watchdog:
RUN_PHASE=mmio RUN_UNTIL_MARKER=1 RUN_TIMEOUT=120s INIT_LITE_WATCHDOG_TICKS=800 ./scripts/qemu-test.sh

# Check virtio-blk output:
grep -E "virtio-blk|warmup|persist" uart.log
```

---

## Recommended Next Steps

1. **QEMU monitor debugging**: Run with `-monitor stdio`, use `info qtree` to inspect virtio-blk device
2. **Compare with virtio-net**: Check `userspace/nexus-net-os/src/smoltcp_virtio.rs` queue setup
3. **Add QEMU trace events**: `qemu-system-riscv64 -trace "virtio_blk*" ...`
4. **Check backing file**: Verify `build/blk.img` is accessible by QEMU process

---

## Key Files

| File | Purpose |
|------|---------|
| `source/drivers/storage/virtio-blk/src/lib.rs` | VirtIO-blk driver (blocking issue) |
| `userspace/nexus-net-os/src/smoltcp_virtio.rs` | Working virtio-net for comparison |
| `source/services/statefsd/src/os_lite.rs` | StateFS service |
| `source/init/nexus-init/src/os_payload.rs` | MMIO grants |

---

## Session Context

- Commit `3fe2987`: All changes committed as WIP
- Testing discipline: Hit 4 attempts without progress, need fresh approach
- virtio-net uses same feature negotiation but works - issue is blk-specific
