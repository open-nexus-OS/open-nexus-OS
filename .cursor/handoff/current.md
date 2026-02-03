# TASK-0009 Continuation: Persistence v1 (StateFS + VirtIO-blk)

**Date**: 2026-02-03
**Task**: `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
**Status**: Core persistence working, routing issues blocking full QEMU test pass

---

## Current State

### Working ✓

**Host tests (35 tests, ~6 seconds):**
```bash
cargo test -p statefs -p storage-virtio-blk -p updates_host
```

**QEMU markers achieved:**
- `statefsd: ready`
- `blk: virtio-blk up`
- `SELFTEST: statefs put ok`
- `SELFTEST: statefs persist ok`
- `SELFTEST: device key persist ok`

### Not Working ✗

| Issue | Cause | Location |
|-------|-------|----------|
| `SELFTEST: ota switch FAIL` | `updated` can't route to `bundlemgrd` | `updated/os_lite.rs:509` |
| `SELFTEST: samgrd v1 register FAIL` | Dynamic routing fails | selftest-client |
| `SELFTEST: bootctl persist ok` missing | Test never reached (timeout) | After OTA tests |
| Watchdog fires | Routing operations block too long | init-lite |

---

## Immediate Fix Needed

**Problem**: `updated` uses dynamic routing for bundlemgrd which fails:
```rust
// source/services/updated/src/os_lite.rs line ~509
let client = match KernelClient::new_for("bundlemgrd") {
    Err(_) => return Err("route"),  // <-- fails here
};
```

**Solution**: Use hardcoded slots like we did for statefsd/keystored policyd calls.

Check init-lite's updated block for bundlemgrd slots:
```bash
grep -A20 '"updated" =>' source/init/nexus-init/src/os_payload.rs
```

---

## New Infrastructure Created

### 1. `slot_map.rs` - Centralized slot definitions
**Path**: `source/init/nexus-init/src/slot_map.rs`

Defines expected slot numbers per service. Use instead of magic numbers:
```rust
use nexus_init::slot_map::selftest;
KernelClient::new_with_slots(selftest::KEYSTORED_SEND, selftest::KEYSTORED_RECV)
```

### 2. `slot_probe.rs` - Early validation
**Path**: `source/libs/nexus-abi/src/slot_probe.rs`

Validates slots at service startup before they're used:
```rust
use nexus_abi::slot_probe::validate_slots;

let missing = validate_slots("keystored", &[
    ("policyd", 0x09),
    ("reply_recv", 0x05),
]);
if missing > 0 {
    return Err(ServerError::Unsupported);
}
// Emits: "SLOT MISSING: keystored needs policyd at slot 0x09"
```

**TODO**: Integrate `validate_slots` into keystored, statefsd, updated at startup.

---

## Test Commands

```bash
# Fast iteration (host tests first):
cargo test -p statefs -p storage-virtio-blk -p updates_host

# QEMU with increased watchdog:
RUN_PHASE=mmio RUN_UNTIL_MARKER=1 RUN_TIMEOUT=120s INIT_LITE_WATCHDOG_TICKS=800 ./scripts/qemu-test.sh

# Filter for relevant output:
./scripts/qemu-test.sh 2>&1 | grep -E "SELFTEST:|updated:|SLOT|route"
```

---

## Key Files

| File | Purpose |
|------|---------|
| `source/services/updated/src/os_lite.rs` | OTA switch routing issue |
| `source/services/keystored/src/os_stub.rs` | Device key persistence (working) |
| `source/services/statefsd/src/os_lite.rs` | StateFS service (working) |
| `source/apps/selftest-client/src/main.rs` | All SELFTEST markers |
| `source/init/nexus-init/src/os_payload.rs` | Slot distribution logic |

---

## Session Context

- Converted keystored/statefsd `policyd_allows` to hardcoded slots
- Added keystored to selftest-client's hardcoded slot list (0x11, 0x12)
- Fixed keystored `reload_device_key` to actually read from statefsd
- Created slot_map.rs and slot_probe.rs for deterministic slot handling
- Core persistence works; remaining failures are routing issues not persistence bugs
