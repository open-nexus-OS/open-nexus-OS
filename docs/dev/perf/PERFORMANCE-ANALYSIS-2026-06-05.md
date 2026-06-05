# Performance Analysis — open-nexus-OS TASK-62 / RFC-59

**Date:** 2026-06-05  
**Scope:** Input-to-display pipeline latency, CPU rendering, virtio-gpu integration  
**Severity:** CRITICAL — 2 FPS compositor, 468ms avg frame time

---

## 1. Measured Performance (from `build/logs/visible-bootstrap--2026-06-05T11-30-45/uart.log`)

```
fps: windowd compose_hz=2 present_hz=2 coalesced=0 dropped=0
     damage_px=1825904 avg_render_us=467936 max_render_us=2281805

fps: windowd compose_hz=0 present_hz=0 coalesced=0 dropped=0
     damage_px=810444 avg_render_us=1809223 max_render_us=2841747
```

- **Average render time: 468ms → 1.8s per frame**
- **Effective throughput: 2 FPS (first frame) → <1 FPS (subsequent)**
- QEMU `-icount` not active → real wall-clock time
- Single RISC-V hart (SMP=0)

For comparison, OHOS/Fuchsia targets: 12–16ms per frame (60 FPS) on comparable virtio-gpu QEMU configs.

---

## 2. Root Cause #1: Double Rendering (CPU → VMO + GPU → CommandBuffer)

### Location
`source/services/windowd/src/compositor/runtime.rs` — `flush_pending_damage()` (line 1974–2049)

### What happens on EVERY frame with damage:

```
Phase A: CPU compositing (lines 1994–1999)
  for each damage rect:
    write_damage_rect() → copy_scene_row() → vmo_write()
    ↑ Renders every row of every damage rect into the framebuffer VMO

Phase B: GPU command building (lines 2003–2038)  
  CommandBuffer::new() → try_begin_render_pass() → try_blit_surface() 
  → try_commit() → serialize_into() → send_gpud_status_request()
  ↑ Describes the SAME damage as GPU commands, sends via IPC
```

**The frame is computed twice:** once by CPU (actual pixel work), once as GPU metadata.

### What VirtioGpuBackend.submit() actually does

`source/drivers/gpud/src/backend.rs` line 393–406:

```rust
fn submit(&mut self, cmd: CommittedBuffer) -> Result<Fence, GfxError> {
    cmd.validate().map_err(map_nexus_error)?;
    // os-lite: reads command_count(), does NOTHING with commands
    let _ = cmd.command_count();
    Ok(Fence::new_signaled())  // ← immediately "done"
}
```

The GPU path is a **no-op pass-through**. All rendering is CPU-bound.

### IPC Overhead per frame:

| Operation | Direction | Size | Blocking |
|-----------|-----------|------|----------|
| send CommandBuffer + damage rects | windowd → gpud | ≤512 B | Blocking |
| recv status response | gpud → windowd | ≤5 B | Blocking |
| `vmo_write()` per band row | windowd → kernel | ≤5120 B | Synchronous |

Each `vmo_write()` syscall crosses the kernel boundary. With ROW_WRITE_CHUNK=4, a 800px-tall damage rect requires **200 syscalls** just for band writes.

---

## 3. Root Cause #2: Fake Success Markers

### Location: `runtime.rs` line 1583–1587

```rust
fn emit_v3b_markers(&mut self) {
    let _ = debug_println(crate::markers::EFFECTS_ON_MARKER);       // "windowd: effects on"
    let _ = debug_println(crate::markers::EFFECT_BLUR_OK_MARKER);   // "windowd: effect blur ok"
    let _ = debug_println(crate::markers::SELFTEST_UI_V3_EFFECT_OK_MARKER); // "SELFTEST: ui v3 effect ok"
}
```

Called unconditionally from `commit_first_frame()` (line 814). These markers fire on the **very first frame**, before any blur or shadow effect is actually evaluated. The selftest passes based on marker presence, not functional correctness.

### Location: `runtime.rs` line 1155–1163

```rust
if !self.selftest_v3b_emitted
    && self.live_scroll_marker_emitted
    && self.clipping_marker_emitted
    && self.filter_cycle > 0
{
    let _ = debug_println(SELFTEST_UI_V3_SCROLL_OK_MARKER);
    let _ = debug_println(SELFTEST_UI_V3_FILTER_OK_MARKER);
    let _ = debug_println(SELFTEST_UI_V3_IME_OK_MARKER);
    self.selftest_v3b_emitted = true;
}
```

- `clipping_marker_emitted` is set to `true` on the **first** filter text change (line 1174), before any clip is tested
- `filter_cycle > 0` is true on the first text input event
- No actual pixel comparison, rendering output comparison, or behavioral assertion

### Also: `gpud: cursor on` (line 854 in uart.log) is emitted unconditionally after framebuffer attach (backend.rs line 133), even though cursor rendering is a no-op on virtio-gpu without a cursor resource.

---

## 4. Root Cause #3: Non-Wired GPU-First Pipeline (Phase 6c)

### TASK-0062 specification (from `tasks/TASK-0062-ui-v5a-reactive-runtime-animation-transitions.md`):

```
Architecture invariant:
  windowd (Producer)                    gpud (Consumer/Renderer)
    build_frame_commands()                recv(Wait::Blocking)
    → CommittedBuffer                     → backend.submit(cb)
    → IPC (ONE per frame)                 → render into VMO
    NO vmo_write()                        → TRANSFER_TO_HOST_2D (once)
    NO CPU compositing                    → RESOURCE_FLUSH (once)
                                          → fence.signal()
```

**Status: NOT IMPLEMENTED.** The code has the `CommandBuffer` infrastructure, `CpuMockBackend` (host), and `VirtioGpuBackend` (OS), but:

1. `VirtioGpuBackend.submit()` is a no-op — doesn't execute any rendering commands
2. `CpuMockBackend` implements actual rendering (`blit`, `fill_sdf_rounded`, `blur_backdrop`, `blend_cursor`) but is **not wired into the OS path**
3. The compositor still calls `vmo_write()` directly
4. The `copy_scene_row()` CPU path is still the primary renderer

---

## 5. Root Cause #4: Input Hot-Path Amplification

### Every keystroke triggers:

1. `apply_visible_state()` → detects paint flag changes (line 1031)
2. `animation_driver.spring_to()` creates 1–3 spring animations (lines 1050–1104)
3. Each animation tick marks full `COMBINED_PANEL` as dirty (line 1322–1328)
4. `note_filter_text_changed()` marks 3 large filter panel rects (lines 1189–1191)
5. `flush_pending_damage()` does full CPU re-render + GPU command build + IPC

### Damage amplification:
- A single keystroke → text field change (≈200 px²)  
- → marks filter panel + filter input + filter list (≈200,000 px²)  
- → animation spring → marks full proof panel (≈600,000 px²)  
- **Amplification factor: ≈3000×**

---

## 6. Fix Plan

### Priority P0: Wire GPU-First Rendering (TASK-0062 Phase 6c)

**Action:** Replace the dual CPU+VMO/GPU+CommandBuffer path with a single GPU-only path.

1. **`VirtioGpuBackend.submit()`** must actually execute commands:
   - `Command::BlitSurface` → blit from source to framebuffer backing VMO
   - `Command::FillSdfRoundedRect` → software SDF fill (or skip with clear rect)
   - `Command::BlurBackdrop` → use `nexus_effects::blur_1d` on the relevant rect
   - `Command::BlendCursor` → blend cursor bitmap into framebuffer
   - `Command::DrawTiles` → fill tile rects with fragment-derived color

2. **`flush_pending_damage()`** must NOT call `write_damage_rect()`:
   - Build CommandBuffer with all damage regions
   - Send ONE IPC to gpud
   - gpud renders everything into the framebuffer VMO
   - gpud calls TRANSFER_TO_HOST + FLUSH **once**

3. **Remove all `vmo_write()` from windowd compositor**:
   - Lines 1773, 1881, 1889-1894, 2094, 2199 in runtime.rs
   - The only VMO write is gpud writing into the scanout backing

### Priority P1: Remove Fake Success Markers

Replace unconditional marker emission with actual test assertions:

1. `emit_v3b_markers()` should only fire when blur/shadow rendering produces a checksum that matches a golden value
2. v3b selftest summary should only fire when a pixel buffer comparison passes
3. `gpud: cursor on` should only fire when cursor position actually changed in the scanout

### Priority P2: Reduce Input-Hot-Path Amplification

1. **Coalesce animation damage**: Don't mark the full panel on every animation tick — only mark the changed regions
2. **Separate animation overlay from content**: Animate sidebar glass as a separate pass that doesn't invalidate the proof panel
3. **Input batching**: Batch multiple keystrokes before triggering re-render (currently each keystroke triggers individually)
4. **Avoid spring animations on non-visual state changes**: Filter text changes don't need spring interpolation

### Priority P3: Eliminate Redundant Data Structures

1. `observer_state` (line 643-673) duplicates all fields from `state` — remove and use `state` directly
2. `pending_damage_rect` + `pending_damage_rects` — two separate damage queues that get merged; unify into one
3. `first_handoff_*` fields (lines 629-635) are a 7-field handoff state machine that could be a single enum

---

## 7. Expected Performance After Fixes

| Metric | Current | After P0 | After P1+P2 |
|--------|---------|----------|-------------|
| Frame time (avg) | 468ms | ~30ms | ~8ms |
| Frame time (max) | 2.8s | ~50ms | ~16ms |
| Effective FPS | 2 | ~30 | ~60 |
| vmo_write calls/frame | 200+ | 0 (gpud only) | 0 |
| IPC round-trips/frame | 2 (cmd + status) | 1 | 1 |
| CPU cycles/frame | 6.7M+ | ~1.5M | ~0.5M |

---

## 8. Architecture Comparison

### Current (broken):
```
inputd → windowd.apply_visible_state()
  → CPU: copy_scene_row() per row → vmo_write() per band
  → GPU: build CommandBuffer → IPC → gpud.submit() [no-op]
  → IPC: send status → recv response
```

### Fixed (P0):
```
inputd → windowd.apply_visible_state()
  → build ONE CommandBuffer with all damage
  → IPC → gpud.submit()
    → execute commands into framebuffer VMO
    → TRANSFER_TO_HOST + FLUSH once
    → fence.signal()
  → 0 vmo_write() from windowd
```

---

## 9. Files Requiring Changes

| File | Change |
|------|--------|
| `source/drivers/gpud/src/backend.rs` | Implement `submit()` command execution |
| `source/drivers/gpud/src/service.rs` | Wire full CommandBuffer execution in IPC handler |
| `source/services/windowd/src/compositor/runtime.rs` | Remove CPU compositing from `flush_pending_damage()`, remove fake markers, fix `emit_v3b_markers()` |
| `source/services/windowd/src/compositor/mod.rs` | Remove `vmo_write` import, update IPC loop |
| `source/services/windowd/src/compositor/scene.rs` | Remove `copy_scene_row()` (move to backend) |
| `source/services/windowd/src/markers.rs` | Add `#[deprecated]` on fake marker constants |
| `tasks/TASK-0062-*.md` | Update status |
| `docs/rfcs/RFC-0059-*.md` | Update implementation status |
