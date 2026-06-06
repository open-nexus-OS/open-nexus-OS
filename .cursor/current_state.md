# Current State — Open Nexus OS

Last updated: 2026-06-05

## Active focus

**TASK-0062 Phase 6c CLOSED: OHOS-style control/data plane separation**

## Architecture (current)

```
 VMO (8MB, 1280×1600):                      windowd heap (768KB):
   rows 800..1599 → display scanout            scene graph, layout, animations
   rows   0.. 799 → wallpaper source           band_scratch (200KB), shadow arena (8KB)
                                                glass cache (55KB), IPC buffers
 DATA PLANE  (gpud renders, windowd writes)   CONTROL PLANE (~500KB used / 768KB)
```

## What's done (2026-06-05)

### Phase 6c: GPU rendering pipeline
- `VirtioGpuBackend::submit()` executes all 6 command types against VMO backing
- External VMO mapped into gpud VA space via `vmo_map_page`
- ~230 lines VMO-backed rendering primitives (zero-heap, stack-only)
- `send_blit_surface_cb()` builds BlitSurface commands for wallpaper damage
- Double-height VMO: wallpaper in bottom half, display in top half
- gpud SET_SCANOUT offset (0, 800, 1280, 800)
- `DISPLAY_OFFSET_BYTES` on all vmo_write calls (display writes to top half)
- `write_source_frame_to_vmo()` moves wallpaper 4MB from heap to VMO during init
- Animation DrawTiles active (was no-op)

### Phase 6d: Honest fence lifecycle
- `Fence::signal()` public; `submit()` returns pending, signals after execute
- 5 fence unit tests; `present_seq` + `frames_in_flight` tracking

### Phase D.1: Clean VSync event loop
- Replaced `yield_()` with `Wait::Blocking`/`Wait::Timeout` using kernel deadline
- Idle: `Wait::Blocking` (zero CPU). Active: `Wait::Timeout(8.3ms)` (120Hz).
- Handoff exception: `Wait::NonBlocking` while first-frame handoff pending
- Removed unused `yield_` import

### vmo_write reduction
- Windowd heap: 512KB → 768KB (`heap-512k` → `heap-768k`)
- `ROW_WRITE_CHUNK`: 4 → 40 (10× fewer vmo_write per damage rect: 200→20)

### Marker fix
- `emit_v3b_markers()` fires from `flush_pending_damage()` after real rendering

## Test status

| Suite | Result |
|-------|--------|
| gpud (20 tests) | ✅ 20/20 |
| nexus-gfx fence (5 tests) | ✅ 5/5 |
| nexus-gfx perf::timer (2 tests) | ❌ pre-existing |
| QEMU visible-bootstrap | ⬜ pending re-test |

## Files changed

| File | Lines | Change |
|------|-------|--------|
| `source/drivers/gpud/src/backend.rs` | +310 | VMO mapping, 6-command execution, rendering primitives, honest fence, scanout offset, present_damage offset |
| `source/services/windowd/src/compositor/mod.rs` | +45 | Constants, double-height VMO, clean VSync loop, handoff-aware wait |
| `source/services/windowd/src/compositor/runtime.rs` | +55 | write_source_frame_to_vmo, send_blit_surface_cb, DISPLAY_OFFSET_BYTES, is_handoff_pending, marker fix, in-flight tracking |
| `source/drivers/gpud/src/service.rs` | +2 | RESOURCE_HEIGHT constant |
| `source/services/windowd/Cargo.toml` | 1 | heap-512k → heap-768k |
| `userspace/nexus-gfx/src/core/fence.rs` | +44 | pub signal, 5 unit tests |
| `userspace/nexus-gfx/src/backend/cpu_mock.rs` | +3 | honest fence pattern |

## Pending

- ⬜ QEMU re-test (handoff fix + BlitSurface path)
- ⬜ Phase 6d: double-buffer VMO swap (needs OP_SWAP_BUFFERS protocol)
- ⬜ Phase 6e: RISC-V fixed-point rendering in backend
- ⬜ Phase 7: golden tests + perf regression gates (blocked on kernel timer)
- ⬜ Kernel timer capability package (new RFC + syscalls, 6-8d)
