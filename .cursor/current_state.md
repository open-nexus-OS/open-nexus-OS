# Current State — Open Nexus OS

Last updated: 2026-06-09

## Active focus

**Animation fix + `just start` repair — 86% (3 fixes applied, QEMU verification pending)**

## Architecture

```
VMO (16MB, 1280×3200, 4-plane):
  Plane 0: wallpaper source       (offset 0x000000)
  Plane 1: retained scene         (offset 0x3E8000)
  Plane 2: frame ring slot A      (offset 0x7D0000)
  Plane 3: frame ring slot B      (offset 0xBB8000)

windowd heap (1MB):                gpud pipeline:
  scene graph, layout, IPC           BlitSurface, FillSdfRoundedRect
  band_scratch (200KB)               BlurBackdrop, StrokeSdfRoundedRect
  heap usage: ~768KB (was 768KB)     DrawCursorResource, DrawTiles
                                     BlendCursor, DrawLine
kernel:                              TRANSFER_TO_HOST + RESOURCE_FLUSH
  HartTimers (BTreeMap queue)
  timer_create/set/cancel syscalls
  IRQ → pop_expired → OP_TIMER_FIRED
  all Context::new + install_runtime sites (52 test + 4 OS)
```

## Gate status (2026-06-09)

| Check | Result |
|-------|--------|
| cargo check windowd (os-lite, riscv) | ✅ |
| windowd host tests (11) | ✅ 11/11 |
| dep-gate (forbidden crates) | ✅ PASS |
| cross-compile (build.sh) | ✅ kernel=6605120B init=6374520B |
| QEMU visible-bootstrap | ⚠️ requires GTK display |

## Fixes applied (2026-06-09)

### 1. Animation pacer timer re-arm (mod.rs)
- Bottom recv `OP_TIMER_FIRED` handler now resets `pacer_timer_armed = false`
- Also adds `flush_pending_damage()` for consistency with batch recv handler
- Expected chain: `batch commit → live transition → spring converge → v5 transition ok`

### 2. Makefile MODE=host support
- Added `MODE ?= container` variable
- `ifeq ($(MODE),host)` branch: direct cargo build + build.sh
- `just start` no longer requires podman

### 3. Windowd heap 768KB → 1MB
- Added `heap-1m` feature to nexus-service-entry
- windowd os-lite feature now uses heap-1m
- Fixes `alloc-fail svc=windowd` in high-rate interactive mode

## Production UI End Architecture

| Workstream | Progress |
|-----------|----------|
| 1. Remove CPU compositing | 85% (GPU blur path wired) |
| 2. Present ring | 85% (4-plane VMO, slot tracking) |
| 3. Resource model | 60% (budgets + handles defined) |
| 4. Blur by architecture | 50% (GPU BlurBackdrop active, CPU fallback) |
| 5. Cursor GPU-first | 85% (unchanged) |
| 6. Unified pacing | 90% (pacer re-arm fix applied) |
| 7-8. DSL/SystemUI | 0% (future) |
| **Aggregate** | **65%** |

## Pending

- ⬜ QEMU visible-bootstrap verification (requires GTK display)
- ⬜ `SELFTEST: ui v5 transition ok` marker verification
- ⬜ PRESENT_DONE events (gpud async completion channel)
- ⬜ Cursor hardware upload (Phase 6)
- ⬜ `gpud: bad-status=0x02` scanout race (still present in logs)
