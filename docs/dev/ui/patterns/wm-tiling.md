# Window tiling (TASK-0066, ADR-0069)

The desktop default model, pointer AND keyboard, with every UI element in the window kit and
the geometry in windowd's window manager.

## Zones

| Zone | How |
|---|---|
| Left / right half | drag release at the left/right edge · menu tile · Super+Ctrl+←/→ |
| Four quarters | drag release in a corner (within 64 px of both edges — no edge contact needed) · menu tile · Super+Ctrl+Shift+←/→ (top), Super+Ctrl+Alt+←/→ (bottom) |
| Fill (the work area, chrome kept — not fullscreen) | drag release at the top edge · menu tile · Super+Ctrl+↑ or +F |
| Return (the frame before the first tile) | drag a tiled window off its tile · menu tile · Super+Ctrl+↓ or +R |
| Arrangements: Left&Right, Top&Bottom, Quarters | menu tiles — the window plus the next ones in z-order |

Frames are a pure function of the display mode, the work area (status bar top, taskbar
bottom) and the margin (`zones::zone_frame`); odd sizes give the extra pixel to the
right/bottom tile. `ui.tile.margin` (0 | 8 | 16 px) insets every tile; neighbours share one gap.

## Who owns what

- **windowd** (`zones.rs`, `runtime/tiling.rs`): geometry, the pre-tile frame, reflow on a
  work-area change, the ONE verb `CONTROL_WIN_ZONE`, the feed bits (RFC-0086 bits 2..=5), and
  the drag PREVIEW push (`OP_SURFACE_TILE_PREVIEW` to the desktop surface: the candidate zone,
  one push per change, 0 on release). Nothing is drawn.
- **desktop shell** (`overlays/TilePreview.nx`): paints the candidate zone from
  `device.tilePreview` as a translucent pane over the work area — a full-bleed overlay with no
  handler.
- **inputd**: Super+Ctrl chords are facts on the state push (`VisibleState.wm_chord`,
  one-shot); the chord's key never reaches imed or an app.
- **window-kit**: `WinAppWindow` mounts `WinAppMenu` for every window-kit app when
  `$state.menu == "app"` (the app chip opens it): two `WinTileGlyph` rows without text
  headers (move & resize, fill & arrange), Vollbild, the inert "Auf anderes Gerät bewegen",
  Minimieren, Schließen; every tile dispatches `WinAct("zone.<name>")` and the app's `WinAct`
  effect forwards it as `window.control`. An app's own menus mount no backdrop while the window
  menu is open (a later overlay would sit above it).
- **Settings → Personalisierung → Fenster**: `ui.tile.edges`, `ui.tile.margin`,
  `ui.tile.chords` (settingsd; windowd applies live).

## Markers

`windowd: wm tile (zone=<zone> id=<window>)`, `windowd: wm return (id=…)`, `windowd: wm
arrange (kind=… n=…)`, `windowd: wm chord (zone=… id=…)`, `windowd: wm tile deny
(reason=not-resizable|no-window|edges-off|chords-off|no-focus)`, `SELFTEST: ui v7 tile ok`
(a tile followed by a Return on one boot). Trace (not a contract): `windowd: wm release at
(x,y)` for a drag release that tiles nothing.

## Not in v1

Thirds, a chord editor, top/bottom halves as single tiles, moving a window to another device
(the menu entry is there, inert).
