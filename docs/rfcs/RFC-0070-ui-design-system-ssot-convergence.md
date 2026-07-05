# RFC-0070: UI design-system SSOT convergence — one declarative component library, one token source, one reactive path

- Status: **Draft** (2026-07-05) — architecture spine for TASK-0073 (primitives) + TASK-0074 (app shell/modals). User decision 2026-07-05: full convergence, staged; no façade — one SSOT; promote the best impl, not the incumbent; Apple production-grade, not the minimal solution.
- Owners: @ui @runtime
- Created: 2026-07-05
- Links:
  - Design contract: `docs/dev/design_handoff_open_nexus_os/` (54 components / 67 interfaces, tokens, 4 glass levels, motion, 7 templates, 5-surface notifications).
  - Prep artifacts: `docs/dev/ui/components/inventory.md`, `docs/dev/ui/foundations/visual/token-reconciliation.md`.
  - Builds on: **RFC-0057** (layout engine / pretext contract — `nexus-layout(-types)`), **RFC-0063** (scene-graph GPU pipeline + theme contract), **RFC-0067** (windowd compositor-service boundary — this RFC is the "promote AA rasterizer, slim windowd" direction extended to the whole UI), RFC-0056 (asset/theme/cursor/text pipeline).
  - DSL consumer: `tasks/TRACK-DSL-V1-DEVX.md` + `TASK-0075` (syntax/IR) — components are the DSL's emit target.
  - Code SSOT candidates: `userspace/ui/widgets/*`, `userspace/ui/shells/desktop`, `userspace/ui/{layout,layout-types,style,theme,theme-tokens,svg,effects}`, `userspace/nexus-gfx`, `windowd/src/{scene_graph.rs,compositor/runtime/*}`.

## Problem — the same UI exists three times, and a fourth is about to be added

Open Nexus OS renders its desktop through **three parallel expressions of the same UI** (see the
inventory for the per-component breakdown):

- **A — declarative:** `ui/widgets/*` + `ui/shells/desktop` produce `LayoutNode` trees (pure
  builders, id-based interaction, host-tested). Clean and DSL-shaped — **but not the live path.**
- **B — scene-graph:** `windowd/scene_graph.rs` is a retained-mode engine (`SceneNode`/
  `RenderPrimitive`, dirty-set, µs invalidation → `nexus-gfx`). **The reactive engine** — only
  partially fed.
- **C — bespoke:** `windowd/compositor/runtime/*` (~15k LOC) hand-draws chat/search/settings/
  desktop_layer/greeter per row — **the actually visible pixels** — bypassing A and B.

At the **token** layer the same fracture exists **four** ways: authored `.nxtheme.toml`, a
hardcoded typed `BaseTokens` that silently drifts from it, windowd's build-time `ThemeTokens`,
and the handoff CSS contract.

TASK-0073 as drafted (2025-12) would add a **fifth/fourth** parallel structure (`ui/design` +
`ui/kit` folders) and re-derive tokens again. That is the double-structure trap. This RFC defines
the convergence so the design system is built **once**, to Apple/ArkUI fidelity, on one path.

## Decision

### D1 — One reactive path

`Component (LayoutNode) → LayoutEngine (measure/place) → SceneGraph (retained, dirty-set) →
nexus-gfx`. This is the only path from any UI producer (widget, shell, or DSL) to pixels. It
preserves today's µs reactivity because B (the scene graph) stays the engine; components feed it
instead of bypassing it.

### D2 — One component library SSOT (no façade)

The canonical library is the **promoted** `userspace/ui/widgets/`, organized by the 7 handoff
groups (`core/controls/inputs/feedback/navigation/overlays/window`). Every one of the 54
components is a `LayoutNode` builder + typed variant/size/state enums + id-based interaction +
A11y role. **No `ui/design` and no `ui/kit`** — motion/spacing/type/color are tokens (D3), not a
wrapper crate; `ui/shells/desktop` is the reference consumer. Clean separation stays: **`ui/*` =
declarative trees + layout + tokens; `nexus-gfx` = drawing.**

### D3 — One token SSOT, generated typed contract

`.nxtheme.toml` is the single runtime source, extended to carry the full handoff contract
(theme-varying colors + glass materials per theme; theme-invariant type/spacing/radius/shadow/
motion/z scales authored once). `nexus-theme-tokens` typed enums and their `Tokens` impl are
**generated from the toml** (build.rs) — the hardcoded `BaseTokens` is deleted; windowd's
`ThemeTokens` folds into the same bake. Handoff CSS becomes reference + the golden the runtime is
checked against. Details + the `accent`-semantics decision: `token-reconciliation.md`.

### D4 — One glass primitive

The signature liquid-glass surface (backdrop-blur + tint + 1px border + top-shine + drop-shadow)
is **one reusable draw** in `nexus-effects`/`nexus-gfx`, consumed via material tokens. windowd's
boot-hardened glass compositing (frosted blur, RT backdrop) is **promoted into it**, not
re-implemented per component. Everything glass is built on this one primitive.

### D5 — Promote the best, not the incumbent

When collapsing A/B/C per component, evaluate quality and **promote the best implementation,
deleting the loser** — typically A's declarative form **combined with** C's boot-hardened
render/glass/scroll/AA quality. Never keep a structure just because it is at the target location
or because it is currently live. (Same rule as RFC-0067 P5, where windowd's production rasterizer
was promoted into `nexus-gfx` rather than keeping the coarse `cpu_mock`.) The inventory records the
verdict per component.

### D6 — DSL-aligned from the start, testable as Rust

Component signatures (variants/sizes/states as typed enums, id-based target-action) are designed to
be the DSL's emit target 1:1 (a DSL `Button{variant:.glass}` → the `Button` builder), so the DSL
task (0075+) maps onto them without a rebuild. They ship as host-testable `LayoutNode` builders
now — the pattern is already DSL-compatible; no separate DSL runtime is required to test them.

## Convergence waves (each host-first + boot-gated where it touches windowd)

- **W1 — Foundations:** token SSOT (D3) + glass primitive (D4). Unblocks every surface.
- **W2 — core:** GlassButton, GlassCard, GlassToggle, Badge, AppIcon.
- **W3 — controls/inputs:** Segment, Slider, Checkbox, RadioGroup, Stepper, Select, Rating,
  WheelPicker, DatePicker, TextField, SearchBar, TextArea.
- **W4 — overlays + modal manager (TASK-0074):** Modal, ActionSheet, Alert, Popover, Menu,
  ContextMenu, Tooltip, FAB + the userspace modal stack (backdrop, focus trap, ESC) + unified Toast.
- **W5 — navigation/window:** Toolbar, TabBar, List/ListItem, Sidebar/SplitView, Accordion,
  TreeView, Breadcrumbs, Pagination, Avatar, Chip, SubHeader + Window/WindowPane/AppWindow/
  WindowActionBar/WindowButton/WindowControls/Icon.
- **W6 — windowd convergence:** collapse `compositor/runtime/*` (C) onto A→B surface-by-surface
  (chat → search → settings → desktop_layer → greeter), each boot-verified identical, then delete
  the bespoke renderer. This is the RFC-0067 windowd-slimming, realized.
- **W7 — DSL emit + notifications:** DSL emits the component set; implement the 5-surface
  notification routing (Activity Runner / Mitteilungen / Control Center / System-Toast / Background
  Jobs) as behavior, not just visuals.

Waves are follow-on tasks; TASK-0073/0074 own W1–W5 (host-first) and the W6 direction; W6 execution
and W7 are their own boot-gated tasks.

## Non-goals

- No kernel changes.
- No new scripting language in UI nodes (bounded, deterministic — per the DSL track stance).
- Not a big-bang rewrite of windowd — C collapses incrementally, each step boot-verified.
- The MSDF crate (`ui/msdf`) stays parked (too soft at 12–16px; build-time A8 coverage atlas is the choice).

## Invariants

- Determinism (goldens, stable rasterization), boundedness (no unbounded caches — MAX_WINDOWS/atlas
  budget respected), `no_std` where the runtime consumes it, no `unwrap/expect`, no blanket
  `allow(dead_code)`, no company/product names anywhere.
- Every visible state (hover/pressed/focus/disabled) maps to the existing live-input + visible-state
  contracts; A11y roles + min touch-target + contrast lints are part of "done" per component.

## Open questions

- Exact home for the theme-invariant scale table (`base.nxtheme.toml [scale.*]` vs. a dedicated
  `scale.nxtheme.toml`) — decided in W1.
- Whether `LengthToken` splits into `Radius`/`Spacing` tokens or stays one enum — decided in W1.
- oklch→hex conversion authority (author-time table vs. build.rs converter) — decided in W1.
