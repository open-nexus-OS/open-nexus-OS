# LAYOUT.md — metrics & behaviour for a native reimplementation

Everything below is measured from the shipped components (`reference/components/*.jsx.txt`).
Sizes are CSS px at 1× — treat them as logical points and scale by DPI. Colours are named
tokens; resolved values per theme are in `reference/tokens.json`.

## Window

| Property | Value |
|---|---|
| Default size | 900 × 600 (component default 960 × 600) |
| Corner radius | `--radius-3xl` |
| Surface | `window_bg` gradient + 1px `window_border`, backdrop blur 72px / saturate 180% |
| Shadow | `window_shadow` (outer drop + inner top highlight) |
| Title chrome | height = content + `padding: 10px 12px`, `gap: 8` |
| Body | `padding: 0 8px 16px 8px`, `gap: 8`, flex row |
| Mount animation | opacity 0→1, `scale(0.97) translateY(6px)` → identity, 0.35s `cubic-bezier(0.16,1,0.3,1)` |

Title chrome is three zones: leading (left, `gap: 6`), toolbar (absolutely centered,
`gap: 1`), trailing + window controls (right, `gap: 1`, separated by a 1px × 16px
`divider` with 6px side margin).

- **leading**: 32 × 32 app-icon chip, radius 9, `chip_bg` + 1px `chip_border`, 22 × 22 icon
  inside; then an 11px chevron-down in `text_secondary`; then the sidebar-toggle
  `WindowButton` (added by `AppWindow`).
- **toolbar**: Zurück · Vorwärts · divider · Suche. Disabled buttons render at `opacity .4`
  and ignore pointer events.
- **trailing**: three-dot menu button, divider, window controls.

## Panes

Both the sidebar and the content area are `WindowPane` cards:

| Property | Value |
|---|---|
| Radius | `--radius-2xl` |
| Surface | `pane_bg` gradient + 1px `pane_border` + `inset 0 1px 0 pane_inset` |
| Header | `padding: 12px 16px 10px`, bottom hairline `divider`, title 14px/600 `text_strong` |
| Body | scroll-y; padded default `6px 8px`; settings content uses `padded={false}` and its own `12px 16px 18px` |
| Sidebar width | 264 (component default 240) |

Content column and side panes sit side by side with an 8px gap.

## Responsive modes (observed on window width)

| Mode | Width | Sidebar |
|---|---|---|
| desktop | ≥ 820 | inline, toggleable (hidden state persists) |
| compact | ≥ 560 | overlay: inset 16px, width 254, radius 28 |
| mobile | < 560 | overlay: full height, width `min(300px, 82%)`, radius `0 20 20 0` |

Overlay: same window surface, `padding: 14px 14px 0`, slides from `translateX(-118%)` +
`opacity 0`, transition `transform .34s cubic-bezier(.16,1,.3,1), opacity .26s`. A scrim
(`scrim`, blur 2px, `opacity .26s`) sits behind it and closes on click. Switching mode
closes any open overlay.

## Sidebar

- Header: full-bleed strip (`margin: -14px -14px 0`, `padding: 13px 16px 11px`) with a bottom
  hairline; label 12px/600 `text_secondary`, clickable → overview.
- Items: icon (16px, stroke 2) + two text lines — title 12px/500, description 10px/400 at
  `opacity .65` (both single-line, ellipsised). Description line is toggleable
  (`Sidebar kompakt`). Selected item = filled row; hover `background .12–.15s ease`.
- Footer: 34px avatar with online dot + name 12px/500 `text_primary` + „AIVA Account“
  10px `text_secondary`; whole block clickable → Accounts section.
- 12 sections in order: Verbindungen · Verbundene Geräte · Sound & Töne · Monitore ·
  Benachrichtigungen · Personalisierung · Apps · Allgemeine Verwaltung · Accounts ·
  Datenschutz & Sicherheit · Energie Sparen · Geräte Info.

## Content — row groups (browse mode)

```
column, gap 14
  group: column, gap 7
    label   10.5px / 600 / letter-spacing .06em / uppercase / text_secondary / padding-left 2
    body    radius 14, background subtle_bg, overflow hidden
              row (ListItem, ~54px tall)
              1px divider inset 16px on both sides between rows
    note    11px / line-height 1.4 / text_secondary / padding 0 2 / text-wrap pretty
```

Row internals (`ListItem`): leading icon 16px stroke 2 in `text_secondary`; title
12.5–13px/500 `text_primary`; subtitle 11px `text_secondary`; trailing is a toggle, a
static value (12.5px/500 `text_secondary`, `nowrap`), a select (min-width 196), or a
chevron for nav rows. Whole row is the hit target — never below 44px on touch.

## Content — overview grid

`grid-template-columns: repeat(auto-fill, minmax(208px, 1fr))`, gap 10. Card: padding 14,
radius 14, `subtle_bg`, 1px transparent border → hover `hover_bg` + `chip_border`,
transition `.16s`. Inside: 30 × 30 icon chip (radius 9, `chip_bg` + `chip_border`, 17px
icon) + column (gap 2) with title 12.5px/600 `text_primary` and description 11px/1.35
`text_secondary`.

## Content — search mode

```
column, gap 14
  SearchBar               full width, 40px tall, placeholder "Alle Einstellungen durchsuchen"
  "VORSCHLÄGE"            group label style
  chips                   flex-wrap, gap 8; Chip ~96 × 28; selected = filled
  "<n> Treffer"           group label style
  results                 same 14px-radius subtle_bg list as browse groups, max 12 rows,
                          subtitle = breadcrumb path ("Verbindungen › Datennutzung")
  empty state             column centered, padding 44px 20px, radius 14, subtle_bg;
                          title 14px/600, body 12px text_secondary, max-width 280
```

Matching: case-insensitive substring over `title + subtitle + value + options + path`.
Suggestion chips: Bluetooth · Auflösung · Sprache · Akku · Berechtigungen — clicking one
sets the query, clicking it again clears it.

## Content — appearance page

Sections stacked with gap 18, each `column, gap 8` under a group label.

1. **Modus** — `grid 3 × 1fr`, gap 10. Tile: padding 9, radius 15, `subtle_bg`;
   preview block 82px tall, radius 10 (light `linear-gradient(135deg,#eef3fb,#cdd9ee)`,
   dark `(135deg,#0d1117,#1a2744)`, auto `(105deg,#eef3fb 0 49%,#101a2e 51%)`) containing a
   mock window inset 13%/18%/13%/20%, radius 8, with three 4px bars at 52% / 78% / 64% width;
   footer row with label 11.5px/600 and a 17px radio circle (1px `divider` border, 11px check).
   Selected tile: `box-shadow: 0 0 0 1.5px text_primary` inset overlay.
   Auto mode adds a footnote linking to Monitore.
2. **Akzentfarbe** — label row with the current accent name right-aligned (11.5px/500
   `text_secondary`); panel radius 15, `subtle_bg`, padding 14 16, `flex-wrap` gap 12;
   9 swatches 28 × 28 circles. Selected: ring `inset -4px` +
   `0 0 0 1.5px text_primary` and a white 13px check.
3. **App-Icons & Widgets** — `grid 4 × 1fr`, gap 10. Tile padding 12 11, radius 15,
   `subtle_bg`, centered column gap 10: three 32 × 32 icons (radius 9) + name 11.5px/600 +
   description 10px/1.3 `text_secondary`. Variants:
   - `Standard` — icon as-is.
   - `Dunkel` — `linear-gradient(165deg,#33373f,#14161a 55%,#07080a)` base,
     icon `grayscale(1) brightness(1.05) contrast(3.2)` + `mix-blend-mode: screen`.
   - `Klar` — `rgba(255,255,255,.13)` + 1px `rgba(255,255,255,.18)`,
     icon `grayscale(1) brightness(1.6) contrast(.45)` + `luminosity`.
   - `Getönt` — accent base, icon `grayscale(1) brightness(1.35) contrast(.9)` + `luminosity`.
4. **Ordnerfarbe** — panel radius 15, `subtle_bg`, padding 14 16, `flex-wrap` gap 14;
   six 44 × 34 folder glyphs (back flap in the light tint, front body in the base colour,
   see `folder_colors` in tokens.json) with 10px caption; selected ring `inset -3px`,
   radius 10. Footnote below.

## Behaviour contract

- Navigation state: `mode` (overview / browse / search) × `nav` (section) × `sub` (sub-page).
  Every jump resets `query` and `sub` unless the jump targets a sub-page.
- Breadcrumbs mirror that state: root → overview, section → clears `sub`, sub-page is a leaf.
- Back: search → browse · sub-page → section · section → overview. Forward is always disabled.
- Toggles, selects and appearance choices apply immediately — no confirm step, no Save button.
- `Beschreibungen anzeigen` off ⇒ every row subtitle is suppressed (rows shrink to one line).
- `Erweiterte Optionen` off ⇒ groups flagged `advanced` are removed from the tree entirely.
- The window never scrolls as a whole; only pane bodies scroll.
