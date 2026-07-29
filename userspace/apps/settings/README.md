# settings — DSL app

The system settings app, rebuilt against its design handoff
(`docs/design_handoff_os_settings_window/` — `README.md` for the component
contract, `LAYOUT.md` for the metrics, `source/OsSettingsWindow.dc.html` for
the tree). Built on `WinAppWindow` (RFC-0084): this app is the second
consumer that proves the window scaffold generalises past stash.

## What is real and what is optics

The handoff describes a whole settings tree. Most of it has no system behind
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

Everything else is the handoff's demo data. The ~34 switches live in
`demo.store.nx` and reach no service — moving one out of that store is the
signal that it stopped being a mockup. Two things are drawn but deliberately
inert, and both are recorded in `docs/dev/ui/design-token-audit.md`:
**Modus › Automatisch** (windowd's `ThemeMode` is `dark|light`) and the
**App-Icon-Stil / Ordnerfarbe** pickers (no consumer).

## Layout

```
manifest.toml              bundle_type = "settings" (admits nexus.permission.SETTINGS);
                           dependencies = ["window-kit"]
i18n/{en,de}.json          complete (251 keys); en is BAKED as the default
i18n/{ja,ko,zh}.json       navigation + chrome only; row detail falls back to en
ui/pages/
  Routes.nx                "/" -> SettingsPage
  SettingsPage.nx          Window intent · WinTopBar · WinAppWindow slots ·
                           breadcrumb chain · the .overlay() layers
ui/components/
  chrome/                  NavSidebar · CrumbBar · PropsPane · MoreMenu ·
                           MenuRow · PickerSheet · PickerOption
  rows/                    SetGroup · SetRow · SetValue · SetHairline ·
                           RegionSelect (the five Select triggers)
  overview/                OverviewGrid · OverviewCard
  appearance/              AppearanceView · ApModeTile · ApSwatch ·
                           ApIconTile · ApFolderTile
  sections/                SectionView + Sec* (one per section)
ui/composables/            window · navigation · appearance · region · demo
```

## Stores — one per domain, and why the split falls where it does

The lowering enforces it: a `reduce` block may be declared **once** per event,
must cover **every** case of that event, and all of its arms together may
touch **one** store. So an `Event` type binds to exactly one `Store`, and a
field lives where its writers live — that is why `menu` sits in `WindowStore`
and `picker` in `RegionStore`, not both in navigation.

A change that has to cross domains goes through an `@effect` that dispatches
the other domain's event. `NavStore` closes the chrome menu that way. Effects
cannot branch (`if` is not an effect statement form), so each such bridge
fires unconditionally — which is why the toolbar renders back but not forward
or refresh: `WinNav` has exactly one meaning to forward.

## Navigation

`mode` (overview | browse) × `section` × `subPage`. The sidebar item ids ARE
the section values, so adding a section is one line in `NavSidebar` and one
arm in `SectionView`. Back walks sub-page → section → overview.

Phase 1 renders the section ROOTS. The handoff's ~20 chevron sub-pages are
Phase 2 and their rows show as value rows until then — a chevron that pushes
nothing would promise a screen that does not exist. `Personalisierung ›
Erscheinungsbild` is the exception: it is a real sub-page because the page
behind it is real.

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
- **`.overlay()` does not composite inside a banded scroll surface**, which is
  why the menu and picker layers are siblings of the window on the page root
  rather than children of the scrolling content pane.
