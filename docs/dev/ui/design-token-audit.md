<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Design-token audit — handoff `reference/tokens/` + `component-api.d.ts` vs. implementation

Audit vom 2026-07-14 (Quelle: `docs/dev/design_handoff_open_nexus_os/reference/tokens/*.css`
+ `component-api.d.ts` gegen `userspace/ui/theme-tokens`, `resources/themes/base.nxtheme.toml`,
DSL-Registry). Die BEHOBENEN Punkte sind markiert; der Rest ist die offene Arbeitsliste.

## Behoben (2026-07-14)

- **Gradients als Primitiv**: `VisualStyle.background_gradient` + DSL `.bgGradient(top, bottom)`
  (modId 49, append-only) + Zeilen-Lerp im `scene_raster`-Painter. Nutzt: App-Icon-Artwork,
  Glass-Shine.
- **Glass-Shine wird gerendert**: `GlassSurface.edge` (edge_highlight_*) war token-only —
  jetzt komponiert `Mods::visual()` den Design-`--glass-shine` als vertikalen Verlauf
  tint⊕edge → tint auf jede Glass-Fläche ohne explizites `.bg`.
- **Radius-Skala**: `.rounded()` folgt jetzt der Handoff-Skala sm 6 / md 8 / lg 10 / xl 14 /
  xxl 16 (vorher 4/8/12/16).
- **DSL-Mappings ergänzt**: `warning/onWarning/info/onInfo/focusRing/shadow` (Farben),
  `xxl/xxxl/display` (Typo).
- **Blur-Layering**: Fenster-Glass-Backdrop-Cache wird invalidiert, wenn sich darunter
  Inhalt ändert (Desktop-Damage, Present eines überlappten Fensters, Drag) — Blur zeigt den
  echten Hintergrund statt des Wallpaper-Snapshots.

## Offen — Token-Abweichungen

| Rolle | Handoff | Implementierung | Status |
|---|---|---|---|
| `warning-fg` | `#ffffff` | `#0a0a0a` | echte Abweichung, klären |
| `divider` | `rgba(0,0,0,.10)` | `#d4d4d4` opak | angleichen |
| `success` | `#22c55e` | `#16a34a` | GEWOLLT (a11y, dokumentiert) |
| `destructive` | `#d4183d` | Wert in TOML, **kein `ColorToken::Destructive`** | Rolle ergänzen |
| glassWindow tint | 165°-Gradient 2 Stops | Solid (Stop 1) | GPU-Glass-Tint als Gradient (FS_SDF_GRAD existiert) |
| glassSubtle | kein Border/Blur | erfundener Border + blur 12 | angleichen (blur→8/sm) |
| Blur-Skala | benannte Tokens sm 8/md 20/lg 40/xl 64 | nur per-Material `blurRadiusDp` | Tokens einführen |

## Offen — fehlende Primitives

- **Shadows**: kein `ShadowToken`-Set/`[shadow]`-TOML; `.shadow`-Modifier deklariert, nie
  konsumiert. Handoff-Elevation: sm `0 1px 2px .12` · md `0 4px 12px .15` · lg `0 8px 24px .18`
  · xl `0 12px 32px .22` · 2xl `0 25px 50px .25` + icon/dock/label-Schatten + mehrlagige
  Material-Schatten mit inset (window: `0 30px 60px .30, inset 0 1px 0 .85`).
- **Scrim**: `--glass-scrim rgba(0,0,0,.28)` [dark .45] fehlt (Modal/Alert-Backdrop).
- **Borders**: nur uniform 1px; per-Seite (Sidebar right-border, pane-border) + die
  Material-Border-Farben (window-pane `.07`, chip `.09`, bar `.95`, icon `.20`) fehlen.
- **material(overlay)**: `MaterialToken::Overlay` existiert, `GlassLevel` + DSL-Mapping nicht.

## Offen — Component-API (Handoff prop-basiert vs. DSL modifier-basiert)

Fehlende Komponenten: Select, Segment, Stepper, Rating, RadioGroup, WheelPicker, DatePicker,
AppIcon (adaptiv), TextArea, Accordion, Breadcrumbs, Pagination, Sidebar, SplitView, SubHeader,
TabBar, TreeView, ActionSheet, Alert, FAB, Menu, ContextMenu, Modal, Popover, Tooltip,
Refresher, SkeletonText — plus die Window-Familie als DSL-Komponenten (Widget-Crates existieren).

Vorhandene mit Prop-Lücken: Button (variant glass/ghost/active), Card (variant-Auswahl),
Toggle (`label` ignoriert), TextField (error/helper/icon/trailing/type/size), ListItem
(subtitle/trailing/chevron/destructive), List (Hairline-Divider, `inset`), Toolbar
(subtitle/leading/trailing/centerTitle), Badge (variant-Set).
