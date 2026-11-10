<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Materials: “Glass” (Resource-Conscious)

This page defines the **translucent (“glass”) material system** for Open Nexus OS UI: the
eight materials of the one token set (RFC-0116), where each may go, and how a material reaches
the compositor.

Performance baseline:

- Follow `docs/dev/ui/foundations/quality/performance-philosophy.md` for the default rules around damage, caching, bounded degrade,
  and "idle should be cheap".

Goal: achieve a modern layered look (panels, the dock, app windows) without requiring
expensive “real glass” rendering.

## Design intent (what “glass” means here)

Our “glass” is a **frosted layer**:

- a blurred snapshot of the content behind the surface (“backdrop”),
- a subtle white/black tint so foreground content stays legible,
- and a small edge highlight so it reads as a distinct layer.

We do **not** aim for physically correct refraction or per-pixel distortion.

## The five rules (measured on the board; RFC-0116 makes them the contract)

On the board's GPU a blurred pixel costs 20–60 ns and a glass layer a scene pass plus two blur
passes whenever its backdrop changes (TASK-0335, cycle 51: nine glass layers in a desktop scene,
three to four of them one window's own; 3.4× overdraw). Where glass goes decides whether a frame
fits 16.7 ms, so these are design rules and cost rules in one:

1. **Glass only on a root surface.** Panels (Control Center, notifications, launcher sheets),
   the dock and the taskbar, app windows, and small standalone pieces blur the wallpaper.
   **Everything that floats** — modals, alerts, menus, popovers, tooltips, toasts, select
   lists, the action bar — is `.material(overlay)`: dense frosted glass (`glassOverlay`, four
   fifths opaque) over a regular blur of what it floats on, the operator's call (2026-11-10:
   a menu shows the blur behind it). **The content layer is opaque** — a window's pane is
   `.material(pane)` — and so is an overlay nested in another surface: it cannot blur there
   and is drawn at full strength. A floating surface is small and short-lived; its blur costs
   a scene pass and two blur passes only while it is open and its backdrop changes.
2. **One blur per stack, never glass on glass.** A window is ONE material layer over the
   desktop. Panels, cards and controls inside a blur surface are `card` (translucent, no blur
   of its own) or opaque — what lies under them is already the blurred base, and a blur of a
   blur is the same frost, only wider. The DSL rejects a blur material inside another (NX0414);
   app-host declares only the root of a stack as a glass region.
3. **Controls are opaque or tinted, not translucent.** A button inherits its surface's material
   and has no blur of its own; a tinted control is `.tint(role)` (a wash at a fixed alpha).
   Hover and press are the **opacity of a fill layer** (`fillHover`, `fillActive`) the
   compositor owns, not pixels the app repaints.
4. **Blur at rest.** A glass is blurred when its backdrop changed and the scene is still. While a
   window is animated, dragged or resized its glass keeps its last blur (or a tint from the
   backdrop's average); one blur when it comes to rest. What nobody can see is not computed.
5. **Reduce transparency is a setting** (below): every material becomes its fill, opaque —
   accessibility, the board's low-power mode and the A/B switch of every frame-cost measurement
   in one.

Corollaries: nested corner radii follow their container (concentric: parent radius − padding,
never below `sm`); a shadow belongs to the window or panel, not to every card in it; the shell's
glass (dock, top-bar panels) and one window glass per window are the only blur chains a desktop
frame normally carries.

## The materials

`.material(m)` in the DSL, `nexus_layout_types::Material` in code. The recipe of each —
`recipe()` in `userspace/ui/theme-tokens/src/design.rs` — names a fill role, a backdrop, an
edge and a shadow:

| Material | Fill | Backdrop | Edge · shadow | Use |
|---|---|---|---|---|
| `panel` | `glassPanel` | regular | `glassEdge` · `shadow4`, lit top edge | a panel on the wallpaper (the `Panel` widget) |
| `window` | `glassWindow` | regular | `glassEdge` · `shadow4`, lit top edge | an app window |
| `dock` | `glassPanel` | thick | `glassEdge` · `shadow3`, lit top edge | the dock, the taskbar, the launcher backdrop |
| `thin` | `glassPanel` | thin | `glassEdge` | a light standalone piece on the wallpaper |
| `ultraThin` | `glassPanel` | ultraThin | `glassEdge` | a small standalone piece on the wallpaper |
| `card` | `glassCard` | — | `glassEdge` | a card, a grouped row or a tile on a blur surface |
| `pane` | `surfacePane` | — (opaque) | — | the pane inside a window |
| `overlay` | `glassOverlay` | regular (a root); — nested, opaque | `glassEdge` · `shadow3`, lit top edge | every floating surface |

The backdrops are theme sections, authored once in `resources/themes/base.nxtheme.toml`
(`[material.ultraThin|thin|regular|thick]`, `type = "backdrop"`): `blurRadiusDp`
8 · 20 · 40 · 64, `saturationPct` 100 · 100 · 180 · 180, `downsampleFactor` (the compositor's
perf knob). The fills and the edge are colour roles; the shadows are `[shadow.*]`.

**`panel` ≠ `pane`.** The two are easy to confuse: `panel` is a blur surface that sits straight
on the wallpaper; a pane sits inside a window's blur, one layer further in, and is opaque
`surfacePane`. Painting a file listing with `panel` would blur inside a blur. Pick by WHAT IS
BEHIND the surface, not by how big it is.

**One recipe, one place.** `nexus_style::Style::material(material, tokens)`
(`userspace/ui/style/src/lib.rs`) composes all of it — the fill, the 1 px `glassEdge` hairline,
the shadow, the lit top edge (`edgeHighlight`, a one-pixel line) and, for a surface on the
wallpaper, the backdrop blur. There is no shine gradient. The DSL's `.material(...)` and the kit
widgets (`Card`, `Banner`, `Toast`, `Avatar`, `Accordion`, the `TextField` pill) call it, so a
page and a hand-written widget cannot render the same material differently; an explicit `.bg`,
border or `.shadow` on a DSL node wins over the recipe.

## How a material reaches the compositor

app-host paints the fill, edge and shadow into its own surface pixels and declares a region per
material box (`region_kind` in `source/services/app-host/src/probe/layers.rs`):

| Material | Region | Glass bucket | Blur |
|---|---|---|---|
| `panel` | glass, a stack's root only | `GLASS_PANEL` (0) | regular, 40 px |
| `window` | glass | `GLASS_WINDOW` (3) | regular, 40 px |
| `dock` | glass | `GLASS_DOCK` (5) | thick, 64 px |
| `thin` / `ultraThin` | glass | `GLASS_THIN` (1) / `GLASS_ULTRA_THIN` (2) | 20 px / 8 px |
| `overlay` | glass as a root; nested, opaque (`MATERIAL_OPAQUE`) at full opacity | `GLASS_PANEL` (0) | regular, 40 px |
| `pane` | opaque (`MATERIAL_OPAQUE`), only at full opacity | — | — |
| `card` | none: painted into the surface | — | — |

The level on the wire (`source/libs/nexus-display-proto/src/client_surface.rs`) is a **blur
bucket**, not the material; windowd reads the bucket's radius from the theme's backdrops
(`region_blur` in `source/services/windowd/src/compositor/runtime/surface_layers_encode.rs`), so
the compositor and the token set cannot disagree on a radius. Id 4 (`GLASS_OVERLAY`) is retired
and never reused: a floating overlay takes the regular bucket. A nested blur declares no region
and blends into its parent's pixels; an opaque `pane` — or a nested `overlay` — drawn at full
opacity is an occluder wherever it sits (a pane occludes the window's blur, a menu inside a
panel the panel's) — faded, it shows what lies below and claims nothing.

The planner cuts what lies under an opaque layer (`nexus_gfx::frame::occlude`) and runs the
opaque program on its straight parts. Rule 4 is the planner's: a glass of a window in motion
keeps its rows (`GlassCache::keep`). Rule 5 is `ui.material = solid` — the theme byte's
`THEME_SOLID`, `SolidTokens` in app-host (the glass roles opaque, every backdrop 0), the window
body unblurred and opaque in windowd. A floating window's page base is `glassWindow` at the
window's frost; a fullscreen or overlay surface's is the opaque `background`.

Rule 3's fill layer is built for hover (RFC-0109): the app draws one 16×16 patch of `fillHover`
into its surface's store once, and a hover is a motion frame — the wash's rectangle, radius and
opacity over the hovered control — that windowd composes as a layer; no app raster, no row copy.
Press (`fillActive`), selection and focus as layer opacity are the follow-on.

## Rendering recipe (cheap glass)

For a glass surface:

1. Capture a **backdrop snapshot** of the scene region behind the surface.
2. Downsample it (`downsampleFactor`) and apply a **separable blur** (fast).
3. Composite:
   - blurred backdrop
   - the material's fill (a translucent role)
   - optional subtle noise (only if deterministic and cheap)
   - the 1 px edge and the lit top edge

Important:

- The blur is **clipped to the surface's own shape**, not to its bounding box.
  The final (vertical) blur pass carries the layer's corner radius and writes
  the rounded-rect SDF coverage as its alpha — the same analytic curve the
  content pass uses — so blur edge and fill edge are one line. A rectangular
  blur under a rounded fill leaves blurred backdrop standing in the corners,
  which is what a pill or a circular icon button shows first and worst.
- Prefer **clip + caching** per surface (panel, window), not per widget.
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
  - keep the fill + edge,
  - reduce blur quality (higher downsample),
  - or temporarily switch to solid materials in low-power mode.

### Idle behavior

If neither the surface nor the background is changing:

- keep the cached backdrop (“frozen glass”),
- and only animate foreground UI (cheap).

## Accessibility & user preference

Provide a system setting:

- **Reduce transparency** / **Solid surfaces**

When enabled:

- disable backdrop blur and translucency (`SolidTokens`: the glass fills opaque, no blur),
- keep the edges and shadows (still layered, but opaque),
- keep all content contrast-safe.

This setting should also be used as a low-power fallback path. The high-contrast theme's
backdrops are flat (0 px) on their own.

## Determinism requirements (goldens)

- The blur algorithm and parameters must be stable and deterministic.
- If noise is used, it must be deterministic (seeded/patterned) and not frame-random.
- Material selection must be token-driven and testable.

## Related

- The token set: RFC-0116 · the vocabulary in the DSL: `docs/dev/dsl/modifiers.md`
- UI performance philosophy: `docs/dev/ui/foundations/quality/performance-philosophy.md`
- Colors/tokens: `docs/dev/ui/foundations/visual/colors.md`, `docs/dev/ui/foundations/visual/theme.md`
- App shell zones (sidebar/control center surfaces): `docs/dev/ui/patterns/app-shell.md`
- Compositor concepts: `docs/dev/ui/foundations/rendering/compositor.md`
