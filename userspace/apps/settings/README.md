# settings — DSL app

The system settings app, built to the design artifacts (settings window) on `WinAppWindow`
(RFC-0084): a sidebar of twelve sections, a content pane with a breadcrumb header, an overview
grid, the Erscheinungsbild sub-page and the keyboard-shortcuts window. It is the second consumer
that proved the window scaffold generalises past Stash.

## Routes

One page, one route: `/` -> `SettingsPage` (`ui/pages/Routes.nx`). Navigation is state, not
routes: `mode` (overview | browse) x `section` x `subPage`. The sidebar item ids ARE the section
values, so a new section is one line in `NavSidebar` and one arm in `SectionView`. Back walks
sub-page, section, overview. Phase 1 renders the section roots; the design's ~20 chevron
sub-pages show as value rows (a chevron that pushes nothing would promise a missing screen).
Erscheinungsbild and Info > Systemaktualisierung are real sub-pages because a service backs them;
Tastaturkurzbefehle opens a window (`ScOpen`) on the page root, like the picker sheets.

## What is real and what is optics


The design describes a whole settings tree. Most of it has no system behind
it yet, and the app says so by construction:

| Really writes | Key | Where |
|---|---|---|
| Hell / Dunkel | `ui.theme.mode` | Personalisierung › Erscheinungsbild |
| 9 accents | `ui.theme.accent` | Personalisierung › Erscheinungsbild |
| Systemsprache | `ui.locale` | Allgemeine Verwaltung |
| Land / Region | `region.country` | Allgemeine Verwaltung |
| Tastaturlayout | `input.keymap` | Allgemeine Verwaltung |
| Zeitzone | `time.zone` | Allgemeine Verwaltung |
| Stundenformat | `time.format` | Allgemeine Verwaltung |
| Adaptive Vorschläge / Vergessen | `ime.personalization` | Allgemeine Verwaltung |

Read from a system, not written: **Allgemeine Verwaltung › Tastaturkurzbefehle** opens a
window (app-modal, over the settings window) that lists the shortcut registry
(`svc.shortcuts.list`, RFC-0113 §7 — the same table inputd and the keymap match keys against;
`ShortcutsWindow`, `shortcuts.store.nx`). Its "Ändern" and "Standard wiederherstellen" are drawn
disabled: rebinding is a later phase.

Everything else is the design's demo data. The ~34 switches live in
`demo.store.nx` and reach no service — moving one out of that store is the
signal that it stopped being a mockup. Two things are drawn but deliberately
inert, and both are recorded in `docs/dev/ui/design-token-audit.md`:
**Modus › Automatisch** (windowd's `ThemeMode` is `dark|light`) and the
**App-Icon-Stil / Ordnerfarbe** pickers (no consumer).

## Stores and events

A `reduce` is declared once per event, covers every case and touches ONE store, so an `Event`
type binds to one `Store` and a field lives where its writers live (`menu` in the kit's
`WindowStore`, `picker` in `RegionStore`). A change across domains goes through an `@effect` that
dispatches the other domain's event (`Goto` closes the chrome menu that way, and the kit's
sidebar pick reaches `NavStore` as `NavPick`).

| Store (file) | State | Events |
|---|---|---|
| `NavStore` (`navigation.store.nx`) | `mode`, `section`, `subPage` | `NavPick`, `Goto`, `OpenSub`, `Back`, `GoOverview` |
| `AppearanceStore` (`appearance.store.nx`) | mode, accent, icon style, folder colour, `ui.material`, apply error | `SetTheme`, `SetModeAuto`, `SetAccent`, `SetIconStyle`, `SetFolder`, `SetMaterial`, `ApLoad`, `Ap*` results |
| `RegionStore` (`region.store.nx`) | locale, country, keymap, zone, hour format, IME personalisation, open `picker` | `OpenPicker`, `ClosePicker`, `SetLocale`, `SetCountry`, `SetKeymap`, `SetZone`, `SetHourFmt`, `SetPersonalization`, `Forget`, results |
| `TilingStore` (`tiling.store.nx`) | the three tiling keys | `SetTileEdges`, `SetTileMargin`, `SetTileChords` |
| `ShortcutsStore` (`shortcuts.store.nx`) | the shortcuts window and its registry list | `ScOpen`, `ScClose`, `ScPick`, `ScListed`, `ScFailed` |
| `UpdatesStore` (`updates.store.nx`) | slot, trial, feed, staged build | `UpdRefresh`, `UpdStage`, `UpdSwitch`, `UpdRollback` and their `Upd*` results |
| `ViewStore` (`view.store.nx`) | `showSubtitles`, `sidebarDesc`, `advanced` — the ⋮ menu's "Ansicht" options (`@persist`) | `ToggleSubtitles`, `ToggleSidebarDesc`, `ToggleAdvanced` (each closes the open menu through an effect) |
| `DemoStore` (`demo.store.nx`) | the ~34 demo switches and the three demo slider positions (`rngVolume`, `rngInput`, `rngBrightness`) | toggles and slider taps (no service) |
| `WindowStore` (window-kit's composable, compiled in with the chrome) | the kit's chrome state: menus, panes | the kit's `Win*` events (RFC-0084); a sidebar pick reaches `NavStore` as `NavPick` through an effect |

## Services

`svc.settings.get/set` (theme, accent, `ui.material`, locale, region, keymap, zone, hour format,
tiling keys, IME personalisation, window controls), `svc.shortcuts.list` (the registry,
RFC-0113 section 7), `svc.updates.status/check/feed/stage/switch/rollback` (the update engine's
slot truth). Everything else is demo data.

## Window

`Window { style: plain, mode: freeform, resizable: true }` — freeform is a visual contract, not
a preference (see the trap below); the page owns its chrome through `WinTopBar`.

## Structure

```
manifest.toml              bundle_type = "settings"; dependencies = ["window-kit"]
i18n/{de,en}.json          complete; en is baked as the default. ja/ko/zh: chrome, shortcuts, the
                           view menu, accents and the new rows (the rest falls back to en)
ui/pages/
  Routes.nx                "/" -> SettingsPage
  SettingsPage.nx          the composition: Window intent, WinTopBar, WinAppWindow slots, layers
ui/components/
  chrome/                  NavSidebar (+ SideItem) · CrumbTrail · CrumbBar · PropsPane ·
                           MoreMenu (+ MenuCheck, JumpMenu: the ⋮ menu and its left-hand submenu)
  pickers/                 PickerLayers (one open sheet) · PickerSheet · PickerOption ·
                           Language/Region/Layout/Zone/HourFormat/Margin pickers
  rows/                    SetGroup · SetRow · SetSliderRow · AccountRow · SetValue · SetHairline ·
                           ToggleRows (the on/off settings as toggle rows) · RegionSelect
  overview/                OverviewGrid · OverviewCard
  appearance/              AppearanceView = ApModeBlock · ApAccentBlock · ApIconsBlock ·
                           ApFolderBlock (each under an ApHeader with the current choice), with
                           ApModeTile · ApSwatch · ApIconTile · ApFolderTile
  sections/                SectionView + Sec* (one per section) · PerLook (the Erscheinungsbild
                           row's live subtitle) · UpdatesView
  shortcuts/               ShortcutsWindow · ScCategory · ScRow · ScKeys · ScName
ui/composables/            the stores above
```

## Tests

`tests/dsl_apps_conformance/tests/`: `settings.rs` (navigation, appearance writes, all nine
accents, region and updates truth, the view options, the state-dependent subtitles, the tiling
toggle and select rows), `settings_tiling_menu.rs`, plus the shared `app_structure`,
`shell_panels`, `window_menu_global`, `long_session`, `budget_probe`, `arena_invariant` and
`frame_arena_budget` (settings layout budget). `nexus-dsl-runtime`'s `layout_viewport.rs` mounts
the app. Run `cargo test -p dsl_apps_conformance --test settings`.

## Known limits

- The chevron sub-pages (S24, S30), the search mode and toolbar Suche (S9, S37) and the
  "In <Sektion> suchen" action of the design are not built: no service or screen backs them. The
  ⋮ menu draws search, help and reset DISABLED, with the design's shortcut hints (no key binding
  behind them yet); the toolbar's Suche is not drawn at all because the kit's `WinToolCell` has no
  disabled form.
- Option lists (`PickerSheet`) are a centred sheet, not an anchored dropdown: the kit's `Select`
  is the trigger only and an overlay has no anchor to the trigger. A `Select` takes no size
  modifier (its builder owns them), so the design's 210 px minimum width is the width of the
  wrapper the row puts around it.
- The sliders (Lautstärke, Eingangspegel, Helligkeit) are demo state: they move on taps and reach
  no service. Nobody drags them (the `Slider` has no drag yet).
- Appearance: the mode tiles' backdrop is a vertical gradient (the design's is a 135° diagonal),
  the selection rings are 2 px (the design's 1.5), and the icon-style tiles draw glyph chips
  instead of three real app icons.
- The sidebar pane costs ~160 kB of layout when a `WinPane` has both header and footer
  around a scrolling body (window-kit); the recorded settings budget carries it.

## Traps worth knowing before editing


- **`just dsl lint <file>` lies.** It parses one file with no project merge
  and no manifest dependencies, so it sees neither `WinAppWindow` nor the
  stores. `just dsl fmt` is worse: it DELETES comments. The only honest gate
  is `just check` (bundlemgrd's build script compiles every app fail-closed);
  `cargo test -p dsl_apps_conformance` drives the interactions.
- **Icon symbol names are not compiler-checked.** An unmapped name paints a
  grey box. `tests/dsl_apps_conformance/tests/icon_symbols.rs` scans the
  source instead — a new component prop that forwards a symbol must be added
  to its `SYMBOL_PROPS` list.
- **`Window { mode: freeform }` is a visual contract.** `fullscreen` makes
  app-host paint an opaque page base and makes windowd skip the backdrop-blur
  band, so the glass panes would frost a slab instead of the wallpaper.
- **Both panes scroll as scroll layers** (RFC-0109): the sidebar's section
  list and the content pane are each a `.scroll(vertical)` container — app-host
  fills their bands and runs their physics, windowd moves them at the offset the
  app commits; the statics around them are the base. An `.overlay()` (menu,
  picker) over a pane covers its layer: its content then paints in the base,
  under the overlay, until the overlay closes.
