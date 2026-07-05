<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Design-System Component Inventory (IST → convergence target)

> STATUS: prep artifact for TASK-0073/0074. Captures the **current state** of every
> design-system component across the three parallel UI expressions, and the
> **"promote the best, not the incumbent"** convergence verdict per component.
> Companion: [`token-reconciliation.md`](../foundations/visual/token-reconciliation.md),
> RFC-0070 (SSOT convergence). Source contract:
> `docs/dev/design_handoff_open_nexus_os/reference/component-api.d.ts` (54 components / 67 interfaces).

## The three parallel expressions (the double/triple structure)

| Tag | Where | What | Reactive path? |
|---|---|---|---|
| **A** deklarativ | `userspace/ui/widgets/*`, `userspace/ui/shells/desktop` | pure `LayoutNode` builders, id-based interaction, host-tested | yes, by design — but **not the live path** |
| **B** scene-graph | `windowd/src/scene_graph.rs` | retained `SceneNode`/`RenderPrimitive` (Rect/StrokeRect/Surface/Text/BackdropFilter/Group/Cursor), dirty-set, µs invalidation → `nexus-gfx` | **yes — the µs-reactive engine** (partially fed) |
| **C** bespoke | `windowd/src/compositor/runtime/*` (~15k LOC) | hand-drawn per-row renderers (chat/search/settings/desktop_layer/greeter) — the **actually visible pixels** | no — bypasses A and B |

**Convergence target (RFC-0070):** one path `A → LayoutEngine → B → nexus-gfx`. C's
boot-hardened rendering/glass/scroll quality is **promoted into** the declarative components,
then C collapses surface-by-surface (boot-gated). The library SSOT home is the promoted
`userspace/ui/widgets/*` (7 groups), **not** new `ui/design`/`ui/kit` folders.

## Legend

- **Exists**: `A` = declarative widget crate · `C` = bespoke in windowd (file) · `—` = gap
- **Best src**: which implementation is qualitatively best → gets promoted; loser is deleted
- **Target home**: canonical crate/module after convergence (all under `userspace/ui/widgets/<group>/`)

---

## CORE (5)

| Component | Exists | Quality notes | Best src → promote | Gap to full contract |
|---|---|---|---|---|
| `GlassButton` (6 variants × 4 sizes) | A `nexus-widget-button` (140 LOC, basic Stack+Style) · C windowd title `[– □ ×]` buttons (hover, hit-test SSOT) | A is clean but variant-blind; C has hardened hover/press + hit-test | **A shape + C states** | variants (default/glass/ghost/destructive/active/secondary), sizes, icon slot, disabled |
| `GlassCard` (panel/dock/card/inner/subtle) | A `nexus-widget-panel` (190 LOC) · C windowd frosted-glass surfaces (backdrop blur, shine, shadow — boot-hardened) | C glass compositing is production; A has no real glass | **C glass → A panel** (via glass primitive) | 5 surface levels mapped to materials; the glass primitive itself (see RFC-0070) |
| `GlassToggle` (iOS switch) | C windowd settings theme-row toggle (bespoke) | live but hardcoded to one row | **new, model on C behavior** | full component: checked/default/onChange/label/disabled, spring thumb |
| `Badge` (8 variants) | — | — | **new** | all 8 variants |
| `AppIcon` (native/wrapped/freestanding, 5 sizes, badge, active) | C windowd dock glyphs (tinted, bespoke) · `nexus-svg` for art | icon rasterization is production (nexus-svg); tile/backing bespoke | **nexus-svg art + new tile** | 3 variants, badge, active dot, glass backing |

## CONTROLS (9)

| Component | Exists | Best src → promote | Gap |
|---|---|---|---|
| `Segment` (sliding thumb, 3 sizes) | — | **new** | full |
| `Slider` (drag + tap, icons) | — | **new** | full |
| `GlassCheckbox` | — | **new** | full |
| `GlassRadioGroup` | — | **new** | full |
| `Stepper` (−/+) | — | **new** | full |
| `Select` (frosted dropdown, not native) | C windowd topbar dropdown (`open_topbar_menu`, bespoke) | **C dropdown behavior → new Select** | typed options, checkmark, sizes |
| `Rating` (fractional) | — | **new** | full |
| `WheelPicker` (snap scroll) | A `nexus-virtual-list` physics reusable | **virtual-list physics + new band UI** | snap, selection band |
| `DatePicker` (3 wheels) | — (composes WheelPicker) | **new (compose)** | full |

## INPUTS (3)

| Component | Exists | Best src → promote | Gap |
|---|---|---|---|
| `TextField` (label/icon/trailing/helper/error, 3 sizes) | A `nexus-widget-text-field` (130 LOC) · C windowd search filter field | **A + C caret/edit behavior** | label/icon/trailing/helper/error/sizes |
| `SearchBar` (iOS pill, clear, submit) | C windowd search window filter (bespoke, hardened) | **C behavior → new SearchBar** | pill chrome, clear button |
| `TextArea` (auto-grow, counter) | — | **new** | full |

## FEEDBACK (7)

| Component | Exists | Best src → promote | Gap |
|---|---|---|---|
| `Spinner` (12 spokes) | — | **new** | full |
| `ProgressBar` (determinate/indeterminate) | — | **new** | full |
| `Toast` (auto-dismiss, action) | — (design: system-toast surface) | **new** — feeds 0074 toast unification + 5-surface routing | full |
| `Skeleton` / `SkeletonText` | — | **new** | full |
| `Banner` (4 variants, inline) | — | **new** | full |
| `Refresher` (pull-to-refresh) | A `nexus-virtual-list` scroll host | **virtual-list + new gesture** | threshold, spinner |

## NAVIGATION (13)

| Component | Exists | Best src → promote | Gap |
|---|---|---|---|
| `Toolbar` (leading/center/trailing) | C windowd desktop_layer topbar (bespoke, live) | **C behavior → new Toolbar** | slots, centerTitle, variants |
| `TabBar` (bottom tabs, badge) | — | **new** | full |
| `List` / `ListItem` (dividers, chevron, destructive) | A `nexus-virtual-list` (1457 LOC, production scroll) · C chat/search row rendering | **virtual-list + declarative ListItem row** | ListItem contract (leading/title/subtitle/trailing/chevron) |
| `SubHeader` | — | **new** | full |
| `Accordion` (animated height) | — | **new** | full |
| `Sidebar` / `SplitView` (panel/plain) | C windowd sidepanel (bespoke) · A `shells/desktop` composes | **A shell shape + C render** | item model, active accent, header/footer |
| `TreeView` (expand, indent) | — | **new** | full |
| `Breadcrumbs` | — | **new** | full |
| `Pagination` | — | **new** | full |
| `Avatar` (image/initials/status) | — | **new** | full |
| `Chip` (selectable/removable) | — | **new** | full |

## OVERLAYS (9) — modal-manager targets (0074)

| Component | Exists | Best src → promote | Gap |
|---|---|---|---|
| `Modal` (backdrop, header, footer, close) | — (windowd has fullscreen/window mgmt, not modal) | **new — 0074 modal manager** | focus trap, backdrop, ESC |
| `ActionSheet` (bottom, grouped) | — | **new — 0074** | full |
| `Alert` (1–2 buttons) | — | **new — 0074** | full |
| `Popover` / `PopoverItem` (anchored) | C windowd dropdown (bespoke) | **C anchor/dismiss → new Popover** | placement, offset |
| `Menu` / `ContextMenu` (submenu, shortcuts) | C windowd topbar menu (`AppMenu`, bespoke) | **C `AppMenu` → new Menu** | submenu, shortcuts, checked, dividers |
| `Tooltip` (hover/focus) | — | **new** | full |
| `FAB` (expandable) | — | **new** | full |

## WINDOW (8) — the shell scaffold

| Component | Exists | Best src → promote | Gap |
|---|---|---|---|
| `Window` (dense surface + 3-zone chrome) | A `nexus-widget-window` (`Frame`/`ResizeEdge`/`TitleButton`/`WindowPress`, 577 LOC — **hardened by track 0070-72**) · C windowd renders it | **A `Frame` (already the SSOT for hit/resize) + C render quality** | window material tokens, animate |
| `WindowControls` (– □ ×) | A `nexus-widget-window::TitleButton` · C render | **A** (already SSOT) | — mostly done |
| `WindowButton` (ghost/active/danger) | C windowd chrome buttons | **promote into A window crate** | active/danger treatments |
| `WindowPane` (inner card + header) | A `nexus-widget-panel` partial · C windowd window bodies | **A panel + C header** | header actions, scrolling body |
| `AppWindow` (sidebar·content·properties, responsive) | A `shells/desktop` partial · C windowd shell | **compose from Window+Sidebar+Pane** | responsive collapse (≥820/≥560/<560) |
| `WindowActionBar` (floating pill) | — | **new** | full |
| `Icon` (Lucide SVG renderer) | A `nexus-svg` (production AA+arc rasterizer) | **nexus-svg** (already SSOT) | thin Icon wrapper over nexus-svg |
| — (`Frame` resize/snap/dock) | A `nexus-widget-window` + windowd wm.rs/snap.rs/dock.rs | **A** (already SSOT, host-tested) | — done |

---

## Summary counts

- **54 components / 67 interfaces** in the handoff contract.
- **Already have a promotable impl** (A and/or C): ~18 (button, card, toggle*, appicon*, select*, wheelpicker*, textfield, searchbar*, refresher*, toolbar*, list/listitem, sidebar*, popover*, menu*, window, windowcontrols, windowbutton*, windowpane, icon). `*` = behavior exists in C, needs promotion into a declarative component.
- **Pure gaps** (net-new, but built to full contract from tokens + glass primitive): ~30 (segment, slider, checkbox, radiogroup, stepper, rating, datepicker, textarea, spinner, progressbar, toast, skeleton(s), banner, tabbar, subheader, accordion, treeview, breadcrumbs, pagination, avatar, chip, modal, actionsheet, alert, tooltip, fab, windowactionbar, badge, appwindow-responsive…).
- **Already at/near SSOT** (declarative + hardened): `Window`/`Frame`, `WindowControls`, `List` (virtual-list), `Icon` (nexus-svg) — these validate the target architecture.

## Convergence ordering (feeds the RFC-0070 waves)

1. **W1** token SSOT + glass primitive (unblocks every glass surface).
2. **W2 core** — GlassButton, GlassCard, GlassToggle, Badge, AppIcon.
3. **W3 controls/inputs** — Segment, Slider, Checkbox, Radio, Stepper, Select, TextField, SearchBar, TextArea, WheelPicker/DatePicker, Rating.
4. **W4 overlays** — Modal/ActionSheet/Alert/Popover/Menu/Tooltip/FAB + the **modal manager** (0074).
5. **W5 navigation/window** — Toolbar, TabBar, List/ListItem, Sidebar/SplitView, Accordion, TreeView, Breadcrumbs, Pagination, Avatar, Chip, SubHeader + Window/WindowPane/AppWindow/WindowActionBar.
6. **W6 windowd convergence** — collapse C surface-by-surface onto A→B (boot-gated per surface).
7. **W7** DSL emit alignment + 5-surface notification routing.
