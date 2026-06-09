# Hardening Plan: Remove CPU Compositing, Unify GPU Pipeline

Date: 2026-06-09
Status: Planning — not yet started

## Principle

Every phase follows: **chain test first → implement → verify chain test → clean old code**.
No reverting on error — fix forward. No old paths kept "just in case."

## Current Double Structure

```
flush_pending_damage()
├── write_damage_rect()          ← CPU: copies wallpaper rows, draws SDF panels,
│   ├── copy_scene_row()             CPU blur, CPU shadow, text, icons
│   │   ├── draw_proof_surface_row() → vmo_write() to Plane 2
│   │   ├── dark_glass_row()         ← CPU BLUR (double-blur!)
│   │   └── compute_shadow_row()     ← CPU SHADOW
│   └── vmo_write()
├── submit_cursor_to_gpud()      ← GPU: hardware cursor position
└── present_frame_with_gpu_blur() ← GPU: BlurBackdrop overlay (double-blur!)
    └── send_gpud_present()
```

## Target Architecture

```
flush_pending_damage()
├── build_frame_command_buffer()  ← GPU: one CB, all commands
│   ├── BlitSurface               (wallpaper from Plane 0)
│   ├── FillSdfRoundedRect        (panel backgrounds)
│   ├── StrokeSdfRoundedRect      (panel borders)
│   ├── BlurBackdrop              (glass — single blur, no double)
│   └── Group { shadow }          (panel shadows)
├── submit_cursor_to_gpud()       ← GPU: hardware cursor position (unchanged)
└── send_gpud_present(cb_frame)   ← GPU: fire-and-forget (unchanged)
```

## Phases

### Phase A1: Remove CPU Blur (30 min)

**Files:** `runtime.rs`, `backdrop.rs`

**What changes:**
- `copy_scene_row` (in `scene.rs`) calls `dark_glass_row` for glass panel rows
- Remove the call — GPU `BlurBackdrop` already handles this
- `write_rows` no longer passes `glass_quality` parameter to row compositing
- Keep `GlassQuality` enum for GPU blur parameters (radius, saturation)

**After:** Single blur in GPU path. No more double-blur.

**Chain test:** `chain_no_double_blur` — verify `dark_glass_row` is never called, `BlurBackdrop` is the only blur command.

**Delete:** `dark_glass_row()` function in `backdrop.rs`

### Phase A2: Replace CPU Panel Rendering (1h)

**Files:** `runtime.rs`, `surface.rs`, `shadow.rs`

**What changes:**
- Instead of `draw_proof_surface_row` per-panel-per-row, build `FillSdfRoundedRect` + `StrokeSdfRoundedRect` commands in the CB
- Instead of `compute_shadow_row` per-panel-per-row, add `Group { shadow }` to the panel node
- `write_rows` becomes `build_frame_cb` — no vmo_write, no row loops

**Key simplification:** The proof panel has fixed dimensions. Panel rects are known at init time. Build a static CB template once, clone per frame:
```rust
fn build_proof_panel_cb(&self) -> CommittedBuffer {
    let mut cb = CommandBuffer::new();
    // Wallpaper blit
    encoder.try_blit_surface(0, 0, 1280, 800, 0, 0);
    // Panel backgrounds
    encoder.try_fill_sdf_rounded_rect(panel_rect, radius, color);
    // Panel borders
    encoder.try_stroke_sdf_rounded_rect(panel_rect, radius, stroke_width, border_color);
    // Glass blur
    encoder.try_blur_backdrop(glass_rect, radius, saturation);
    // Shadow
    encoder.try_group_with_shadow(panel_rect, shadow_offset, shadow_blur, shadow_color);
    cb.try_commit()
}
```

**After:** No CPU SDF rendering. Panel rendering is GPU-side.

**Chain test:** `chain_gpu_panel_rendering` — verify `FillSdfRoundedRect` and `StrokeSdfRoundedRect` commands in CB, no `draw_proof_surface_row` calls.

**Delete:** `surface.rs`, `shadow.rs`

### Phase A3: Remove CPU Wallpaper Copy (30 min)

**Files:** `runtime.rs`, `source.rs`

**What changes:**
- `copy_scaled_systemui_row_clipped` copies wallpaper from heap `source_frame` to `band_scratch`
- Replace with `BlitSurface` from VMO Plane 0 (wallpaper) to display region
- `write_source_frame_to_vmo` already writes wallpaper to VMO offset 0
- The CB's first command is `BlitSurface { src_x:0, src_y:0, width:1280, height:800, dst_x:0, dst_y:0 }`
- No more per-row copy — one GPU command

**After:** wallpaper rendering is zero-copy GPU blit.

**Chain test:** `chain_gpu_wallpaper_blit` — verify `BlitSurface` from Plane 0 to display region.

**Delete:** `source.rs`

### Phase A4: Remove `write_damage_rect` Entirely (30 min)

**Files:** `runtime.rs`, `scene.rs`

**What changes:**
- Remove `write_rows`, `write_damage_rect`, `write_fast_bootstrap_frame`
- `flush_pending_damage` now: build CB → submit cursor → send present
- `build_frame_cb` generates the full CB from damage rects + panel spec

**After:** No CPU compositing path remains in the steady-state frame loop.

**Chain test:** `chain_no_cpu_compositing` — verify no `vmo_write` calls in frame path, only `send_gpud_present`.

**Delete:** `scene.rs`

### Phase B: Remove Legacy Animation Path (20 min)

**Files:** `runtime.rs`, `gpud/service.rs`

**What changes:**
- Delete `submit_animation_to_gpud()` — animations use pacer-driven frame path
- Delete `GPU_ANIMATION_SUBMIT_OP` constant
- Delete `handle_frame()` in gpud service — all opcodes handled in main match
- Delete `OP_SUBMIT_ANIMATION_FRAME` handler in gpud

**Chain test:** `chain_no_animation_opcode` — verify `OP_SUBMIT_ANIMATION_FRAME` never sent.

### Phase C: Wire Scene Graph (1h)

**Files:** `runtime.rs`, `scene_graph.rs`, `systemui_shell.rs`

**What changes:**
- `DisplayServerRuntime` owns a `SystemUiShell`
- `apply_input_state` calls `shell.update_cursor(x, y)` → marks scene graph dirty
- `flush_pending_damage` calls `shell.graph.compute_dirty_set()` → generates CB from dirty nodes
- `SceneGraph::generate_commands()` implements CB generation from scene nodes

**After:** Scene graph is the single source of truth for rendering. All UI updates go through the graph.

**Chain test:** `chain_scene_graph_drives_rendering` — verify CB generated from scene graph dirty set.

### Phase D: Frame Fencing + Double Buffering (40 min)

**Files:** `runtime.rs`, `mod.rs`, `gpud/backend.rs`

**What changes:**
- `send_gpud_present` increments `present_seq` → gpud response carries seq
- `note_present_completed` toggles `current_display_slot`
- `build_frame_cb` writes to `current_display_offset()` (slot A or B)
- gpud calls `SET_SCANOUT` on slot switch (new opcode: `OP_SWAP_BUFFERS`)
- `max_in_flight()` returns 2 (double-buffered)

**After:** True double-buffered rendering. No tearing. Pipelined frames.

**Chain test:** `chain_double_buffered_present` — verify slot toggle on present completion.

### Phase E: Clean Up Dead Code (20 min)

**Delete these files:**
- `source/services/windowd/src/compositor/scene.rs` — CPU row compositing
- `source/services/windowd/src/compositor/backdrop.rs` — CPU blur
- `source/services/windowd/src/compositor/shadow.rs` — CPU shadow
- `source/services/windowd/src/compositor/surface.rs` — CPU SDF rendering
- `source/services/windowd/src/compositor/source.rs` — CPU wallpaper copy

**Remove from mod.rs:**
- `mod scene;` `mod backdrop;` `mod shadow;` `mod surface;` `mod source;`

**Remove unused imports** across all files.

**Chain test:** All existing chain tests + new phase tests must pass.

## Total: ~4.5 hours of focused work

## Acceptance Criteria

After Phase E:
1. `cargo test -p windowd -p gpud -p nx` — all pass
2. `cargo check --target riscv64 --features os-lite` — zero new warnings
3. `just dep-gate` — no forbidden crates
4. `just diag-os` — zero errors
5. `make build && just test-os visible-bootstrap` — clean run, cursor moves visible
6. No `write_damage_rect`, no `vmo_write` in steady-state frame path
7. No `copy_scene_row`, no `dark_glass_row`, no `compute_shadow_row`
8. Single CommandBuffer per frame, generated from scene graph
