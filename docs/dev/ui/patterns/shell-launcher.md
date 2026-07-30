<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Shell / Launcher layout contract (design_handoff_launcher)

SSOT for how the shell home + launcher select their layout. The visual ground
truth is `docs/dev/design_handoff_launcher/` (tokens = the literal numeric
contract; `OpenNexusLauncher.html` = the living reference).

## The one driving concept: mode ⟂ width

Two INDEPENDENT axes decide the whole layout:

1. **Desktop mode** — an explicit user toggle (Control Center → Tablet/
   Desktop), NOT width-derived. Wire: app-host `svc.settings.set
   ("ui.shell.mode")` → `OP_SURFACE_CONTROL(CONTROL_SHELL_PROFILE)` → windowd
   (presentation authority) applies + persists + pushes `OP_SURFACE_PROFILE`
   → the shell app-host REMOUNTS; `device.profile == "desktop"` selects the
   `ui/platform/desktop/` page overrides (taskbar shell, windowed launcher).
2. **Width** — only when NOT desktop, picks between the touch layouts via
   `device.sizeClass` (mobile-first): **compact** = phone (`w < 640`),
   **regular** = tablet portrait (`< 1024`), **wide** = landscape (`≥ 1024`).
   The app-host derives the class from the REAL surface width
   (`size_class_for`, app-host `main.rs`) and re-emits the scene when a
   resize crosses a breakpoint (`reemit_for_size_class` — a re-emit, never a
   remount: stores survive). Pages branch with `if device.sizeClass == …`.

## The four layouts (`userspace/apps/desktop-shell/ui/`)

- **Desktop** (`platform/desktop/pages/ShellPage.nx`): 56px full-width
  taskbar — bare launcher glyph left · app icons xs DIRECTLY on the bar +
  divider + mini player centre · bare nav right; top bar = time pill + bell +
  FOUR separate status pills; top-left icon field. Launcher = WINDOWED
  720×520 panel (`platform/desktop/pages/LauncherPage.nx`) with "All apps" +
  search + grid + identity footer.
- **Wide touch** (`pages/ShellPage.nx`, wide arm): three floating glass
  elements — round launcher button left · dock pill (44px tiles + divider +
  mini player) centre · nav pill right.
- **Regular touch**: launcher button + dock pill centred side by side, bare
  nav row below.
- **Compact touch**: full-width dock row (apps spread evenly), bare nav row
  below (back · launcher glyph · home · overview); merged time+bell pill.

Launcher (touch) = fullscreen panel-glass overlay: greeting + date, search,
centred grid (`pages/LauncherPage.nx`); phone keeps the single-column list
override.

## App tiles

Tiles are UNIFORM glass (`.material(card)`) — apps differ only by their icon
glyph, never tile color. The glyph comes from the app manifest
(`icon = "<symbol>"`, a theme icon-set name from `resources/themes
[icons.symbols]`), flows bundlemgrd `OP_LIST_APPS`
(`id,label,icon` length-prefixed triples) → `svc.bundlemgr.enumerate`
records → `AppTile { label: app.label, icon: app.icon }`. Registry-driven —
never a hardcoded app list.

## Verification

Host: `layout_viewport.rs` probes pin all four families (wide dock bottom,
compact/regular mount+dock, desktop taskbar heights). On-device: tablet wide
+ desktop toggle + both launchers are boot-proven (2026-07-13); the compact
family is boot-proven at 600×800 (2026-07-14) via the display-info chain
below.

## Device display mode (how the shell learns its real resolution)

The guest follows the device's reported mode instead of assuming 1280×800:

1. **gpud probe** sends virtio-gpu `GET_DISPLAY_INFO`; `pmodes[0]` carries
   the device's preferred mode (QEMU `xres=`/`yres=`). Clamped to the fixed
   1280×800 resource budget → `backend.display_w/h`; scanout, present rects
   and damage clamps follow it.
2. **windowd** asks gpud `OP_GET_DISPLAY_MODE` (one bounded round-trip on
   the init-wired persistent windowd↔gpud slot pair, BEFORE the runtime is
   built) and constructs `DisplayServerRuntime::new_with_mode(w, h)`.
3. **Resource ⟂ visible**: the shared VMO layout (stride 5120, plane
   offsets, atlas) stays at the fixed 1280×800 maximum; only the VISIBLE
   sub-rect (scanout, viewport, dock/window sizes, damage clamps) follows
   the mode. `VisibleBootstrapMode::for_visible` welds the stride to the
   fixed pitch, so every VMO-addressing site is untouched.
4. The desktop surface then reports the real width to the DSL shell →
   `device.sizeClass` picks the dock family (mode ⟂ width, above).
5. **inputd** re-bases its pointer DISPLAY space on the same mode: it asks
   windowd `OP_GET_VISIBLE_MODE` (input-live-protocol op 5) from its server
   loop's idle arm (rate-limited retry until windowd serves), then
   `InputdService::set_display_space(w, h)` rebuilds the transform — so
   absolute (tablet/touch) coordinates land in the exact space windowd
   hit-tests in. Marker: `inputd: display mode WxH` (fold-immune).
6. The **wallpaper** maps aspect-correct via `build_cover_luts` (uniform
   cover scale + center-crop; at native aspect byte-identical to the plain
   scale LUT) — no stretching on narrow modes.

Wire traps this chain survived (documented for the next data-carrying op):
the virtio response DESCRIPTOR must advertise the full reply length (a
header-sized window silently truncates `pmodes` to zeros); an early
`new_for("gpud")` mint in windowd crossed slots — the fresh recv slot
received inputd's `OP_UPDATE_VISIBLE_STATE` pushes ("IN…" frames) meant for
windowd's SERVER, and gpud never saw the request — use the init-wired
persistent slot pair for early queries; boot markers on this path must be
fold-immune (`debug_write`) or the outcome is invisible in armed boots.

## Drop-down panels (v3, design_handoff_panels)

Six panels — **Control Center · Mitteilungen · Kalender · WLAN · Ton ·
Batterie** — opened exclusively by the top bar's pills. Both sides live in one
component each: `components/topbar/TopBar.nx` is the 36px bar (it used to be
written out inline in BOTH `ShellPage`s, which is how a trigger matrix drifts),
and `components/panels/PanelHost.nx` is the `.overlay()` layer plus the six-way
selection (it used to be a four-level nested `if/else`, also twice).

**Which panels exist is two independent axes**, exactly as the handoff states:
`device.profile == desktop` is the explicit Ansichtsmodus flag, and
`device.sizeClass` only matters when it is false. Desktop gets four separate
right-hand pills (WLAN · Ton · Batterie · Control Center); touch modes get ONE
combined status pill, because **there are no WLAN/Ton/Batterie panels there** —
their contents fold into the Control Center. `PanelHost` guards those three ids
on `device.profile`, so a panel id can never render something no pill can
reach.

Geometry (handoff §2): bar 36, pills 28 with `.hitSlop(2)` so the BAR is the
touch target; panels anchored `top: 44` via a leading `Stack{}.height(8)` and
`left/right: 12` via `.paddingX(3)` around a `Spacer` — `.overlay()` has no
anchor API, so anchoring is flex. Widths 328 · 330 · 288 · 300/300/300, radii
30 · 26 · 24 · 30. Entrance: Control Center `slideUp`, the other five
`slideDown` (a drop-down falls out of the bar; it does not rise into it).

One `PanelStore.panel: Str` selects the open panel and `SetPanel(p)` TOGGLES,
so only one is ever open; the layer's own handler is the outside-click closer
and every panel body consumes with `PanelNoop`.

**What is real and what is drawn.** Two controls reach a service: the
Ansichtsmodus tile writes `ui.shell.mode` and the Erscheinungsbild button
writes `ui.theme.mode`, both through `ControlStore`. Both read their current
value from `device.*` (`device.profile` / `device.theme`) and never mirror it
in a store — a re-mount resets stores, so a mirror would be wrong exactly when
it mattered. Everything else (radios, devices, energy profile, networks,
notifications, the calendar's three authored months) is demo state in
`composables/{connectivity,sound,power,calendar,notifications}.store.nx`: it
changes visibly, calls no `svc.*`, and sets no marker.

Shared parts live in `components/panels/parts/` — `ToggleTile`, `RadioRow`,
`GlassToggle`, `Meter`, `SignalBars`, `CountBadge`, `AppearanceButton`,
`MuteButton`, `SectionTitle`, `SectionRow`, `GroupLabel`, `HeaderIconButton`.

**Glass over surface content (the glass-reset contract).** On the virgl
buildup backend every glass region already gets destination-so-far GPU blur
(gpud `blur_rt_backdrop` — the desktop's own base layer composites first in
the same present). What used to ghost through panels was SURFACE-side: the
CPU painter src-over'ed the translucent panel fill over the tiles baked in
the same band. Two painter rules fix it: (1) GLASS boxes RESET their rect to
the pure premultiplied tint (`fill_round_rect_row_replace`,
`nexus-scene-raster`) — content beneath glass belongs to the compositor's
backdrop blur, never to the surface pixels; (2) the GLYPH pass drops text
runs whose box lies under a LATER glass box (text paints after all fills and
would otherwise print over the reset). Panels ride the plain `panel`
material.

Known deltas vs the handoff (deliberate): island bars are static and the
island does not expand on hover (media service pending; a hover-driven size
change is not an expressible motion token), paged launcher grid + page dots
land when one page overflows, desktop icon field wraps in rows (column-wrap
pending engine support).

Panel-specific deltas, all platform limits rather than choices: **no exit
animation** (`.transition` is enter-only), **sliders render but do not drag**
(the `Change` trigger carries no payload, so a drag has no value to deliver —
mute still moves the volume fill, because that goes through a `Tap`), and
padding/gap sit on the 4px spacing grid where the handoff uses 9/10/11/13/15
(fixed sizes and radii ARE exact; `.rounded` takes raw px for that reason).
