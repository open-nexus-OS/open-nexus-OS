# Handoff — Einstellungen-Fenster (OS Settings Window)

Full settings app built on the **open nexus OS Design System**. Everything here is copied
verbatim from the design system — no forked or re-styled code.

Namespace at runtime: `window.AIVAOSDesignSystem_afec52`

```
source/OsSettingsWindow.dc.html   the template (markup + logic, single file)
source/ds-base.js                 loads styles.css + _ds_bundle.js; edit `base` when moving it
source/support.js                 DC runtime (do not edit)
reference/styles.css              entry point, @imports reference/tokens/*
reference/tokens/*.css            all design tokens (colors, glass, radii, motion, type)
reference/tokens.json             the same tokens resolved to plain values — for non-CSS targets
reference/components/*.txt        source + typed API of every component used
                                  (`.jsx.txt` / `.d.ts.txt` — strip `.txt` to compile)
reference/assets/icons/*.svg      the four app icons the template loads
LAYOUT.md                         pixel/behaviour spec for a native reimplementation (Rust etc.)
```

## Run it

`source/ds-base.js` resolves the design system from `base` (default `../..`). Keep `source/`
two levels below the design-system root, or point `base` at the bound `_ds/<folder>` tree.
Then open `OsSettingsWindow.dc.html` — no build step.

The template loads app icons via `../../assets/icons/*.svg` (design-system root). Outside this
project, repoint those to `reference/assets/icons/`.

## Components used

| Component | Where | Notes |
|---|---|---|
| `AppWindow` | window shell | slots: `leading`, `toolbar`, `trailing`, `sidebar`, `contentHeader`, `contentActions`, `children`. Sidebar toggle is added by the component. Responsive: desktop ≥820px, compact ≥560px, mobile below — the sidebar becomes an overlay with scrim. |
| `Window` / `WindowPane` / `WindowControls` / `WindowButton` | chrome (via AppWindow) | title chrome, inner pane card, traffic lights, toolbar buttons (`title`, `active`, `onClick`). |
| `Icon` | all glyphs | Lucide-style path strings, `size`, `strokeWidth`, `color`. All paths used live in `this.P` in the logic class. |
| `Sidebar` | left navigation | 12 sections, `items`, `value`, `onChange`, `header`, `footer`, `variant="plain"`. Two-line items (title + description) — the second line is toggleable. |
| `Breadcrumbs` | content header | functional: `Einstellungen › Sektion › Unterseite`, `onNavigate` jumps back. |
| `ListItem` | every settings row | `leading` (icon), `title`, `subtitle`, `trailing` (toggle / value / select), `showChevron`, `onClick`. |
| `GlassToggle` | boolean rows | controlled: `checked` + `onChange`. |
| `Select` | enum rows (Sprache, Region, Layout) | `options`, `value`, `onChange`, `size="sm"`. |
| `SearchBar` | search mode | `value`, `onChange`, `placeholder`. |
| `Chip` | search suggestions | `selected`, `onClick`. |
| `Menu` | three-dot menu | `items` with `header`, `divider`, `checked`, `shortcut`, `submenu`, `destructive`; `placement`, `width`, `trigger`. |
| `GlassButton` | "In <Sektion> suchen" | `variant="glass"`, `size="sm"`. |
| `Avatar` | sidebar footer | `initials`, `size`, `status="online"`. |

Local markup only: the overview card grid, the appearance page (mode previews, accent swatches,
icon-style tiles, folder colours) and the section/group scaffolding — all composed from tokens.

## Colors — tokens only, no literals

Text `--glass-text-strong` (pane titles) · `--glass-text-primary` (row titles, section labels) ·
`--glass-text-secondary` (subtitles, group captions, icons, values).
Surfaces `--glass-window-bg` / `--glass-window-border` (window), `--glass-window-pane-bg`
(content + sidebar pane), `--glass-subtle-bg` (row groups, cards, appearance panels),
`--glass-window-chip-bg` / `-chip-border` (icon chips), `--glass-hover-bg`, `--glass-divider`
(all hairlines), `--glass-scrim` (overlay scrim), `--color-destructive` (Zurücksetzen).
Both `dark` and `light` flip via the `theme` prop.

The only literal colours are the **accent palette**, the **folder colours** and the
mode-preview mockups — they are content, not chrome. See `reference/tokens.json`.

Radii `--radius-3xl` window · `--radius-2xl` panes · 15px appearance cards · 14px row groups ·
9px icon chips. Type `--font-sans` (Inter 400–700), sizes 10–14px.

## Motion

`--motion-instant` 0.1s · `--motion-swift` 0.16s · `--motion-quick` 0.28s · `--motion-slow` 0.5s ·
`--motion-spring` `cubic-bezier(0.34,1.4,0.5,1)` · `--motion-spring-soft` `cubic-bezier(0.34,1.2,0.5,1)`

| Element | Animation |
|---|---|
| Window mount | `aiva-window-in` 0.35s `cubic-bezier(0.16,1,0.3,1)` |
| Sidebar overlay (compact/mobile) | `transform .34s cubic-bezier(.16,1,.3,1), opacity .26s`; scrim `opacity .26s` |
| Overview cards | `background .16s, border-color .16s` on hover |
| Sidebar items, list rows | `background .12–.15s ease` |
| Toolbar buttons | press `scale(0.88)` at `--motion-instant`, release springs at `--motion-quick` |
| `GlassToggle` | thumb 0.28s spring-soft, track cross-fade 0.28s ease |
| `Menu` open | scale + fade, `--motion-swift` |
| Selection ring (appearance) | instant `box-shadow: 0 0 0 1.5px var(--glass-text-primary)` |

## Structure & interactions

Three content modes, one at a time in the content pane:

1. **overview** — card grid of all 12 sections. Entered by clicking the app icon, the sidebar
   header, the root breadcrumb, or `Übersicht` (⌘0) in the menu.
2. **browse** — the selected section, or a drilled-down sub-page (`sub`). Rows are grouped;
   groups carry an optional uppercase label and a footnote. Chevron rows push a sub-page.
   `Personalisierung › Erscheinungsbild` opens the custom **appearance** view instead of a row list.
3. **search** — replaces the content: `SearchBar`, suggestion chips, flat result list across the
   whole tree (title + subtitle + value + path matched, max 12), empty state „Keine Treffer“.
   With no query it lists the last 6 rows of the current section.

Back button: search → browse, sub-page → section, section → overview.

The three-dot menu carries view options (`Beschreibungen anzeigen`, `Sidebar kompakt`,
`Erweiterte Optionen`), jump-to-section submenu, help submenu, and destructive reset.
`Erweiterte Optionen` reveals groups flagged `advanced` (e.g. Mono-Audio under Sound).

## Data model

The whole settings tree is one declarative array in `_tree()`; each section is
`{ value, title, desc, icon, groups[] }` and each group `{ label, rows[], note, advanced }`.
Row kinds:

| Kind | Shape | Renders |
|---|---|---|
| `t` toggle | `{ t:'t', title, key, subtitle, icon }` | `GlassToggle` bound to `state.tg[key]` |
| `v` value | `{ t:'v', title, value, subtitle, icon }` | right-aligned static value |
| `s` select | `{ t:'s', title, key, options, subtitle, icon }` | `Select` bound to `state.sel[key]` |
| `n` nav | `{ t:'n', title, subtitle, icon, sub:[groups] }` | chevron row → sub-page |

State: `nav`, `sub`, `mode` (`overview` \| `browse` \| `search`), `query`, `showSubtitles`,
`sidebarDesc`, `advanced`, `tg{}` (39 booleans), `sel{}` (region/language/layout),
`ap{ mode, accent, iconStyle, folder }`.
Tweakable props: `theme`, `appName`, `appIconSrc`, `breadcrumbRoot`.

## Notes for implementation

- All values (Akku 82 %, 2560 × 1600, GB-Zahlen, Accountname) are demo data in `_tree()` —
  swap for the real system source; the tree shape is the contract.
- Search indexes `_index()` over sections → rows → sub-rows; keep the `path` string so results
  stay navigable.
- Appearance writes `--ap-accent` on its container; icon-style tiles derive their look from
  filters + blend modes over the same source SVG, not from separate icon sets.
- Keep `Icon` for glyphs so stroke width and colour stay consistent.
- `LAYOUT.md` has the metrics and behaviour rules a non-web (Rust) reimplementation needs.
