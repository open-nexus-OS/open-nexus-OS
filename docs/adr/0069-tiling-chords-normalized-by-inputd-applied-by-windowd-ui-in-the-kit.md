# ADR-0069: Window tiling — geometry in windowd's WM, chords normalized by inputd, every UI element in the window kit

- Status: Accepted
- Date: 2026-10-06
- Links:
  - Tasks: `tasks/TASK-0066-ui-v7a-wm-split-snap.md`
  - RFCs: `docs/rfcs/RFC-0086` (window feed — zone bits), `docs/rfcs/RFC-0053` (live input
    contract — the `wm_chord` fact), `docs/rfcs/RFC-0067` (windowd = compositor service)
  - Related ADRs: `docs/adr/0068-*` (modal semantics: one windowd verb, UI in the kit)

## Context

The desktop default tiling model (halves, quarters, Fill, Return, arrangements; drag to
edges and corners; a window menu with tile glyphs; keyboard chords; three settings) needs
three things the repo has never placed: where tiling GEOMETRY lives, how a global KEYBOARD
CHORD reaches the window manager without apps ever seeing it, and where the UI (menu, glyphs,
settings) lives. The user's rule for this codebase is firm: windowd is a compositor and WM
service — it draws no UI; UI elements live in the one component library (`window-kit`).

## Decision

- **Geometry is windowd's.** `zones.rs` is the pure SSOT (zone frames, release rule, codes);
  `runtime/tiling.rs` applies frames through the ONE geometry path (`apply_window_frame`),
  remembers the pre-tile frame (Return), reflows on a work-area change. One verb,
  `CONTROL_WIN_ZONE` (own-window gated), carries every zone, Return and arrangement.
  `snap.rs` is deleted; the top edge FILLS the work area (no fullscreen gesture).
- **Chords are inputd facts.** inputd tracks Super and the arrows, recognizes the fixed
  table (Super+Ctrl + ←/→ halves, ↑/F Fill, ↓/R Return, +Shift ←/→ top quarters, +Alt ←/→
  bottom quarters) on the press, and sets `VisibleState.wm_chord` for exactly one state push;
  the chord's key is never a keyboard dispatch (imed and apps never see it). windowd applies
  the code to the focused app window (`ui.tile.chords` gates it). No new wire, no new route.
- **UI is the kit's.** `WinAppWindow` mounts the app-chip dropdown (`WinAppMenu`) for every
  window-kit app: the two glyph rows (`WinTileGlyph`, drawn from primitives, no text headers),
  Fullscreen, the inert "move to another device" entry, Minimize/Close — labels as props, no app
  copies the menu; settings live in Personalisierung → Fenster over settingsd keys
  `ui.tile.edges|margin|chords`. The feed carries the zone (RFC-0086 bits 2..=5) for the shell.
- **The drag preview is the shell's** (amended 2026-10-07 after board cycle 1). windowd knows the
  candidate zone of a drag (`zones::release_zone_at` per move) and pushes it to the DESKTOP
  surface as `OP_SURFACE_TILE_PREVIEW` (one push per change, 0 on release); app-host exposes it
  as the device axis `device.tilePreview`; the desktop shell's `TilePreview` overlay paints the
  pane. windowd still draws nothing, and apps never see a non-empty value.
- **Out of scope:** thirds, a chord editor, top/bottom halves as single tiles, cross-device moves
  (dsoftbus).

## Consequences

- **Positive**: one geometry path and one verb (menu, chord, edge all converge); apps cannot
  intercept or spoof chords; the kit's menu is the single window-menu structure (settings and
  the file manager dropped their duplicated panels); tiling is configurable through the same
  settings path as every other `ui.*` key.
- **Negative / accepted**: the chord table is fixed in v1; a chord with Super+Ctrl held is
  consumed even when the focused window cannot tile (a deny marker says why).
- **Churn**: `VisibleState` grew one byte (append-only, both ends in one crate); the legacy
  "split" window mode is the left-half tile.
