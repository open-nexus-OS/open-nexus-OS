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
+ desktop toggle + both launchers are boot-proven (2026-07-13); compact/
regular boot proof waits on virtio-gpu display-info plumbing (the guest mode
is fixed 1280×800 today — QEMU `xres=` hints are ignored).

Known deltas vs the handoff (deliberate v1): island bars are static (media
service pending), no drop-down panels yet (Control Center is a page, panels
= v2), paged launcher grid + page dots land when one page overflows, desktop
icon field wraps in rows (column-wrap pending engine support).
