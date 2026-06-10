# Handoff — Animation Fix + `just start` Repair

Date: 2026-06-09

## Status

Three bugs fixed. 11/11 windowd host tests pass. Dep-gate PASS. Cross-compile OK.
⚠️ QEMU visible-bootstrap requires GTK display — unavailable in CI runner.

## Bugs fixed

### 1. Animation freeze (pacer timer re-arm)
**Root cause**: `source/services/windowd/src/compositor/mod.rs` — bottom recv handler (line ~415) handles `OP_TIMER_FIRED` but did NOT reset `pacer_timer_armed = false`. After first timer fire, `pacer_timer_armed` stayed true, so the pacing arm block (`!pacer_timer_armed`) never re-armed the one-shot timer. Animation froze after one tick.

**Fix**: Added `pacer_timer_armed = false;` to the bottom recv handler, matching the batch recv handler at line 335. Also added `flush_pending_damage()` call for consistency.

**Expected marker chain after fix**:
```
uiruntime: batch commit ok
windowd: live transition ok
uianim: spring converge ok
SELFTEST: ui v5 transition ok
```

### 2. `just start` build failure (Makefile MODE=host)
**Root cause**: The Makefile's `build` target ignored `MODE=host` — always used podman container build. `just start` calls `make MODE=host build` but the build ran in podman, failing if podman wasn't available.

**Fix**: Added `MODE ?= container` variable and `ifeq ($(MODE),host)` branch that runs direct `cargo build` + `./scripts/build.sh`.

### 3. Windowd heap exhaustion (768KB → 1MB)
**Root cause**: In interactive mode (`just start`), high-rate input (1484Hz vs 94Hz in proof mode) exhausts the bump allocator. Windowd crashed with `alloc-fail svc=windowd` after ~3s.

**Fix**: 
- Added `heap-1m` feature to `nexus-service-entry` (1024KB HEAP_SIZE)
- Updated `windowd/Cargo.toml` `os-lite` feature to use `heap-1m`
- Updated stale comment in `runtime.rs` referencing old 384KB heap

## Files changed

| File | Change |
|------|--------|
| `source/services/windowd/src/compositor/mod.rs` | +9 lines: pacer_timer_armed = false + flush in bottom recv |
| `Makefile` | +9 lines: MODE=host conditional in build target |
| `source/libs/nexus-service-entry/Cargo.toml` | +1 line: heap-1m feature |
| `source/libs/nexus-service-entry/src/lib.rs` | +3 lines: heap-1m HEAP_SIZE constant |
| `source/services/windowd/Cargo.toml` | 1 line changed: heap-768k → heap-1m |
| `source/services/windowd/src/compositor/runtime.rs` | 1 line: comment update (384KB → 1MB) |

## Verification

| Check | Result |
|-------|--------|
| cargo check nexus-service-entry (heap-1m) | ✅ |
| cargo check windowd (os-lite, riscv) | ✅ 128 warnings (pre-existing) |
| windowd host tests (11) | ✅ |
| dep-gate (forbidden crates) | ✅ PASS |
| cross-compile (build.sh) | ✅ kernel=6605120B init=6374520B |
| QEMU visible-bootstrap | ⚠️ requires GTK display |

## Next step

`just test-os visible-bootstrap` on a machine with GTK display to verify `SELFTEST: ui v5 transition ok` marker appears.
