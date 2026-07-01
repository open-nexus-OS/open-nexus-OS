# ADR-0041: Never-black boot — held GPU splash → atomic desktop reveal

- Status: Accepted; the held-splash + atomic reveal + gpud self-tick shipped and boot-verified on
  virgl. The reveal now tracks the real ready-moment (not a fixed cap) and is time-bounded so it
  never hangs. Two follow-ups remain open (see Consequences): the wallpaper (VMO Plane 0) reads
  ready late in some boots — instrumented, root-causing next — and the short pre-logo bootstrap→GL
  black is a separate step.
- Created: 2026-07-01
- Plan: `~/.claude/plans/nested-stargazing-clock.md` (deterministic soft-real-time boot)
- Builds on: ADR-0032 (gpud command ring + pipelined present), ADR-0034 (reactive cursor),
  RFC-0059 (retained-surface display model), RFC-0067 (windowd is a layer-compositor service)
- Related: `deferred-windowd-present-slowdown`, `black-screen-is-2d-3d-dual-not-host`,
  the display-first-boot task track (never-black splash + reactive blocking + QoS)

## Context

The boot read as a sequence of visible states: firmware/bootstrap → **black** → logo → wallpaper →
menu → mouse. Each transition appeared separately, and the total time to a usable desktop was slow
and **non-deterministic** (first-frame timestamp swung across runs). The product goal is the
opposite: one honest hand-off — a branded loading screen held until the desktop is genuinely ready,
then the complete desktop (wallpaper + shell + cursor) in a single step, with no intermediate black
or staggered reveal.

Two facts shaped the design:

- **The GPU compositor (gpud) already seeds a splash.** In the virgl GL-scanout path gpud seeds a
  wallpaper texture at init and composites the frame incrementally (wallpaper → shell layers →
  cursor). Revealing each piece as it arrived *was* the staggering.
- **windowd's present loop stalls after its first frame** (its in-flight accounting pins and it
  backs off ~0.5 s before force-recovering). gpud blocked on windowd's next command, so anything
  that waits for windowd's presents inherits that stall.

## Decision

### 1. The loading screen is a GPU-composited splash with the product wordmark

The Open Nexus wordmark SVG is rasterized **once at build time** (a gpud `build.rs` using the
`nexus-svg` asset pipeline) into an embedded BGRA bitmap, and composited (premultiplied `OVER`)
centered on the splash gradient. Zero runtime SVG cost and no pressure on gpud's non-freeing bump
heap. This replaces "bare colour, then logo" with a branded loading screen from the first GL scanout.

### 2. Atomic reveal — hold the pure splash until the desktop is composable, then reveal in one frame

gpud holds **only** the splash (clear + seeded wordmark texture) and withholds the wallpaper upload,
the shell/atlas layers, the icon, and the cursor until a single gate is satisfied:

> the real wallpaper is present in shared-VMO Plane 0 **and** the cursor sprite is uploaded (the
> mouse path is live).

On the reveal frame all of them composite together, so the boot reads splash → complete desktop with
no wallpaper→menu→cursor staggering. Two time fallbacks, measured from the first buildup present,
guarantee the splash is **never** held forever: a short one (wallpaper up, cursor still lagging) and
a **hard cap** (currently 1.2 s) that reveals regardless — so the desktop and its markers always
appear even if a readiness signal is slow or missing. The reveal is latched; every later frame
composites normally.

### 3. gpud self-drives the reveal while holding — it does not block on the stalled compositor

Because windowd stalls its present loop after the first frame, gpud cannot wait for windowd's next
command to re-evaluate the gate. While the splash is held gpud wakes on a frame-paced timeout
(~120 Hz), re-runs the buildup present, and re-checks the gate — so the reveal fires the instant the
desktop is ready rather than at the next (possibly seconds-late) windowd present. Once revealed gpud
reverts to a **blocking, fully reactive** recv (zero busy-poll). The reveal marker records which
condition released it (wallpaper ready / cursor slow / time cap), so a slow boot pins the culprit
directly in the UART timeline.

### 4. Asset-pipeline correctness in `nexus-svg` (prerequisite for real logo assets)

Rasterizing a real editor-exported wordmark surfaced three gaps, now fixed generically (they benefit
every SVG asset, not just the logo):

- **viewBox precedence** — the document's coordinate space comes from `viewBox`, not `width/height`
  (which exported assets set to `100%`); the render target size bounds allocation instead.
- **Real `<g>` group nesting** — groups now push a stack frame carrying their transform/opacity/style
  and compose nested matrices down the tree; previously groups were flattened and their transforms
  discarded (it only worked for flat icons).
- **Tolerant attribute handling** — the standard `version` attribute and namespaced editor-metadata
  attributes are ignored rather than hard-failing the parse.

## Consequences

- **Honest, deterministic hand-off.** No fake early reveal: the splash holds until the desktop is
  real, then appears at once. The reveal tracks the true ready-moment and is bounded (≤ the hard cap).
- **The stall is decoupled, not yet fixed.** gpud's self-tick makes the *reveal* independent of
  windowd's present stall, but the stall itself still delays windowd's own work; after reveal the
  desktop can briefly appear static until windowd's loop recovers. Fixing the stall (windowd's
  in-flight/completion accounting) is the next real speedup — tracked with `deferred-windowd-present-slowdown`.
- **Open: late wallpaper readiness.** In some boots gpud's Plane 0 probe reports the wallpaper empty
  well after windowd should have written it (the wallpaper is a compile-time constant written at
  windowd boot, before the VMO hand-off). The reveal marker now names this case ("TIME CAP — plane0
  still empty") so the next boot log distinguishes a probe/VMO-plumbing bug from genuine late landing.
- **Open: pre-logo black.** The short black between the firmware/bootstrap scanout and the first GL
  logo scanout is the 2D-bootstrap→GL-scanout transition. Painting the bootstrap frame with the same
  gradient+wordmark would close it, but it touches the 2D/3D scanout path (historically a source of
  virgl black screens), so it is a separate, isolated step.

## Alternatives considered

- **Reveal incrementally (status quo).** Rejected: it is exactly the staggering the product wants gone.
- **Reveal on a fixed timer.** Rejected: dishonest (shows a half-built desktop) or slow (over-waits).
  The gate + self-tick reveals on the real ready-moment, with the timer only as a never-hang backstop.
- **Runtime SVG rasterization in gpud.** Rejected: gpud has no SVG rasterizer and a non-freeing bump
  heap; build-time rasterization is zero-cost at runtime.
