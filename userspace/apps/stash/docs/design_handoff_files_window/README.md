# Handoff — Dateien-Fenster (OS App Window Template)

File-manager window built on the **open nexus OS Design System**. Everything in this folder is
copied verbatim from the design system — no forked or re-styled code.

Namespace at runtime: `window.AIVAOSDesignSystem_afec52`

```
source/OsAppWindow.dc.html   the template (markup + logic)
source/ds-base.js            loads styles.css + _ds_bundle.js; edit the `base` path when moving it
reference/styles.css         all design tokens (colors, glass, radii, motion, type)
reference/components/*.txt   source + typed API of every component used here
                             (`.jsx.txt` / `.d.ts.txt` — strip the `.txt` to compile them)
```

## Run it

`source/ds-base.js` resolves the design system from `base` (default `../..`). Drop the `source/`
folder two levels below the design-system root, or change `base` to point at the bound
`_ds/<folder>` tree. Then open `OsAppWindow.dc.html` in a browser — no build step.

## Components used

| Component | Where | Notes |
|---|---|---|
| `AppWindow` | window shell | slots: `leading`, `toolbar`, `trailing`, `sidebar`, `contentHeader`, `contentActions`, `properties`, `actionBar`, `children`. Sidebar- and properties-toggle buttons are added by the component. Responsive: desktop ≥820px, compact ≥560px, mobile below — panes become overlays. |
| `WindowButton` | toolbar, trailing, content actions | `title`, `active`, `onClick`. `active` drives the highlighted state of Kacheln/Liste, Suche and Mehr. |
| `WindowActionBar` | floating bottom bar | `items[]` with `label`, `icon`, `variant` (`primary` / `danger` / `muted`), `divider`. |
| `Icon` | all glyphs | Lucide-style path strings, `size`, `strokeWidth`, `color`. |
| `Sidebar` | left navigation | `items`, `value`, `onChange`, `header`, `footer`, `variant="plain"`. |
| `Breadcrumbs` | content header | `items` (Dieser PC › Bilder › Urlaub 2026). |
| `ListItem` | properties pane | `leading`, `title`, `trailing`. |
| `Avatar` | sidebar footer | `initials`, `size`, `status="online"`. |
| `GlassToggle` | settings panel | controlled: `checked` + `onChange`. |

Only the dropdown menu, the search field, the file list/grid rows and the settings panel are
local markup — they are composed from tokens only, no new components.

## Colors — tokens only, no literals

Text `--glass-text-strong` (primary values, headings) · `--glass-text-primary` (file names, menu
items) · `--glass-text-secondary` (meta, column heads, counts, icons).
Surfaces `--glass-window-bg`, `--glass-window-border`, `--glass-surface-strong` (dropdown),
`--glass-divider` (all hairlines), `--glass-hover-bg`, `--glass-card-bg`, `--glass-toggle-on-bg`,
`--glass-toggle-on-shadow`, `--glass-scrim` (overlay scrim), `--color-destructive` (Löschen).
Both `dark` and `light` themes are supported via the `theme` prop; every value above flips with it.

Local raised surfaces (search field, grid tiles, settings panel) use `rgba(255,255,255,0.03–0.07)`
over the glass background plus a `--glass-divider` border — the DS glass-elevation recipe.

Radii `--radius-2xl` / 14px panels / 12px dropdown / 8px search + rows.
Type `--font-sans` (Inter 400–700), `--font-mono` for file sizes. Sizes 10–13.5px.

## Motion — design-system tokens

- `--motion-instant` 0.1s · `--motion-swift` 0.16s · `--motion-quick` 0.28s · `--motion-slow` 0.5s
- `--motion-spring` `cubic-bezier(0.34,1.4,0.5,1)` · `--motion-spring-soft` `cubic-bezier(0.34,1.2,0.5,1)`

| Element | Animation |
|---|---|
| Window mount | `aiva-window-in` 0.35s `cubic-bezier(0.16,1,0.3,1)` (Window/AppWindow) |
| Sidebar / properties pane (compact + mobile) | slide + fade `transform .34s cubic-bezier(.16,1,.3,1), opacity .26s`; scrim fades `opacity .26s` — all inside `AppWindow` |
| Properties pane in settings mode | driven by `AppWindow`'s `showProperties` prop, not by unmounting the slot |
| Action bar in settings mode | `translateY(18px) scale(0.96)` + fade out, `--motion-quick` / `--motion-spring-soft` |
| Toolbar buttons | press `scale(0.88)` at `--motion-instant`, release springs back at `--motion-quick` / `--motion-spring-soft` |
| `GlassToggle` | thumb slides 0.28s spring-soft, track cross-fades 0.28s ease |
| Dropdown items | `background 0.12s ease` on hover |
| Sidebar items, list rows | `background 0.12–0.15s ease` |

## Interactions implemented

- **Suche** (toolbar magnifier) toggles a search field between content header and list; live
  substring filter on the file name, object counter follows the result count, empty state
  "Keine Objekte gefunden", ✕ clears, closing the field resets the query.
- **Kacheln / Liste** switch the content between a table (Name · Datum · Größe) and a
  responsive tile grid; the active mode is highlighted.
- **Sortier-Icons** in the content actions: direction (auf/absteigend), by size, by type.
  Visual only — wire to your sort model.
- **Mehr (⋯)** opens a dropdown with *Einstellungen*. Settings mode replaces the file content
  with a settings panel, hides the properties pane and the action bar, and shows a back arrow.
- **Versteckte Dateien anzeigen** (GlassToggle) reveals two dot-files in the list and updates
  the counter.

## State model

`nav`, `query`, `searchOpen`, `view` (`list` | `grid`), `menuOpen`, `settingsOpen`, `showHidden`.
Tweakable props: `theme`, `appName`, `appIconSrc`, `sidebarLabel`, `breadcrumbRoot`,
`breadcrumbMid`, `breadcrumbCurrent`, `fileCount`.

## Notes for implementation

- Files, dates and sizes are demo data inside the logic class — replace with your data source.
- Sorting and the file-type icons (emoji placeholders) are the two open ends.
- Keep the `Icon` component for glyphs so stroke width and color stay consistent.
