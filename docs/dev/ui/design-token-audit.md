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
- **Shadows (Elevation)**: `ShadowLevel`-Skala auf die Handoff-Werte gezogen (md 0/4/12 .15 …
  2xl 0/25/50 .25), `.shadow(sm|md|lg|xl|xxl)` konsumiert sie jetzt (Mods → `VisualStyle.shadow`),
  und `scene_raster` malt den weichen Schatten analytisch pro Zeile (Rounded-Rect-SDF +
  linearer Falloff, One-Shot beim Re-Render). Demo: Stash-Floating-Actionbar `.shadow(lg)`.
- **Scrim + Destructive**: `ColorToken::{Scrim, Destructive, OnDestructive}` + TOML (`scrim`
  base #00000047 / dark #00000073) + DSL-Mapping.
- **Divider**: transluzente Hairline per Handoff (base `#0000001a`, dark `#ffffff1a`).
- **glassSubtle**: Border entfernt, Blur auf die sm-Stufe (8) — wie im Handoff.
- **glassWindow-Gradient**: `GlassSurface.tint_bottom` (TOML `tintBottomColor/-Alpha`) —
  der Fenster-Body rendert den 2-Stop-Handoff-Verlauf (hell `#f8f9fb@.94 → #eceef3@.90`,
  dunkel `#34363e@.82 → #20222a@.74`); Blur auf lg (40).
- **material(overlay)**: `GlassLevel::Overlay` + Wire `GLASS_OVERLAY=4` (append-only) +
  DSL-Mapping — das Overlay-Material ist aus Seiten nutzbar.
- **warningFg**: bleibt bewusst near-black (Amber + Weiß ≈ 2.1 Kontrast) — dieselbe
  dokumentierte a11y-Verschärfung wie `success`; im TOML kommentiert.

## Offen — Token-Abweichungen

| Rolle | Handoff | Implementierung | Status |
|---|---|---|---|
| `success` | `#22c55e` | `#16a34a` | GEWOLLT (a11y, dokumentiert) |
| `warning-fg` | `#ffffff` | `#0a0a0a` | GEWOLLT (a11y, jetzt dokumentiert) |
| Blur-Skala | benannte Tokens sm 8/md 20/lg 40/xl 64 | per-Material `blurRadiusDp` (Werte jetzt auf Skalenstufen) | benannte Tokens optional |

## Offen — fehlende Primitives

- **Per-Seite-Borders im DSL** (Sidebar right-border, pane-border; `EdgeBorder` kann es,
  es fehlen die Modifier) + die Material-Border-Farben (window-pane `.07`, chip `.09`,
  bar `.95`, icon `.20`) als Tokens.
- **Mehrlagige Material-Schatten mit inset** (window: `0 30px 60px .30, inset 0 1px 0 .85`)
  — die einfache Elevation-Skala ist da; inset-Layers fehlen.
- **Icon/Dock/Label-Sonderschatten** (`--shadow-icon`, `--shadow-dock-*`, `--glass-label-shadow`).

## Offen — Component-API (Handoff prop-basiert vs. DSL modifier-basiert)

Fehlende Komponenten: Select, Segment, Stepper, Rating, RadioGroup, WheelPicker, DatePicker,
AppIcon (adaptiv), TextArea, Accordion, Breadcrumbs, Pagination, Sidebar, SplitView, SubHeader,
TabBar, TreeView, ActionSheet, Alert, FAB, Menu, ContextMenu, Modal, Popover, Tooltip,
Refresher, SkeletonText — plus die Window-Familie als DSL-Komponenten (Widget-Crates existieren).

Vorhandene mit Prop-Lücken: Button (variant glass/ghost/active), Card (variant-Auswahl),
Toggle (`label` ignoriert), TextField (error/helper/icon/trailing/type/size), ListItem
(subtitle/trailing/chevron/destructive), List (Hairline-Divider, `inset`), Toolbar
(subtitle/leading/trailing/centerTitle), Badge (variant-Set).
