---
title: TASK-0073 UI v10a (host-first): Design-System SSOT — token convergence + glass primitive + core/controls/inputs/nav/window primitives (full contract, fidelity waves) + goldens + a11y/contrast lints + HIG-grade docs
status: Draft
owner: @ui
created: 2025-12-23
updated: 2026-07-05
depends-on: []
follow-up-tasks:
  - TASK-0074 (app shell + modal manager + overlays wave + SystemUI/app adoption)
links:
  - Architecture spine: docs/rfcs/RFC-0070-ui-design-system-ssot-convergence.md
  - Component inventory (IST + promote verdict): docs/dev/ui/components/inventory.md
  - Token reconciliation (4 sources → 1 SSOT): docs/dev/ui/foundations/visual/token-reconciliation.md
  - Design contract (54 components / 67 interfaces): docs/dev/design_handoff_open_nexus_os/
  - DSL emit target: tasks/TRACK-DSL-V1-DEVX.md, tasks/TASK-0075-dsl-v0_1a-syntax-ir-cli.md
  - Layout/pretext baseline: docs/rfcs/RFC-0057 (nexus-layout)
  - Scene-graph/theme baseline: docs/rfcs/RFC-0063
  - windowd boundary: docs/rfcs/RFC-0067 (this task realizes the W6 direction)
  - Testing contract: scripts/qemu-test.sh
---

## Context (updated 2026-07-05)

The 2025-12 draft assumed a green field (`userspace/ui/design` + `userspace/ui/kit` as new
folders). That is no longer true and would create a **fourth parallel UI structure**. The real
state (see the inventory + RFC-0070):

- The **declarative foundation already exists** (RFC-0057): `nexus-layout-types` (`LayoutNode`
  tree), `nexus-layout` (measure/place engine), `nexus-style` (modifier styling), `nexus-theme
  (-tokens)`, plus `nexus-svg/shape/sdf/effects`.
- A **widget layer already exists** (`userspace/ui/widgets/{button,panel,text_field,window,
  virtual_list}` + `ui/shells/desktop`) — pure `LayoutNode` builders, id-based interaction,
  host-tested. This is the SSOT to **promote and complete**, not duplicate.
- The same UI also lives as windowd's retained **scene graph** (the µs-reactive engine) and as
  ~15k LOC of **bespoke row-renderers** in `windowd/compositor/runtime/*` (the visible pixels).
- Tokens are fractured across **four** representations that silently drift.

This task delivers the **design-system SSOT**: one declarative component library completing the
full handoff contract to Apple/ArkUI fidelity, one token source, one glass primitive — all on the
single reactive path `LayoutNode → LayoutEngine → SceneGraph → nexus-gfx`. It is **host-first**;
the windowd live-convergence (W6) and app-shell/modals (W4/adoption) are `TASK-0074` and follow-ons.

**User intent (2026-07-05):** production-grade Apple quality, not the minimal solution; full
versatility (SwiftUI / ArkUI `advanced_ui_component` level); no double structures — promote the
**best** impl, not the incumbent; keep `docs/dev/` continuously updated to **Human-Interface-
Guidelines quality** as a first-class deliverable.

## Goal

Deliver, per RFC-0070 waves **W1–W3 + W5-non-overlay** (overlays/modal are W4 → TASK-0074):

1. **W1 — Foundations (token SSOT + glass primitive):**
   - Extend `.nxtheme.toml` to the full handoff contract (theme-varying colors + glass materials;
     theme-invariant type/spacing/radius/shadow/motion/z scales authored once) per
     `token-reconciliation.md`, incl. the `accent`-semantics decision and oklch→hex conversion.
   - **Generate** `nexus-theme-tokens` typed enums + `Tokens` impl from the toml (build.rs); delete
     the hardcoded `BaseTokens`; fold windowd `ThemeTokens` into the same bake (one path).
   - One **glass primitive** (blur+tint+border+shine+shadow) in `nexus-effects`/`nexus-gfx`,
     consuming material tokens; promote windowd's hardened glass compositing into it.
   - Motion presets live in tokens + `nexus-style`/`animation` — **no `ui/design` façade crate.**
2. **W2 — core primitives:** GlassButton (6 variants × 4 sizes), GlassCard (5 levels), GlassToggle,
   Badge (8 variants), AppIcon (3 variants × 5 sizes).
3. **W3 — controls/inputs:** Segment, Slider, GlassCheckbox, GlassRadioGroup, Stepper, Select,
   Rating, WheelPicker, DatePicker; TextField, SearchBar, TextArea.
4. **W5-nav/window (non-overlay):** Toolbar, TabBar, List/ListItem, Sidebar/SplitView, Accordion,
   TreeView, Breadcrumbs, Pagination, Avatar, Chip, SubHeader; Window/WindowPane/AppWindow/
   WindowActionBar/WindowButton/WindowControls/Icon.
5. **Full contract up-front:** all 54 component signatures (variants/sizes/states as typed enums,
   id interaction, A11y roles) defined from `component-api.d.ts` even where implementation lands in
   a later wave — so nothing is rebuilt.
6. **Snapshot golden harness:** each primitive rendered in states (default/hover/pressed/disabled/
   focus) × light/dark (+ highcontrast where meaningful); pixel-exact preferred, documented SSIM
   threshold otherwise.
7. **A11y lints:** min touch-target + WCAG-AA-style contrast (configurable threshold); no component
   reads a raw color/length — tokens only.
8. **HIG-grade docs:** every component + foundation documented under `docs/dev/ui/` in the existing
   IA (`components/`, `foundations/visual/`), continuously updated — the target is a coherent,
   navigable design-system reference on the level of a Human Interface Guidelines site.

## Non-Goals

- Kernel changes.
- Overlays wave + modal manager + unified toast (W4) and SystemUI/app adoption — `TASK-0074`.
- windowd live-convergence execution (W6) — its own boot-gated tasks (this task lands the
  primitives + the W6 *direction*, not the collapse of `compositor/runtime/*`).
- MSDF path (`ui/msdf` stays parked).

## Constraints / invariants (hard requirements)

- **Promote the best, not the incumbent** (RFC-0070 D5): combine A's declarative form with C's
  boot-hardened render/glass/scroll/AA quality; delete the loser; record the verdict in the inventory.
- **One reactive path** (D1): components feed `LayoutEngine → SceneGraph`; no new bespoke renderers.
- **DSL-aligned** (D6): signatures are the DSL emit target 1:1; ship as host-testable `LayoutNode`
  builders — no rebuild expected in the DSL task.
- Deterministic rasterization/layout/rounding; explicit SSIM thresholds if not pixel-exact.
- State contract maps to the existing visible-state + live-input delivery; SVG-source icons canonical.
- Bounded caches (respect atlas/MAX_WINDOWS budgets). No `unwrap/expect`; no blanket `allow(dead_code)`.
- No company/product names anywhere.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Token SSOT: generated typed values == handoff `*.css` golden (oklch conversions documented);
  hardcoded `BaseTokens` deleted; one bake path for runtime + windowd.
- `cargo test` green for the primitive crates + `ui_v10_goldens`:
  - goldens for W1–W3 + W5-nav/window primitives in light/dark and key states,
  - SVG-source icon primitives (no PNG-only UI source icons),
  - a11y lints: touch targets meet minimum; contrast meets threshold.
- Glass primitive has a golden proving blur+tint+border+shine+shadow as one draw.

### Docs — required (HIG-grade, continuously updated)

- `docs/dev/ui/components/inventory.md` verdicts reflected; each shipped component has a doc entry
  in its group index; `foundations/visual/` tokens/materials/motion/typography pages current.
- `docs/dev/ui/components/design-system.md` + a `foundations/quality/goldens.md` describe the
  contract and the golden/a11y harness.

### Visual proof handoff — required

- core + controls + text primitives are ready to appear on the shared visible proof surface;
  hover/pressed/focus states line up with the live-input targets launcher/settings/modal reuse.

## Touched paths (allowlist)

- `resources/themes/*.nxtheme.toml` (extend to full contract)
- `userspace/ui/theme-tokens/`, `userspace/ui/theme/` (generated typed contract), `userspace/ui/style/` (motion/material)
- `userspace/ui/effects/` and/or `userspace/nexus-gfx/` (glass primitive)
- `userspace/ui/widgets/*` (promote + complete the component library, 7 groups)
- `userspace/ui/shells/desktop/` (reference consumer)
- `tests/ui_v10_goldens/` (new), `tools/gen-goldens.sh` (optional)
- `docs/dev/ui/components/*`, `docs/dev/ui/foundations/visual/*`, `docs/dev/ui/foundations/quality/goldens.md`
- `docs/rfcs/RFC-0070-*` (keep current)

## Plan (small PRs, fidelity waves)

1. **W1** token SSOT + generated typed contract + glass primitive (+ reconciliation golden).
2. **W2** core primitives + goldens + docs.
3. **W3** controls + inputs + goldens + docs.
4. **W5-nav/window** navigation + window primitives + goldens + docs.
5. Full-contract signature sweep (all 54 typed) + a11y lints + design-system + goldens docs.
