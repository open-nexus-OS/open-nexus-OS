<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Display Output Service Chain

The live visible-output path is service-owned (GPU-only architecture, RFC-0059 Phase 6):

`hidrawd -> inputd -> windowd -> gpud (virtio-gpu)`

`selftest-client` is only an out-of-band observer. It polls `windowd` for
`VisibleState` and emits proof markers only after the service state already
contains the required evidence.

> **Current structure (gfx/driver idealstruktur, Gates 1–4, 2026-06).** The windowd↔gpud wire is the
> `nexus-display-proto` SSOT (opcodes + control frames) carrying a serialized `nexus_gfx::CommittedBuffer`
> payload — one definition, imported by both ends (`docs/adr/0038-display-wire-ssot-and-capnp-boundary.md`).
> CPU/VMO rasterization is the one canonical rasterizer in `userspace/nexus-gfx/src/raster/`, shared by
> the `cpu_mock` reference and gpud's CPU path (RFC-0067). gpud sits on the `nexus-virtio` bus-HAL +
> `nexus-driverkit` submit/fence substrate; the whole device-class layering is
> `docs/adr/0039-device-class-driver-architecture.md`. Some sections below describe the earlier Phase-6
> path (fbdevd, the two-pass `blur_1d` compositor) and are partly historical.

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

  Writes composed rows into the framebuffer `gpud` GRANTED it (RFC-0098 C7: asked once at
  start with `OP_FRAMEBUFFER_REQUEST`, answered with the mode and a clone of the object) and
  attaches it (`OP_SET_FRAMEBUFFER_VMO`, no capability) for zero-copy GPU scanout.
- `gpud` is the display driver and the display-mode authority: probes the device, decides
  the visible mode (the lane's request from its own tree slot, the device's capability, one
  policy), makes the framebuffer FOR its device and grants it, performs `ATTACH_BACKING` +
  `SET_SCANOUT`. It does not own scene composition or a second cursor truth. Since TASK-0251
  P2a step 2 (2026-10-04) its ONE request loop (`service.rs`: the wire, the chain trace, the
  stats, the reveal latch) drives ONE display behind `backend::display::Display` — the virtio
  GPU on QEMU (`backend/virtio_display.rs`: its 2D scanout or its virgl GL scanout) or the
  board's display controller (`backend/dc/`: a contiguous framebuffer made for the controller,
  each present's damage cleaned out of the caches, the boot splash held until the first
  present after windowd's reveal and then switched to the display plane). A present's commands
  run through ONE CPU executor (`backend/cpu_frame.rs`) on every display that composites on the
  CPU — the 2D scanout, the virgl path's per-command fallbacks, the controller. On the board
  the mode is the CEA timing of the layout maximum, 1920x1080@60 (`gpud: dc scanout ok
  (1920x1080@60 cea …)`), until the EDID read over DDC lands with the GPU lane (RFC-0098 C7 as
  amended 2026-10-10); the pointer is the controller's own layer (`gpud: dc cursor layer ok`,
  a move writes the layer's rectangle — no present per move, TASK-0251 step 3c).
- `init-lite` owns capability routing and endpoint rights.

## GPU-only Display Architecture (RFC-0059 Phase 6)

The display path follows a one-owner/one-path compositor pattern:

```
windowd (scene + present authority)
  │
  ├── OP_FRAMEBUFFER_REQUEST → gpud ─ grant: mode + framebuffer (gpud's)
  ├── compose frames into the granted framebuffer
  ├── OP_SET_FRAMEBUFFER_VMO → gpud (attach, no cap)
  ├── OP_GET_DISPLAY_SPACE ← inputd (once, over inputd's reply inbox)
  └── OP_UPDATE_VISIBLE_STATE ← inputd
        │
        ▼
┌──────────────────┐
│  gpud (driver)    │
│  probe virtio-gpu │
│  ATTACH_BACKING   │
│  SET_SCANOUT      │
│  move_cursor      │
└──────────────────┘
```

No fbdevd, no ramfb — one owner per thing: `gpud` owns the mode and the scanout memory
(made for the scanout device, which only its holder can do), `windowd` owns the scene and
every present. A stack without `gpud` is named (`windowd: display none (…)`) and runs
display-less at the layout maximum.

## Input Fast-Path

Cursor-only movement is the latency-sensitive case. `inputd` forwards bounded
visible-input updates to `windowd` via `OP_UPDATE_VISIBLE_STATE`, `windowd`
recomposes only the damaged rows of the Mocu SVG cursor over the root scene,
and the compositor produces a minimal present acknowledgement.

`windowd` applies one staged sample per frame (`input_stage`): motion coalesces to
the newest position, wheel notches sum, one-shot facts (a tiling chord, a capture key)
survive a newer sample — and a primary-button edge ends the batch, so a click held for
less than one busy frame keeps its press at its own position (RFC-0055's semantic-edge
integrity; `tests/input_staging.rs`).

The coordinate contract follows normal screen-space direction:

- positive relative X moves the cursor right,
- negative relative X moves the cursor left,
- positive relative Y moves the cursor down,
- negative relative Y moves the cursor up.

`inputd` owns the canonical pointer state in physical display coordinates and
never turns it into cursor pixels. `windowd` remains hit-test/focus and
composition authority; the visible framebuffer consumes only rows composed by
the DisplayServer.

This intentionally uses a split: pointer events carry a
screen/display-relative position for global routing and a window/component
relative position for delivery. Our current minimal version now keeps the
canonical state in display space, maps absolute devices across the full visible
bootstrap mode, transforms to window-space only for `windowd` delivery, and
derives hover from the routed proof-scene position instead of from a framebuffer
scale-back shortcut.


## DisplayServer v0 Asset Pipeline (TASK-0057)

The cursor rendering follows a hardware-cursor model mapped to software,
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