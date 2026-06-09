# Current State — Open Nexus OS

Last updated: 2026-06-08

## Active focus

**TASK-0062 + Production UI End Architecture — 63% (Phases 1-5 closed, 6-8 pending)**

## Architecture

```
VMO (16MB, 1280×3200, 4-plane):
  Plane 0: wallpaper source       (offset 0x000000)
  Plane 1: retained scene         (offset 0x3E8000)
  Plane 2: frame ring slot A      (offset 0x7D0000)
  Plane 3: frame ring slot B      (offset 0xBB8000)

windowd heap (768KB):              gpud pipeline:
  scene graph, layout, IPC           BlitSurface, FillSdfRoundedRect
  band_scratch (200KB)               BlurBackdrop, StrokeSdfRoundedRect
  heap usage: ~500KB                 DrawCursorResource, DrawTiles
                                     BlendCursor, DrawLine
kernel:                              TRANSFER_TO_HOST + RESOURCE_FLUSH
  HartTimers (BTreeMap queue)
  timer_create/set/cancel syscalls
  IRQ → pop_expired → OP_TIMER_FIRED
  all Context::new + install_runtime sites (52 test + 4 OS)
```

## Gate status (2026-06-08 — Phase 1+2 implemented)

| Check | Result |
|-------|--------|
| cargo check (host) | ✅ |
| cargo check (riscv) | ✅ |
| just diag-os | ✅ |
| forbidden crates | ✅ |
| make build | ✅ |
| just test-os headless | ✅ SELFTEST completed |
| gpud tests (20) | ✅ |
| nexus-gfx (global + fence + golden + perf) | ✅ |
| kernel timer (7) | ✅ |
| kernel all (all) | ✅ |

## What's Done — Complete

### Phase 6c: GPU rendering (CLOSED)
- submit() executes 6 command types + cursor resources
- 4-plane 16MB VMO with double-buffered frame ring
- send_blit_surface_cb() builds full CommandBuffer per frame
- Steady-state: GPU-only. Retained scene: CPU-only when dirty.

### Phase 6d: Pipeline bounding (CLOSED)
- Honest fence (5 tests). MAX_IN_FLIGHT=2. Completion correlation.
- Frame slot backpressure on exhaustion. cleanup_frame_ring + Drop.

### Phase 6e: Fixed-point (CLOSED)
- (x*257+32768)>>16 blend, +zbb, damage pixel budget degrade

### Phase 7: Golden + perf gates (CLOSED)
- 6 golden tests, 2 perf gates, QEMU markers, pipeline chain tests

### Phase D.0-D.3: Kernel timer (ALL DONE)
- RFC-0062 (353 lines)
- HartTimers with BTreeMap queue (7 tests)
- CapabilityKind::Timer + rights
- timer_create/set/cancel syscalls + ABI wrappers
- process_timer_expiry + IRQ dispatch
- windowd vsync_timer_slot + cleanup
- 52 Context::new test sites + 4 OS call sites updated

### Production UI End Architecture

| Workstream | Progress |
|-----------|----------|
| 1. Remove CPU compositing | 85% (GPU blur path wired) |
| 2. Present ring | 85% (4-plane VMO, slot tracking) |
| 3. Resource model | 60% (budgets + handles defined) |
| 4. Blur by architecture | 50% (GPU BlurBackdrop active, CPU fallback) |
| 5. Cursor GPU-first | 85% (unchanged) |
| 6. Unified pacing | 85% (kernel timer done, slot infra ready) |
| 7-8. DSL/SystemUI | 0% (future) |
| **Aggregate** | **63%** |

## Pending

- ⬜ QEMU visible-bootstrap (requires GTK display — unavailable in CI runner)
- ⬜ PRESENT_DONE events (gpud async completion channel)
- ⬜ Cursor hardware upload (Phase 6)
- ⬜ Unified pacing with slot switching (Phase 7)