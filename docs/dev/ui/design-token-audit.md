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

## Behoben (2026-07-26, RFC-0082 / TASK-0305)

- **Type-Ramp**: der gebackene Atlas trägt jetzt eine (Größe, Gewicht)-Leiter
  (13/16 Regular *Full*, 13/16/21 SemiBold + 21 Regular *Latin*, 36 SemiBold,
  120 Light *Digits*). `probe/paint.rs`'s `>= 15`-Schwelle ist weg;
  `FontSize::nearest(px, weight)` ist die eine, dokumentierte Auflösung
  (Größe schlägt Gewicht). CJK oberhalb 16 px fällt bewusst auf das 16-px-
  *Full*-Face zurück. 13/16 px sind **byte-identisch** geblieben.
- **`.fontWeight` / `.leading` / `.textAlign` / `.paddingTop|Bottom|Leading|
  Trailing` / `.border` / `.borderColor`** sind keine No-ops mehr.
- **Text-Shadow**: `.textShadow(none|soft|strong)` (modId 50) + zweiter
  `draw_text_row`-Pass. Bewusst OHNE Blur — der Zeilen-Painter hat keinen
  Offscreen-Puffer; die Token sind Emphase-Stufen, keine Radien.
- **Glas-Rand + Inset-Top-Shine**: `GlassSurface.border` wurde aus dem Theme
  aufgelöst und dann verworfen — jede Glasfläche im OS war randlos. Jetzt
  1-px-Hairline + `inset 0 1px 0` als echte Pixelzeile
  (`VisualStyle.inset_highlight`); der weiche Shine-Verlauf ist auf 15 % der
  `edgeHighlight`-Alpha gedeckelt, sonst bleicht der 0.60-Wert die Fläche.
- **EIN Glas-Rezept**: `Style::glass(level, tokens)` ist die einzige
  Definition. `Card`, `Banner`, `Toast`, `Avatar`, `TextField` (Pill) und der
  DSL-`.material()`-Pfad rufen sie — vorher hatte jedes Widget seine eigene
  Teilmenge (deshalb fehlte dem Card der Shine).
- **On-Glass-Rollen**: `onGlass`/`onGlassMuted`/`onGlassStrong` (die schon
  authorten `glassText*` sind jetzt echte `ColorToken`), `glassIcon`,
  `glassPlaceholder`, `glassFocus`, `glassFill`, `wallpaperTint`,
  `wallpaperVignette`, `textShadow`, `textShadowStrong`, `transparent`.
- **`.bgFade(top, bottom)`** (modId 51): Verlauf aus zwei Farb-Tokens, damit
  eine Vignette dem Theme folgt (`.bgGradient` bleibt hex-basiert, weil
  App-Icon-Artwork Daten sind).
- **Token-Argumente werden geprüft**: `.fg(oNSurface)` ist ein Compile-Fehler
  statt eines stummen No-ops (`registry::TOKEN_VOCABULARIES`).
- **Glas-Level nachgezogen**: `glassPanel`/`glassCard` tragen die Werte des
  Login-Handoffs (dark = dunkler Tint `#121214@.40` statt weißem Wash).

## Behoben (2026-07-28, TASK-0308 Phase 9 / Stash-Design-Parität)

- **Sechs Rollen waren authored-but-unreachable.** `divider`, `glassHover`,
  `glassActive`, `toggleOnBg`, `toggleOffBg` und `notifDot` standen seit dem
  ersten Token-Pass in JEDER `.nxtheme.toml`, hatten aber keinen Eintrag in
  `theme-tokens/build.rs::ROLES` — und dieses Table ist das Gate, nicht die
  TOML. Aus einer `.nx`-Seite waren sie damit unsagbar. Folge im Bild: jede
  Haarlinie in einem Glasfenster wurde mit `border` gemalt (dark `#262626`,
  OPAK) statt mit `divider` (`rgba(255,255,255,.10)`). Jetzt `ColorToken`,
  `COLOR_TOKENS`, `color_token` und `ROLES` — vier Listen, die
  `userspace/dsl/runtime/tests/token_vocabulary_lockstep.rs` gegeneinander
  hält (Bijektion, nicht nur Coverage).
- **`--glass-window-pane-*` und `--glass-window-bar-*` existieren.** Neue
  Material-Ebenen `glassWindowPane` (dark `#484a54@.48 → #34363e@.32`) und
  `glassWindowBar` in allen vier Themes. Vorher erbten Fenster-Panes
  `glassPanel` (dark `#121214@.40`, nahezu schwarz) — die richtige Ebene für
  eine Dock-Kachel auf dem Wallpaper, die falsche für eine Pane auf
  Fensterglas. Das Wire-Feld `glass_level` ist ein BLUR-BUCKET (windowd liest
  daraus nur den Radius; Tint/Shine/Rand malt app-host selbst), deshalb kosten
  die zwei Ebenen kein neues Wire-Symbol: `windowPane` fährt den 20er-Bucket,
  `windowBar` den 40er.
- **On-Glass-Alphas auf den Handoff gezogen.** `glassText{Primary,Secondary,
  Strong}` trugen `.95/.68/1.0` (dark `.95/.60/1.0`) — undokumentierte Drift,
  die die Primary/Secondary-Stufe einebnete. Jetzt die Handoff-Werte
  `.80/.40/.92` bzw. dark `.90/.45/.95`. Der HUE (blau-schwarz `#14141a` statt
  reinem Schwarz) bleibt die bewusste RFC-0082-Entscheidung.
- **`divider` in `light.nxtheme.toml` war opak** (`#d4d4d4`) und damit nur auf
  einer weißen Vollfläche richtig → `rgba(0,0,0,.10)` wie der Handoff.
- **Hover ist wieder eine Fläche**, nicht nur Bewegung: `probe/paint.rs` gab
  hart `None` für den `HoverWash`, weil der Wash dem `corner_radius` der
  HANDLER-Box folgt und die bei `Stack { Pill } on Tap` keinen hat („jeder
  gehoverte Kreis trug ein weißes Quadrat"). Gelöst über den Anker statt über
  das Abschalten: `app-host/src/hover_wash.rs` (host-getestet, weil `probe/`
  nur für RISC-V baut) wählt das Kind, das die Wrapper-Box wirklich ausfüllt.
  Farbe jetzt `glassHover` statt eines Accent-Tints.

## Offen — Token-Abweichungen

| Rolle | Handoff | Implementierung | Status |
|---|---|---|---|
| `success` | `#22c55e` | `#16a34a` | GEWOLLT (a11y, dokumentiert) |
| `warning-fg` | `#ffffff` | `#0a0a0a` | GEWOLLT (a11y, jetzt dokumentiert) |
| Blur-Skala | benannte Tokens sm 8/md 20/lg 40/xl 64 | per-Material `blurRadiusDp` (Werte auf Skalenstufen) + eine ZWEITE, hartkodierte Skala im Compositor (`scene.rs` Level→Radius) + eine DRITTE für Ganzfenster-Glas (`DARK_GLASS_BLUR_RADIUS`) | drei parallele Skalen; die Theme-Werte gewinnen nur mittelbar über die Bucket-Wahl |

## Offen — fehlende Primitives

- **Per-Seite-Borders im DSL** (Sidebar right-border, pane-border; `EdgeBorder` kann es,
  es fehlen die Modifier). Die Material-Border-Farben für window-pane (`.07`) und
  bar (`.95`) tragen jetzt die Ebenen selbst; chip (`.09`) und icon (`.20`) fehlen
  weiterhin — der App-Chip im Fenster-Chrome fährt ersatzweise `glassSubtle`.
- **Mehrlagige Material-Schatten** (window: `0 30px 60px .30` PLUS einer
  inset-Lage): die Elevation-Skala und die inset-Top-Linie existieren jetzt
  einzeln, aber nicht als frei stapelbare Schatten-Liste.
- **Icon/Dock-Sonderschatten** (`--shadow-icon`, `--shadow-dock-*`).
  `--glass-label-shadow` ist als `.textShadow(strong)` abgedeckt.
- **Echter Gauß-Textschatten** (`0 2px 24px`): braucht einen Offscreen-Pass;
  der Zeilen-Painter kann heute nur den 1-px-Versatz.
- **Buchstabenabstand / tabellarische Ziffern als Modifier**: das Hero-Face
  ist ungekernt mit einheitlicher Ziffernbreite gebacken (eine tickende Uhr
  wackelt nicht), aber `letter-spacing` hat weiterhin keinen Code-Pfad.

## Offen — Component-API (Handoff prop-basiert vs. DSL modifier-basiert)

Fehlende Komponenten: Select, Segment, Stepper, Rating, RadioGroup, WheelPicker, DatePicker,
AppIcon (adaptiv), TextArea, Accordion, Breadcrumbs, Pagination, Sidebar, SplitView, SubHeader,
TabBar, TreeView, ActionSheet, Alert, FAB, Menu, ContextMenu, Modal, Popover, Tooltip,
Refresher, SkeletonText — plus die Window-Familie als DSL-Komponenten (Widget-Crates existieren).

Vorhandene mit Prop-Lücken: Button (variant glass/ghost/active), Card (variant-Auswahl),
Toggle (`label` ignoriert), TextField (icon/trailing/type — `error`/`helper`/`size`/
`variant` sind mit RFC-0082 verdrahtet), ListItem
(subtitle/trailing/chevron/destructive), List (Hairline-Divider, `inset`), Toolbar
(subtitle/leading/trailing/centerTitle), Badge (variant-Set).
