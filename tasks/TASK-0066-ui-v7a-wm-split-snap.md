---
title: TASK-0066 UI v7a: window tiling — halves, quarters, fill, return, arrangements; edge/corner drag, zoom-button hover menu, keyboard chords, settings "Fenster"; zone state in the window feed
status: Done (2026-10-07 — model "A" delivered and board-proven over two cycles; polish continues in the GPU lane per user decision; supersedes the 2026-09-09 halves+thirds rewrite)
owner: @ui
created: 2025-12-23
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - UI v6a WM baseline: tasks/TASK-0064-ui-v6a-window-management-scene-transitions.md
  - UI v3a layout baseline (for future tiling): tasks/TASK-0058-ui-v3a-layout-wrapping-deterministic.md
  - Policy as Code (WM constraints): tasks/TASK-0047-policy-as-code-v1-unified-engine.md
  - Config broker (WM keys): tasks/TASK-0046-config-v1-configd-schemas-layering-2pc-nx-config.md
  - Testing contract: scripts/qemu-test.sh
---

## Delivered 2026-10-06 (model "A"; proof chain in the table at the end)

- **P0**: the 2026-07-29 wedge path (drag release at the top edge → `toggle_fullscreen`
  mid-drag) no longer exists — the top edge FILLS through `apply_window_frame`; fullscreen is
  reached only from the app-icon menu / zoom control. RFC-0086 + RFC-0053 amended, ADR-0069,
  `docs/dev/ui/patterns/wm-tiling.md` (the `wm-snap.md` stub never existed; references fixed).
- **P1**: `windowd/src/zones.rs` (pure; inline tests) + `runtime/tiling.rs` (`apply_zone`,
  `return_window`, `apply_release_tile`, `untile_for_drag`, `apply_zone_code`, `arrange`,
  `reflow_tiled`, `apply_chord`), `CONTROL_WIN_ZONE = 9`, `snap.rs` deleted, split mode → left
  half, markers registered, `tests/tile_zones.rs`.
- **P2**: settingsd `ui.tile.edges|margin|chords`; windowd applies them from the `ui.` watch
  (margin change reflows); reflow on shell-profile change.
- **P3**: inputd `ModifierState` tracks Super + Left Alt; `wm_chord_for` fixed table; the chord
  key is consumed (no keyboard dispatch) and parked for one state push
  (`VisibleState.wm_chord`, `STATE_LEN` 62 → 63); windowd `apply_chord` on the focused window
  (`ui.tile.chords` gate, `no-focus` deny). `tests/tiling_chords.rs` (+ `test_reject_*`).
- **P4**: window-kit `WinTileGlyph` + `WinAppMenu`; settings `MoreMenu` and stash `StashPage`
  mount it (their duplicated panels are gone); settings `tiling.store.nx` + the "Fenster" group
  in Personalisierung; i18n ×5 in settings and stash; feed bits (`surface_windows::window_zone`; the app-host accessor lands with its first shell consumer — the warning gate forbids a dead one); goldens
  `kit_window_menu_{light,dark}`; `settings_tiling_menu.rs` (every tile forwards its verb, the
  inert entry registers no handler).
- **P5**: `usb-visible` injector tiling phase (launcher → settings app → Super+Ctrl+← →
  Super+Ctrl+↓), lane requires the `wm-tile` rungs; board ack `tile` pending the cycle.
- **P6 (board cycle 1 feedback, 2026-10-07)**: (a) the window menu is the WINDOW's — `WinAppWindow`
  mounts `WinAppMenu` for every window-kit app (labels as props; settings `MoreMenu` and stash
  keep only their own `more…` menus and mount no backdrop above the kit's); the glyph rows carry
  no text headers (the glyph is the label). `window_menu_global.rs` (settings AND stash open the
  same eleven verbs, one backdrop; `test_reject_*` for a closed menu). (b) Corners: a release
  within `TILE_CORNER_PX = 64` of BOTH edges is the quarter — the pointer need not touch the
  4-px edge (it did at 1080p, and the corner was never recognized: no `wm tile (zone=top-…)` and
  no deny in the log). A no-zone release traces `windowd: wm release at (x,y)` (user-driven,
  bounded). (c) Drag preview: windowd pushes `OP_SURFACE_TILE_PREVIEW` (op 29, one push per
  candidate-zone change, 0 on release/none) to the DESKTOP surface; app-host turns it into the
  device axis `device.tilePreview`; the desktop shell's `overlays/TilePreview.nx` paints the
  zone as a translucent pane in both `ShellPage`s. windowd draws nothing. Proof: proto
  roundtrip + reject, shell host test (every zone paints exactly one pane, "" none, no handler).

### Board cycles

| Cycle | Image | Result |
|---|---|---|
| 1 (2026-10-07) | dev-828f47ef | boots, desktop + pointer + keyboard; left/right edge tiles work; **corners never tile** (no marker, no deny); the app-chip menu existed only in settings/stash; window drags present at 8–10/s (`gpud: present us avg=263138…414322 max≈1.2 s`, dc CPU path) — F6 below; `KSELFTEST: ipc call budget FAIL (85–98 µs/64)` again |
| 2 (2026-10-07) | dev-82f3a12d | `windowd: wm tile (zone=top-right id=app0)` from a corner drag, `wm return` + `SELFTEST: ui v7 tile ok`, `wm tile (zone=fill)`; ack `board-visual: tile`; the user's verdict "noch nicht rund, aber soweit ok" — the rest during the GPU lane (F6/F7). `gpud: present us avg=121371 max=287537` (dc CPU path). Board smoke: 45 of 46 rungs — `inputd: live keyboard route on` absent because no key was pressed in this cycle (user-driven rung), every other rung present |

### Open findings (recorded, not built)

- **F7 — drag feel and preview polish** (board cycle 2, user: "noch nicht rund"): the drag runs at the
  dc present rate (F6), so the preview pane and the window lag the pointer; tuning the preview's
  look (margin-aware frames, fade) and the corner band waits for the GPU lane's present path,
  where the feel can be judged at a real frame rate. Decided with the user on 2026-10-07.

- **F6 — the board's present cost during window drags** (measured, cycle 1): gpud's dc (CPU)
  present path costs 263–414 ms per present (max ≈ 1.2 s); windowd presents 8–10×/s only while
  a title bar is DRAGGED (the window follows the pointer; the backdrop blur is recomputed per
  move), so a drag runs at 2–3 fps. Idle pointer moves present nothing (`apphost: submitted` 15
  in the whole session, `WINDOWD: desktop input routed` 5; the cursor rides the display
  controller's layer, xhcid `dropped=0`). There is no Block-2 baseline line for the present
  cost (the stats line is newer than Block 2), so a regression cannot be shown or excluded
  from logs; the cost is the dc CPU path's and belongs to the GPU lane (first hypothesis to
  measure there: a full-frame convert per present instead of the dirty rect).

- A tile makes the app re-create its surface at the new size (destroy → create); windowd
  parks focus on the desktop for that window and a chord in that gap is refused with
  `no-focus` (sub-second; the injector waits for `windowd: focus id=app…` after a tile). A
  resize that keeps the surface would remove the gap — the WM resize path's business.
- The root cause of the TASK-0074 board hang surfaced while proving this lane and is fixed
  here (ADR-0065 amendment: `anim_sync` after the layout generation closes).

## End-state rewrite 2026-10-06 — model "A": the desktop default tiling (binding; supersedes everything below)

**User decision 2026-10-06:** rebuild the current desktop OS's built-in tiling exactly — not the
third-party utility's zone flood. Pointer AND keyboard. The hover menu lives on the window's
zoom control (window-kit chrome), one row "Move & Resize", then "Fill & Arrange"; **Fullscreen
stays in the app-icon dropdown** (design handoff, `WinMenuItem mode.fullscreen`), never in the
hover menu. Settings get a "Fenster" group in Personalisierung (our "Desktop & Dock").

### Goal (end system)

- **Zones** (`windowd/src/zones.rs`, replaces `snap.rs` in the same package): `LeftHalf`,
  `RightHalf`, `TopLeft`, `TopRight`, `BottomLeft`, `BottomRight` (quarters), `Fill`
  (work-area maximize, NOT fullscreen), `Return` (the frame before the first tile).
  **Arrangements** tile several windows at once: `LeftRight` (focused + next), `TopBottom`,
  `Quarters` (up to four). `zone_frame(zone, mode_w, work_h, top, margin)` is a pure function;
  `ZoneMap` = per-window zone + the pre-tile frame for Return.
- **Pointer**: drag release at the left/right edge → half, in a corner → quarter, at the top
  edge → Fill (replaces today's top = fullscreen); dragging a tiled window off its zone →
  Return. `SNAP_EDGE_PX` stays 4; corners = both edges within the margin.
- **Keyboard chords** (inputd normalizes, windowd applies to the FOCUSED window, apps never
  see them): Super+Ctrl+←/→/↑/↓ = halves (↑ = Fill? no: ↑/↓ = top/bottom half are NOT zones;
  ↑ = Fill, ↓ = Return), Super+Ctrl+Shift+←/→ with ↑/↓ = quarters, Super+Ctrl+F = Fill,
  Super+Ctrl+R = Return. v1 fixed; a chord editor is out.
- **Window menu = the existing app-chip dropdown** (window-kit `WinTopBar` app icon +
  chevron; user decision 2026-10-06 — nothing new is built, the menu we have gets its real
  job): row 1 "Bewegen & Größe ändern" (left half, right half, 4 quarters), row 2 "Ausfüllen &
  Anordnen" (Fill, Left&Right, Top&Bottom, Quarters, Return) as TILE GLYPHS like the desktop
  OS's tiling menu (a frame with the filled part — drawn from DSL primitives, `WinTileGlyph`,
  no text, no new icon assets), then the existing "Vollbild" row, then "Auf anderes Gerät
  bewegen" (future, inert, dsoftbus). Tiles dispatch `WinAct("zone.<name>")` → app-host
  `window.control` → windowd `CONTROL_WIN_ZONE` (value = `sid << 4 | code`, 16 codes). No
  hover menu on the zoom control; the zoom control keeps its fullscreen toggle.
- **Settings → Personalisierung → Fenster**: `wm.tile.edges` (on/off), `wm.tile.margin`
  (`0|8|16` px), `wm.tile.chords` (on/off) — settingsd keys, windowd watches them like
  `ui.theme.mode`; the chord list shown read-only.
- **Feed**: zone state rides RFC-0086 flag bits `WINDOW_ZONE_SHIFT = 2`, 4 bits, wire layout
  unchanged; app-host `window_state_of` exposes it.
- **Markers registered**, ungated prints deleted; `SELFTEST: ui v7 tile ok`.

### Non-goals

Thirds / two-thirds (the utility's extra zones), a chord editor, a drag preview outline, zone
highlight overlays, top/bottom halves as single zones, kernel changes, windowd drawing anything.

### Decisions (replace the 2026-09-09 D1–D6)

- **D1 Explicit zones, no occupancy magic.** A tile is what the user chose (edge, chord, menu);
  the map only remembers zones + pre-tile frames (Return) and reflows them on a work-area
  change.
- **D2 One geometry verb.** `CONTROL_WIN_ZONE = 9` (`control.rs`, approval zone) carries every
  zone and arrangement; `CONTROL_WIN_MODE`'s fullscreen stays the app-icon menu's.
- **D3 Chords are inputd facts on the state push** (`input-live-protocol` `VisibleState`
  gains `wm_chord: u8`, append-only; RFC-0052/0053 amendment): inputd tracks Super (GUI) and
  the arrows, recognizes the fixed table, emits the zone code once per press; windowd applies
  it to `windows.focused()` through the same `apply_zone`. ADR "WM chords: normalized by
  inputd, applied by windowd, invisible to apps" (next free number).
- **D4 No new UI primitive.** The app-chip dropdown (a `.overlay()` the kit already owns) hosts
  the tile rows; `WinTileGlyph` draws each zone from Stacks (frame + filled region). Keyboard
  chords and the menu are the two explicit entry points; edges/corners the implicit one.
- **D5 Settings keys are the only configuration**; defaults edges=on, margin=0, chords=on.
- **D6 `snap.rs` deleted** in the package that lands `zones.rs`; `config/loc-baseline.txt`
  never re-lists it; the top-edge-fullscreen gesture is retired (Fill instead).

### Packages

- **P0** Wedge check of the fullscreen transition path (2026-07-29 finding; host test first,
  visible lane second) + RFC-0086 amendment (zone bits) + RFC-0052/0053 amendment (chord
  field) + ADR + `wm-snap.md` → `wm-tiling.md`. Blast: paper (+ the wedge fix if real).
- **P1** `zones.rs` (+ inline tests: frames for every zone and margin, odd widths, Return
  frames, arrangements) and `CONTROL_WIN_ZONE`; `wm.rs::apply_zone`, edge/corner release,
  `snap.rs` deleted; markers; `tests/tile_zones.rs`. Blast: windowd host tests, smp1/visible.
- **P2** settingsd keys `wm.tile.*` + windowd watch (edges/margin/chords) + reflow on
  work-area change. Blast: settings lanes.
- **P3** inputd chords (Super/arrow tracking, fixed table) → state push → windowd focused
  window; `test_reject_*` (chords off, no focused window, non-resizable). Blast: input lanes.
- **P4** window-kit: tile rows + `WinTileGlyph` in the app-chip dropdown, "Vollbild" kept,
  "Auf anderes Gerät bewegen" inert; `WinAct("zone.*")` mapping; Settings "Fenster" group;
  feed bits + app-host decode; goldens (menu light/dark). Blast: settings/stash apps, shell.
- **P5** Proof: visible lane injector step (open two apps, drag one to an edge, chord the
  other), `SELFTEST: ui v7 tile ok`, test-all, board cycle with ack `board-visual: tile`.

### Definition of Done

Host: every zone/arrangement frame deterministic (incl. margins, odd widths); Return restores
the pre-tile frame; reflow on work-area change; deny table (`test_reject_not_resizable`,
`test_reject_chords_off`, `test_reject_no_focus`); menu goldens (light/dark).
QEMU (registered in `ui.toml`, `qemu-test.sh`, `markers.txt` group `wm-tile` + windowd
contract): `windowd: wm tile (zone=left-half id=…)`, `windowd: wm tile (zone=fill id=…)`,
`windowd: wm return (id=…)`, `windowd: wm chord (zone=… id=…)`, `windowd: wm tile deny
(reason=…)`, `SELFTEST: ui v7 tile ok`. Board: ack `tile`. Docs: `wm-tiling.md`,
`os-markers.md`, settings docs.

### Touched paths

windowd `src/{zones.rs (new), snap.rs (deleted), compositor/runtime/{wm.rs, input.rs,
windows_feed.rs, presentation.rs}, markers.rs}` + `tests/tile_zones.rs`; `nexus-display-proto`
`control.rs` + `surface_windows.rs` (approval); `userspace/input-live-protocol`; inputd
`service.rs`/`os_lite.rs`; settingsd `registry.rs`; app-host `effect_ime.rs` (zone verbs),
`effect_windows.rs`; window-kit `WinTopBar.nx` + `WinTileGlyph.nx`; settings
`SecPersonalization.nx` + i18n; RFC-0086/0052/0053 amendments (approval); ADR.

## End-state rewrite 2026-09-09 (superseded by the 2026-10-06 rewrite above; kept for history)

**Ground truth 2026-09-09 (verified in code):** halves + fullscreen pointer snap exist
(`source/services/windowd/src/snap.rs`, 109 LOC, `SnapTarget::{LeftHalf,RightHalf,Fullscreen}`,
`SNAP_EDGE_PX = 4`, driven by `apply_release_snap` in `compositor/runtime/wm.rs:365-398`,
TASK-0070 Done); window enumeration ships as the feed `OP_SURFACE_WINDOWS = 27` /
`OP_SURFACE_TASKBAR = 28` (`nexus-display-proto/src/surface_windows.rs`, `WindowEntry{owner_sid,
flags}`, 9 B/entry, RFC-0086) — no snap state in it. Missing: thirds + min-size rules, a zone
occupancy map, reflow on display-mode/work-area change, policy deny, registered markers
(`windowd: snap edge=…` is an ungated `debug_println`), and `docs/dev/ui/patterns/wm-snap.md`
is a 10-line stub that claims keyboard snapping (rejected design; pointer-only is accepted).
**Verify first:** the 2026-07-29 finding "chrome drag to the top edge + release →
`toggle_fullscreen` → window invisible, every later window invisible" (transition/override
wedge) has no fix in the git log since — reproduce on the visible lane before P1; if it still
wedges, fixing it is P1's first commit (it is the same code path).

### Goal (end system)

windowd's WM owns snap geometry, occupancy and reflow for halves and thirds, pointer-driven
only; snap state is part of the one window feed; denial is fail-closed with a named reason;
every marker is registered. Zone-picker UI (if ever) is a shell/widget concern, never windowd.

### Non-goals

Keyboard snap shortcuts (rejected design); explicit `snap()/unsnap()/list()` verbs (the feed IS
`list()`); zone highlight/picker UI; dynamic tiling layouts; kernel changes.

### Invariants

- Zone rects are a pure function of `(mode_w, work_area_h, top)`; ≤ `MAX_APP_WINDOWS`
  occupancy entries; ONE geometry-apply path (`apply_window_frame`).
- The policy verdict comes from the single presentation resolver
  (`surface_presentation.rs`), no policyd round trip in the input path.
- Deterministic transitions; no `unwrap`/`expect`; no blanket allows.

### Decisions

- **D1 Thirds are occupancy-driven, not a new gesture.** Edge release → the half if free; if
  that half is occupied the map re-partitions to thirds (previous occupant moves inward to
  `CenterThird`, the newcomer takes the outer third, the opposite half becomes its outer
  third); dragging a snapped window off its zone unsnaps it and the map collapses back to
  halves; a fourth window with all three zones taken → `ZonesFull` deny; `mode_w/3 < min
  width` → `MinSize` deny (halves kept).
- **D2 `snap.rs` REPLACED by `zones.rs`** (`Zone {LeftHalf, RightHalf, LeftThird, CenterThird,
  RightThird, Fullscreen}`, `ZoneMap` = occupancy + layout mode + prior frames for restore,
  `zone_frames(mode_w, work_h, top)`); `snap.rs` deleted in the same package. Gate: `wm.rs`
  compiles only against `zones::ZoneMap`; `config/loc-baseline.txt` never re-lists `snap.rs`.
- **D3 Snap state rides the existing feed** (RFC-0086 append-only flag bits):
  `WINDOW_ZONE_SHIFT = 2`, `WINDOW_ZONE_MASK = 0b111 << 2`, `ZONE_NONE..ZONE_FULLSCREEN = 0..6`;
  wire layout unchanged (9 B/entry). RFC-0086 amended (approval zone). app-host
  `effect_windows.rs::window_state_of` exposes `zone` to the shell.
- **D4 Reflow trigger = work-area change** (shell-mode tablet/desktop toggle, and the display
  mode TASK-0324 P6 makes authoritative): `wm.rs::reflow_snapped()` re-applies `ZoneMap` frames
  through `apply_window_frame`.
- **D5 Deny = presentation verdict.** `resizable = false` or role ≠ `Window` →
  `SnapDeny::NotResizable`; reasons `{NotResizable, ZonesFull, MinSize}` are a stable enum and
  marker token.
- **D6 Ungated prints DELETED** (`wm.rs:376-393`) and replaced by registered literals in
  `windowd/src/markers.rs`.

### Packages

- **P0** Wedge reproduction on the visible lane + RFC-0086 amendment (zone bits) +
  `wm-snap.md` rewritten (pointer-only, occupancy rule). Blast: paper (+ the wedge fix if
  reproduced: windowd transitions; lanes visible, smp1).
- **P1** `zones.rs` + inline tests (thirds rects incl. odd widths, occupancy transitions,
  restore). Blast: windowd host tests.
- **P2** `wm.rs` integration (release → `ZoneMap`, reflow, deny) + `source/services/windowd/
  tests/snap_zones.rs`. Blast: `ui_windowd_host`, lanes smp1 + visible.
- **P3** Feed bits + app-host decode + markers registered + selftest probe. Blast: RFC-0086
  consumers (desktop-shell taskbar), `check-chain-markers`.

### Definition of Done

Host: thirds/halves rects deterministic; two- then three-window occupancy → thirds; unsnap
restores the prior frame; work-area change reflows; deny table (`test_reject_not_resizable`,
`test_reject_zones_full`, `test_reject_min_size`). QEMU (registered in `proof-manifest/markers/
ui.toml`, `scripts/qemu-test.sh` full/visible lists, `tools/nx/chains/markers.txt` group
`wm-snap` + `tools/nx/src/chain/contract/windowd.rs`): `windowd: wm split on`,
`windowd: wm snap (zone=left-half id=…)`, `windowd: wm snap (zone=center-third id=…)`,
`windowd: wm unsnap (id=…)`, `windowd: wm snap deny (reason=zones-full)`,
`SELFTEST: ui v7 snap ok`. Docs: `wm-snap.md`, `docs/testing/os-markers.md`.

### Touched paths

`source/services/windowd/src/{zones.rs (new),snap.rs (deleted),compositor/runtime/wm.rs,
surface_presentation.rs,markers.rs,compositor/runtime/windows_feed.rs}`,
`source/services/windowd/tests/snap_zones.rs`, `source/libs/nexus-display-proto/src/
surface_windows.rs` (approval zone), `source/services/app-host/src/effect_windows.rs`,
`docs/rfcs/RFC-0086-*.md` (approval zone), markers triple, `source/apps/selftest-client/src/
os_lite/`.

### Dependencies

TASK-0324 P4a (windowd on the declarative arm) and P6 (display mode from the kernel = the
reflow source of truth).

## Rebase (2026-08-14) — residual-only — historical, superseded by the end-state rewrite above

### Shipped elsewhere — do NOT re-implement

TASK-0070 (Done) shipped the snap baseline this draft assumed was missing (its
rewrite note, lines 21-23, records that 0066's zones never landed and were
rebuilt pointer-driven):

- `source/services/windowd/src/snap.rs` — `LeftHalf`/`RightHalf`/`Fullscreen`
  edge snapping, `SNAP_EDGE_PX = 4`, **pointer-only by design** (global
  keyboard snap shortcuts were explicitly rejected); geometry unit tests live
  inline in its `#[cfg(test)] mod tests`.
- `source/services/windowd/src/compositor/runtime/wm.rs` (476 LOC) — WM runtime.
- Z-order SSOT `source/services/windowd/src/window_scene.rs`
  (`WindowId::App(u8)`, `MAX_APP_WINDOWS = 4`).
- `dock.rs` no longer exists (deleted by RFC-0086); minimize goes to the shell
  taskbar via `OP_SURFACE_WINDOWS` (27) / `OP_SURFACE_TASKBAR` (28)
  (`source/libs/nexus-display-proto/src/surface_windows.rs`).

### Honest residual scope (Size S)

1. **Thirds** (left/center/right) on top of the shipped halves, with min-size
   compliance and fallback rules.
2. **Zone-occupancy map** (zone → window bookkeeping).
3. **Reflow on display resize** for snapped windows.
4. **`list()` IDL** (enumerate windows + snap state). Explicit
   `snap(win, zone)`/`unsnap(win)` verbs are NOT residual — pointer-driven
   snapping is the accepted design.
5. **Policy deny path** (fail-closed snap denial + reason).

### Boundary rule (docs/dev/ui/windowd-cleanup-map.md:4-9)

windowd = Single Present Authority (compositor SERVICE). WM **geometry**
(zone rects, occupancy, reflow) legitimately lives in windowd; any tiling
**UI** (zone highlights, pickers, drag visuals) would not — that belongs to
widgets / the DSL shell app (`userspace/apps/desktop-shell`). Check the
cleanup map before touching any windowd file; never build into a MOVE/DELETE
file.

### Corrected proof home

`tests/ui_v7a_host/` never existed. Follow snap.rs's existing organization:
pure geometry (thirds rects, occupancy) in inline `mod tests`;
integration-shaped cases (reflow, policy deny) in
`source/services/windowd/tests/` next to `damage_pipeline.rs`/`headless.rs`.

## Context — historical, superseded by the end-state rewrite above

With UI v6 we have a basic WM. UI v7a adds productive “multi-window” behavior:

- snap zones (halves/thirds),
- simple tiling map (zone → window),
- reflow on display resize,
- and a policy hook to restrict multi-window per app.

DnD/clipboard/screencap/share are explicitly out of scope here (v7b/v7c).

## Goal — historical, superseded by the end-state rewrite above

Deliver:

1. Snap zones in `windowd` WM:
   - left/right/top/bottom halves; left/center/right thirds
   - min-size compliance and fallback rules
2. WM IDL extensions:
   - `snap(win, zone)`, `unsnap(win)`, `list()`
3. Simple tiling policy:
   - maintain zone occupancy
   - reflow snapped windows on display resize
4. Markers + host tests + OS/QEMU markers (gated).

## Non-Goals

- Kernel changes.
- Full tiling WM and dynamic layouts.
- Drag-and-drop, clipboard, screenshot/share (separate tasks).

## Constraints / invariants (hard requirements)

- Deterministic zone rect computation for a given display bounds.
- Bounded WM state (cap max windows).
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- Policy enforcement is fail-closed (deny snap if not allowed).

## Stop conditions (Definition of Done)

### Proof (Host) — required

Inline `snap.rs` `mod tests` + `source/services/windowd/tests/` (corrected
2026-08-14; `tests/ui_v7a_host/` never existed):

- thirds zone rects deterministic for given display bounds (halves already covered)
- occupancy map: snap two windows → zones tracked; unsnap restores previous bounds
- resize display → snapped windows reflow deterministically
- policy deny case (multi-window disabled or min size too large) returns deny + reason

### Proof (OS/QEMU) — gated

UART markers:

- `windowd: wm split on`
- `windowd: wm snap (win=..., zone=...)`
- `windowd: wm unsnap (win=...)`
- `SELFTEST: ui v7 snap ok`

## Touched paths (allowlist) — corrected 2026-08-14

- `source/services/windowd/src/snap.rs` (thirds + occupancy) +
  `src/compositor/runtime/wm.rs` (reflow) — check the cleanup map first
- wire ops for `list()` (windowd has no `idl/*.capnp`; ops live in the wire
  libs — `source/libs/**` is an approval zone)
- `policies/` + `schemas/policy/` (wm constraints, if not already present)
- `source/services/windowd/tests/` (integration proofs)
- `source/apps/selftest-client/`
- `docs/dev/ui/patterns/wm-snap.md`

## Plan (small PRs)

1. zone definitions + rect computation + markers
2. IDL changes and WM snap/unsnap implementation
3. policy constraints integration
4. host tests + OS selftest markers + docs + postflight
