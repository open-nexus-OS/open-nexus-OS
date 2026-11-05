<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0097: windowd moves the declared layers — an app states a transition once, and the compositor computes it for every shown frame

- Status: Accepted (2026-11-05, TASK-0354 P2, P3, P5, P6 — board cycle `board--2026-11-05T13-12-18`)
- Date: 2026-11-04
- Links:
  - Tasks: `tasks/TASK-0354-motion-on-the-compositor-an-app-states-a-transition-once-windowd-computes-it-for.md` (P2: execution + proof)
  - RFCs: `docs/rfcs/RFC-0109-surface-layers-the-app-scrolls-and-hovers-the-compositor-composes.md` (amendment 2026-11-04: motion layers — the wire, the bounds, the composition), `docs/rfcs/RFC-0110-frame-timing-the-shown-time-the-cadence-the-motion-for-its-frame.md` (the motion evaluated for the shown time)
  - Related ADRs: `docs/adr/0076-the-app-scrolls-and-hovers-windowd-composes-the-declared-layers.md` (extended: the app decides what moves, the compositor moves it), `docs/adr/0031-three-layer-animation-architecture.md` (the animation crate stays the physics SSOT), `docs/adr/0075-the-compositor-never-waits-on-a-client.md`

## Context

A shell panel, the launcher and the greeter's content enter with a `.transition`. Until now the
app ran that motion: on every frame pulse app-host advanced the keyframes, re-rastered the rows
the node crossed on the CPU and presented them. Board cycle 2026-11-04T18-14-55 measured it: a
panel's or the launcher's enter cost 160–725 ms of CPU raster per frame
(`apphost: hops motion paint_us=`), the greeter's two enters one frame of 678 ms; the launcher's
first open re-created the desktop surface and painted it whole (a tap answered after 977 ms). A
dragged window, whose motion is windowd's transform, stayed smooth on the same board. The
reference systems split the same way: content is rasterized once into a layer, and only its
transform and opacity change per frame, computed outside the app (a render server, a render
thread). windowd already moves windows that way (TASK-0336/0338), and gpud keeps a moving glass's
last blur.

## Decision

- A `.transition` enter the compositor can draw is a **motion layer** (RFC-0109 §motion layers):
  the app paints the node's subtree once, at rest, into its store, paints the base without it, and
  states the motion once with the structure — the curated token (the curve), the duration, the
  offset and opacity it starts from, and the regions, scroll layers and washes that are part of it.
- **windowd computes the motion** for the predicted shown time of every frame (RFC-0110) and
  draws everything of the layer with its transform id; the present carries the transform
  (`Command::LayerTransform`). The motion starts with the first frame that draws a new id; the
  same id declared again continues. The app is not woken between a motion's start and its end.
- **The animation crate is the one curve**: windowd evaluates the token's easing from the same
  crate the app used (ADR-0031); a token outside the curated set refuses the structure. Since
  2026-11-05 (TASK-0354 P3) the easing IS the design artifacts' curve, evaluated exactly (a
  `cubic-bezier` solver; a spring's overshoot included), and an entrance that grows in (a panel
  from 0.97/0.96, a dialog from 0.94, the toast from 0.96) is drawn by windowd too: the layer
  scales about its centre (`Command::LayerTransform` with a 1/10000 scale and one origin).
- **Ownership and bounds**: only the surface's owner (the kernel's sender id) declares its
  structure; a motion layer's rest, pixels, duration, travel and claims are bounded and checked
  whole (`validate_tree`).
- **Where it applies**: on a surface whose page base is transparent (the desktop: the shell, the
  greeter) — there painting the subtree apart composes to the pixels of painting it in place. An
  effect token, a reduced-motion theme, a full bound or store, or a window with a frosted or
  opaque base keep the app's Tier-2 path.
- **In front, over a backdrop (amendment 2026-11-05)**: the board showed the shell's power alert
  and its toast behind the windows (the layers were drawn with the desktop's base), the alert
  falling back to Tier 2 beside an open panel (two layers could not overlap) and repainting the
  whole screen for its scrim. Now an `.overlay(modal|transient)` root is a layer **in front** —
  windowd draws it, and what is its own, after the windows, the bar and the panels, and the
  pointer there is the shell's; layers **may overlap** (drawn in the app's paint order); a root
  that is one flat fill over the whole surface (a scrim) is the layer's **backdrop** — one patch
  texel windowd spreads below the layer and fades with it, the store holds only the sheet; and the
  base's damage **leaves out the boxes a layer holds** (TASK-0354 P5): a modal opening or closing
  repaints no base row, a change inside a layer paints its store rows only. The system toast sits
  below the 36px bar, so being in front never covers it.
- Out of scope: exits (the DSL's `.transition` is enter-only), window surfaces (their frosted
  base, TASK-0354 P7), interaction micro-motion (a press, a hover-grow — small and bounded, the
  app's).

## Consequences

- **Positive**: a panel, the launcher and the greeter enter at the display's rate with one raster
  per open and none per frame; the power alert opens with one raster of its sheet (no full-screen
  raster for its scrim) and stays in front of every window, as the toast does; a moving glass is not re-blurred; a close paints nothing (the base
  never held the panel); a region or layer that comes or goes damages its rectangle, not the
  whole desktop; a store that outgrows the surface keeps what it drew (the re-create copies the
  rows instead of painting them).
- **Negative / accepted cost**: the store holds each open motion layer's pixels (the desktop's
  surface grows by a panel's or the launcher's rectangle, lazily committed); a fading scrim
  recomposes the screen on the GPU for each frame of its fade; a motion layer's
  boxes are painted in their own pass (a change inside an open panel paints its rows into the
  store as well as the base rows it crosses); windowd keeps a start time per motion layer.
- **Follow-ups**: TASK-0354 P3 (the tokens as springs per the design's motion.css; the staggered
  content reveal), window surfaces as motion layers (their body cut or composed exactly), exits
  (the DSL keeping a leaving node alive).

## Alternatives considered

- **Cache the painter's primitives and keep raster per frame** (the symbol masks, the shadow
  profiles): cheaper rows, still a CPU raster and an app wake per frame — the frame rate stays
  the app's (rejected: the measured cost is the per-frame raster itself).
- **Sample the moving subtree from the base at rest and cut the body around it**: no store, one
  raster — but what lies under the panel (the desktop's icons) and what overlaps its shadow would
  vanish or move with it while it fades in (rejected: wrong pixels).
- **A buffer of its own per layer (a sub-surface)**: exact, but a new allocation, mapping and
  restart path per layer (rejected for now: the store already is the surface's own off-screen).
