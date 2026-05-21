<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Display Output Service Chain

The live visible-output path is service-owned:

`hidrawd -> inputd -> windowd -> fbdevd -> ramfb`

`selftest-client` is only an out-of-band observer. It polls `fbdevd` for
`VisibleState` and emits proof markers only after the service state already
contains the required evidence.

## Authority Boundaries

- `hidrawd` owns hardware ingress and normalized HID delivery.
- `inputd` owns normalized pointer/keyboard state and delivery accounting. It
  sends bounded visible-input updates to `windowd`; it does not own scene or
  cursor pixels.
- `windowd` is the Minimal DisplayServer v0. It owns root scene state,
  hit-test/focus, and the full NeX UI rendering pipeline (RFC-0058 Phase 6):

  - **Two-pass retained-mode compositor**: shadow-pass (`compute_shadow_row` via
    `nexus_effects::blur_1d`) → content-pass (`draw_proof_surface_row` with
    backdrop blur via `nexus_effects::blur_1d`) → cursor — zero-copy, per-row.
  - **Tile-based damage tracking**: `TileMap` (64×64 tiles, 260 tiles, bit-array)
    gates band writes in `write_rows` via `has_dirty_in_row_range`; dirty rects
    unioned in `pending_damage_rects`.
  - **Retained layer cache**: `LayerCache` (insert/get/invalidate) stores pre-rendered
    box pixels; `draw_layout_box_row` blits clean layers, skips re-render.
  - **Cursor save/restore**: `save_cursor_bg_inline` captures wallpaper before cursor
    blend; `restore_cursor_bg` writes saved pixels back on cursor move.
  - **Paint-only fast-path**: `paint_only` flag skips non-paint boxes and backdrop
    blur on hover/click/keyboard color changes.
  - **MSDF atlas** (`nexus-msdf`): 95 ASCII glyphs as 32×32 SDF, scale-agnostic.
  - **SDF shapes** (`nexus-sdf`): anti-aliased circles, rounded rects via analytical SDF.
  - **Effects** (`nexus-effects`): `blur_1d` used for backdrop + shadow blur in
    compositor; separable blur, 9-slice shadow, dual-kawase blur available.

  Writes composed rows into the framebuffer VMO registered by `fbdevd`.
- `fbdevd` owns framebuffer capability use, `ramfb` setup, final scanout
  ownership, and visible-state replies. It does not own scene composition or a
  second cursor truth.
- `init-lite` owns capability routing and endpoint rights.

## Minimal Userspace Reactor

Long-lived display/input work runs in service-owned userspace reactors, not in
the kernel and not in `selftest-client`.

For the live display path, `fbdevd` drives a small budgeted scanout observer tick:

- drain bounded service requests,
- register its framebuffer VMO with `windowd` by `CAP_MOVE`,
- sample composed `VisibleState` through a short bounded `windowd` RPC,
- update scanout/telemetry from service-owned evidence,
- yield cooperatively.

The reactor must not let a slow upstream poll block a display refresh for a full
frame budget. If `windowd` does not answer quickly, `fbdevd` keeps ownership of
the last observed state and tries again on the next tick.

Cursor-only movement is the latency-sensitive case. `inputd` forwards bounded
visible-input updates to `windowd`, `windowd` recomposes only the damaged rows
of the Mocu SVG cursor over the root scene, and `fbdevd` only reports the
dirty-row/flush evidence it observes.

The coordinate contract follows normal screen-space direction:

- positive relative X moves the cursor right,
- negative relative X moves the cursor left,
- positive relative Y moves the cursor down,
- negative relative Y moves the cursor up.

`inputd` owns the canonical pointer state in physical display coordinates and
never turns it into cursor pixels. `windowd` remains hit-test/focus and
composition authority; the visible framebuffer consumes only rows composed by
the DisplayServer.

This intentionally mirrors the OpenHarmony/OHOS split: pointer events carry a
screen/display-relative position for global routing and a window/component
relative position for delivery. Our current minimal version now keeps the
canonical state in display space, maps absolute devices across the full visible
bootstrap mode, transforms to window-space only for `windowd` delivery, and
derives hover from the routed proof-scene position instead of from a framebuffer
scale-back shortcut.


## DisplayServer v0 Asset Pipeline (TASK-0057)

The cursor rendering follows the OHOS hardware-cursor model mapped to software,
with `windowd` as the single display-scene authority:

1. **windowd** (DisplayServer authority): composes the root scene.
   - SVG source: `resources/cursors/mocu/src/svg/default.svg` (Mocu theme, CC0),
     build-normalized for the bounded OS SVG renderer
   - Wallpaper source: `resources/wallpapers/base/default.jpeg`
   - Text source: `resources/fonts/inter/docs/font-files/InterVariable.ttf`,
     build-rasterized as an Inter proof overlay for the OS path
   - Rendering: `nexus-svg` cursor raster output + JPEG-sourced wallpaper
     + deterministic Inter text/icon proof targets
   - Composition target: framebuffer VMO registered by `fbdevd`

2. **inputd** (input authority): supplies bounded input updates.
   - Tracks `display_pointer_position()` from HID events
   - Sends `OP_UPDATE_VISIBLE_STATE` to `windowd`
   - Does not render the cursor or own a display scene

3. **fbdevd** (scanout authority): owns the framebuffer and ramfb device.
   - Allocates the framebuffer VMO and sends a cloned capability to `windowd`
   - Serves observer-visible state after it sees `windowd` asset/overlay evidence
   - Emits `fbdevd: cursor overlay on` only after the DisplayServer-composed cursor
     is visible in service state

4. **selftest-client** (observer): validates cursor markers.
   - `windowd: cursor svg loaded` — cursor bitmap successfully rasterized
   - `windowd: wallpaper visible` — JPEG-sourced wallpaper is in the root scene
   - `windowd: text target visible` / `windowd: icon target visible` — v2b proof
     targets are composed by `windowd`
   - `fbdevd: cursor overlay on` — scanout observed the DisplayServer cursor
   - `SELFTEST: ui v2b assets ok` — all asset targets verified

### Contract Tests

| Test | Crate | Verifies |
|---|---|---|
| `cursor_svg_renders_non_empty` | nexus-svg | CURSOR_LEFT_PTR_SVG → non-zero pixels |
| `update_visible_state_rejects_response_and_truncated_frames` | input-live-protocol | DisplayServer-v0 input frame rejects |
| `observer_state_latches_displayserver_asset_evidence` | fbdevd | scanout observer latches service-owned asset evidence |
| `blend_cursor_row_replaces_opaque_pixels` | fbdevd | Opaque cursor replaces destination |
| `blend_cursor_row_skips_transparent_pixels` | fbdevd | Transparent pixels don't overwrite |
| `blend_cursor_row_ignores_out_of_bounds` | fbdevd | OOB cursor position is safe |

### Automated vs Live Proof

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os visible-bootstrap` is the
  automated injected-input proof. It must stop at
  `SELFTEST: ui v2b assets ok`, not the older wheel marker.
- `just start` is the live interactive proof. It should show the same
  DisplayServer scene: JPEG wallpaper, SVG cursor, text/icon targets, and live
  pointer movement.
- Proof target highlighting is transient: hover is active only while the routed
  pointer is over the target, click only while primary pointer is held, keyboard
  only while a non-modifier key is held, and wheel pulses distinguish up/down.
- White cursor-square proof pixels are legacy host affordances only; they are
  not accepted as the live mouse truth in the DisplayServer chain.

## Minimal Closure Rule

Every display-output fix must identify the first broken hop and add the smallest
service-level proof for that hop before relying on QEMU:

1. capability route/rights,
2. protocol request/reply,
3. owner service state transition,
4. downstream telemetry/output,
5. observer marker.

If QEMU reports a stable missing marker while host tests are green, the green
tests are incomplete for this hop.