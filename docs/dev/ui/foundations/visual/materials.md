<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Materials: “Glass” (Resource-Conscious)

This page defines the recommended **translucent (“glass”) material system** for Open Nexus OS UI.

Performance baseline:

- Follow `docs/dev/ui/foundations/quality/performance-philosophy.md` for the default rules around damage, caching, bounded degrade,
  and "idle should be cheap".

Goal: achieve a modern layered look (floating sidebars, control center, sheets) without requiring
expensive “real glass” rendering.

This is guidance, not a hard ABI: the exact look may evolve, but the **tokens, update policy, and fallbacks**
should remain stable.

## Design intent (what “glass” means here)

Our “glass” is a **frosted layer**:

- a blurred snapshot of the content behind the surface (“backdrop”),
- a subtle white/black tint so foreground content stays legible,
- and a small edge highlight so it reads as a distinct layer.

We do **not** aim for physically correct refraction or per-pixel distortion.

## The five rules (2026-10-21 — measured on the board, the reference platforms' guidelines say the same)

On the board's GPU a blurred pixel costs 20–60 ns and a glass layer a scene pass plus two blur
passes whenever its backdrop changes (TASK-0335, cycle 51: nine glass layers in a desktop scene,
three to four of them one window's own; 3.4× overdraw). Where glass goes decides whether a frame
fits 16.7 ms, so these are design rules and cost rules in one:

1. **Glass only on the functional layer.** Bars, sidebars, docks, the Control Center, menus,
   toasts and transient overlays may be glass. **The content layer is opaque**: a window's content
   pane, lists, tables, documents, media, forms. Never a glass content pane (`glassWindowPane`
   becomes an opaque surface with the pane's tint; its token name stays, its blur goes).
2. **One glass per window, never glass on glass.** A window's chrome (top bar + sidebar) is ONE
   material layer over the desktop. Panels, cards and controls inside it are *tinted* (a solid
   colour with alpha), never blurred again — what lies under them is already the blurred base,
   and a blur of a blur is the same frost, only wider.
3. **Controls are opaque or tinted, not translucent.** A button inherits its bar's material and
   has no blur of its own; primary actions are tinted, not glassed. Hover, press, selection and
   focus are the **opacity of a wash layer** the compositor owns (its colour and radius declared
   once), not pixels the app repaints.
4. **Blur at rest.** A glass is blurred when its backdrop changed and the scene is still. While a
   window is animated, dragged or resized its glass keeps its last blur (or a tint from the
   backdrop's average); one blur when it comes to rest. What nobody can see is not computed.
5. **Reduce transparency is a setting** (below): every material becomes its tint — accessibility,
   the board's low-power mode and the A/B switch of every frame-cost measurement in one.

Corollaries: nested corner radii follow their container (concentric); a shadow belongs to the
window, not to every panel in it; the shell's glass (dock, top bar) and one chrome glass per
window are the only blur chains a desktop frame normally carries.

In the code (built 2026-10-21): the registry (`userspace/dsl/runtime/src/registry.rs`) paints
`windowPane` at full alpha; app-host's layer encoding (`source/services/app-host/src/probe/layers.rs`)
sends a window's pane as an opaque region (`MATERIAL_OPAQUE` → `LAYER_OPAQUE`, only at full
opacity), its cards, subtle rows and bar as no region at all (tint in the surface), the desktop's
regions as glass; the planner cuts what lies under an opaque layer (`nexus_gfx::frame::occlude`)
and runs the opaque program on its straight parts. Rule 4 is the planner's: a glass of a window in
motion keeps its rows (`GlassCache::keep`). Rule 5 is `ui.material = solid` — the theme byte's
`THEME_SOLID`, `SolidTokens` in app-host, the window body unblurred and opaque in windowd. State
as a layer property (rule 3's wash) is not built: with the pane opaque and blur kept at rest a
hover blurs nothing; whether the per-control layer protocol is worth its wire is measured first
(TASK-0337 P3).

## Where we use glass (recommended)

Primary:

- **Floating left sidebar** (desktop/tablet; “new layered sidebar”)
- **Control Center** / quick settings overlay
- **Popovers, menus, toasts** (where appropriate)

Secondary:

- window chrome — top bar + sidebar as the window's ONE glass layer (rule 2)

Never:

- the content pane, lists, tables, documents (rule 1 — the content layer is opaque),
- a glass inside a glass (rule 2),
- large continuous backgrounds behind text-heavy content (readability and cost alike).

## Material tokens

Treat materials as theme tokens (authoring in `*.nxtheme.toml`):

- `material.surface` (opaque)
- `material.glassPanel` — dock, control center, launcher, the lock surface
- `material.glassCard` — cards nested in panels, avatars on a wallpaper
- `material.glassSubtle` — settings rows / list items
- `material.glassWindow` — app-window chrome (two-stop tint gradient)
- `material.glassWindowPane` — the content / properties PANE inside a window
- `material.glassWindowBar` — the floating action-bar pill inside a window
- `material.glassOverlay` — modals, sheets, popovers

**`Panel` ≠ pane.** The two are easy to confuse and were confused: `glassPanel`
is tuned for a surface sitting straight on the WALLPAPER (a dock tile, a control
centre card), so its dark tint is near-black `#121214@.40`. A window PANE sits on
the window's own glass, one layer further in, so the handoff gives it a lighter
two-stop gradient (`#484a54@.48 → #34363e@.32`). Painting a file listing with
`panel` reads as a black slab inside a grey window. Pick by WHAT IS BEHIND the
surface, not by how big it is.

**How a level reaches the compositor.** app-host paints the tint, shine, gradient
and hairline into its own surface pixels and sends windowd only a rect + a
`glass_level`, which windowd reads *solely* to pick a backdrop blur radius. The
level on the wire is therefore a **blur bucket**, not the material:
`glassWindowPane` rides the card bucket (20) and `glassWindowBar` the panel
bucket (40), and adding a level with an existing radius costs no wire change.

Each authors `blurRadiusDp`, `downsampleFactor`, `tintColor`/`tintAlpha`
(+ optional `tintBottom*`), `edgeHighlightColor`/`edgeHighlightAlpha` and
`borderColor`/`borderAlpha`.

**One recipe, one place.** `nexus_style::Style::glass(level, tokens)` composes
all of it — tint fill, the soft `--glass-shine` wash, the `inset 0 1px 0`
top-shine as a real one-pixel line, the 1px hairline, and the backdrop blur.
The DSL's `.material(...)` and every kit widget (`Card`, `Banner`, `Toast`,
`Avatar`, the `TextField` pill) call it, so a page and a hand-written widget
cannot render the same level differently. `edgeHighlightAlpha` feeds BOTH the
crisp inset line (full alpha) and the soft wash (capped at 15%) — see
RFC-0082.

Each glass material defines:

- `blurRadiusDp` (logical blur radius)
- `downsampleFactor` (e.g. 2, 4, 8)
- `tintColor` + `tintAlpha` (usually white/black with low opacity)
- `edgeHighlightColor` + `edgeHighlightAlpha`
- `borderColor` + `borderAlpha` (optional)

## Rendering recipe (cheap glass)

For a glass surface:

1. Capture a **backdrop snapshot** of the scene region behind the surface.
2. Downsample it (`downsampleFactor`) and apply a **separable blur** (fast).
3. Composite:
   - blurred backdrop
   - tint overlay (white/black, low opacity)
   - optional subtle noise (only if deterministic and cheap)
   - 1–2px edge highlight / inner stroke

Important:

- The blur is **clipped to the surface's own shape**, not to its bounding box.
  The final (vertical) blur pass carries the layer's corner radius and writes
  the rounded-rect SDF coverage as its alpha — the same analytic curve the
  content pass uses — so blur edge and fill edge are one line. A rectangular
  blur under a rounded fill leaves blurred backdrop standing in the corners,
  which is what a pill or a circular icon button shows first and worst.
- Prefer **clip + caching** per surface (sidebar/control center), not per widget.
- Avoid filters that are renderer/hardware dependent in ways that break goldens.

## Update policy (live only when it matters)

We want the glass to feel “attached to the world” while moving, but be cheap while idle.

### Backdrop refresh triggers

Refresh the backdrop when the scene is at rest and:

- the **background content** behind it changed (the damage intersects the backdrop region —
  and only that band is blurred again, TASK-0335 P1),
- the window stack changed (a window moved underneath, a new overlay appeared).

**Not** while the glass surface itself is animating, dragged or resized (rule 4): it keeps its
last blur — or a tint from the backdrop's average — and blurs once when it comes to rest. The
earlier policy refreshed during motion; cycle 51 measured that at 12.8 passes a frame.

### Throttling and degradation

If the background is “hot” (e.g. video behind the control center):

- throttle backdrop refresh to a bounded rate (e.g. 30Hz or lower),
- or degrade gracefully:
  - keep tint + edge highlight,
  - reduce blur quality (higher downsample),
  - or temporarily switch to `material.surface` (opaque) in low-power mode.

### Idle behavior

If neither the surface nor the background is changing:

- keep the cached backdrop (“frozen glass”),
- and only animate foreground UI (cheap).

## Accessibility & user preference

Provide a system setting:

- **Reduce transparency** / **Solid surfaces**

When enabled:

- disable backdrop blur and translucency,
- use `material.surface` + border (still layered, but opaque),
- keep all content contrast-safe.

This setting should also be used as a low-power fallback path.

## Determinism requirements (goldens)

- The blur algorithm and parameters must be stable and deterministic.
- If noise is used, it must be deterministic (seeded/patterned) and not frame-random.
- Material selection must be token-driven and testable.

## Related

- UI performance philosophy: `docs/dev/ui/foundations/quality/performance-philosophy.md`
- Colors/tokens: `docs/dev/ui/foundations/visual/colors.md`, `docs/dev/ui/foundations/visual/theme.md`
- App shell zones (sidebar/control center surfaces): `docs/dev/ui/patterns/app-shell.md`
- Compositor concepts: `docs/dev/ui/foundations/rendering/compositor.md`
