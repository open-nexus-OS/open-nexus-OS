# Handoff — TASK-0062 Phase 6c CLOSED

Date: 2026-06-05

## Status

QEMU visible-bootstrap: ⬜ pending re-test (handoff fix + BlitSurface path)
gpud tests: ✅ 20/20
nexus-gfx tests: ✅ 7/9 (2 pre-existing perf::timer failures)

## Architecture

```
 VMO (8MB) = DATA PLANE              windowd heap (768KB) = CONTROL PLANE
 rows 800-1599: display scanout        scene graph, layout, animations
 rows 0-799:   wallpaper source        band_scratch 200KB, shadow arena 8KB
                                       glass cache 55KB, IPC buffers
 ─────────────────────────────         ──────────────────────────────
 gpud: BlitSurface + TRANSFER+FLUSH   windowd: build CommandBuffer per frame
                                       windowd: CPU overlay compositing
```

## What was done (2026-06-05)

### Phase 6c CLOSED
- gpud.submit() executes all 6 command types against VMO
- External VMO mapped into gpud VA space
- Double-height VMO: wallpaper bottom half, display top half
- gpud SET_SCANOUT (0,800,1280,800)
- send_blit_surface_cb() for wallpaper damage via GPU
- write_source_frame_to_vmo() moves 4MB from heap to VMO
- DISPLAY_OFFSET_BYTES on all display vmo_write calls
- ROW_WRITE_CHUNK 4→40 (10× fewer vmo_write: 200→20)
- Heap 512KB→768KB, actual usage ~500KB

### Phase 6d foundation
- Honest fence lifecycle, 5 unit tests
- present_seq + frames_in_flight tracking

### Phase D.1
- Deadline-driven VSync: Wait::Blocking/Wait::Timeout (was yield_())

### Marker fix
- emit_v3b_markers() from flush_pending_damage() after real rendering

## Key files

| File | Lines | Change |
|------|-------|--------|
| source/drivers/gpud/src/backend.rs | +310 | VMO mapping, commands, rendering, fence, scanout offset |
| source/services/windowd/src/compositor/mod.rs | +45 | Constants, 8MB VMO, VSync loop, handoff wait |
| source/services/windowd/src/compositor/runtime.rs | +55 | Source write, BlitSurface CB, offsets, tracking |
| source/drivers/gpud/src/service.rs | +2 | RESOURCE_HEIGHT |
| source/services/windowd/Cargo.toml | 1 | heap-768k |
| userspace/nexus-gfx/src/core/fence.rs | +44 | pub signal, 5 tests |
| userspace/nexus-gfx/src/backend/cpu_mock.rs | +3 | Honest fence |

## Next steps

1. QEMU re-test: `just test-os visible-bootstrap`
2. Phase 6d: double-buffer VMO swap (OP_SWAP_BUFFERS)
3. Phase 6e: RISC-V fixed-point rendering
4. Kernel timer: new RFC + timer_create/set/cancel syscalls
5. Phase 7: golden tests + perf regression gates
