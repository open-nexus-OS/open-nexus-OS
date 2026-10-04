# 2026-10-04 — the picture at 1080p (TASK-0251 P2a step 3a)

After step 2 put windowd's desktop on the board, the operator described what the monitor showed:
the wallpaper "uses the full width but not the height", the vector graphics — "mainly the
icons bottom right" — look chopped, text is soft. This folder holds what was measured to explain
each, what was changed for it, and the gates of the next board cycle.

## Question

Which of the observations are the picture windowd and gpud compose, which the scanout, which
the monitor — and for the ones that are ours, where exactly do they come from?

## Instrument

1. **The CPU executor's picture at 1:1**: QEMU's 2D virtio scanout runs the executor the board's
   controller shows; captured over VNC at 1920x1080 (the one-off `visible-fhd` with
   `GPU_MODE=mmio`, then the `visible-2d` lane), next to the GL path's capture at the same mode.
2. **Row profile**: per 40-row band, mean and maximum luma of the capture — an unpainted band
   is flat.
3. **Zoom**: the bottom-right buttons and the avatar, 6× nearest, both paths.
4. **The code** behind each finding (windowd's damage grid, the icon import, the painter, the
   wallpaper bake), and the wallpaper source's size.
5. **Timed grabs** (`grab-{0.4,2,6}` after `systemui: first frame visible`) to tell a transient
   from the settled picture.
6. **The operator**: the stock system's text at 1080p on the same monitor.

## Results

| observation | measured | cause |
|---|---|---|
| wallpaper not full height | rows ≥ 840 one flat colour (luma 4, max 4); the GL path varies to the bottom | windowd's damage tile grid was still 20×13 tiles of 64 px (1280x832): rows past 832 never count as dirty, the base pass never paints them (an M-L leftover) |
| chopped icons | 1-px stair steps with gaps on BOTH paths | the icon import cut every stroke segment into its own quad (no joins, no caps); the painter filled with one sample per pixel, even-odd: a 1.4 px stroke at 17 px breaks up |
| bar above round buttons | a straight 1-px line wider than the circle's top, both paths | the `inset 0 1px 0` highlight was a straight line inset by 30 % of the radius |
| jagged rings and circles | binary edges everywhere | the whole painter was binary (integer corner test, 32-gon circles, one-sample polygons) |
| soft wallpaper | the source is 1536x1024 (3:2): the 16:9 crop 1536x864 is upscaled 1.25× | the bake's box filter degenerates to nearest-neighbour above 1:1 (stair steps on every ridge) |
| soft text | crisp in the 1:1 capture | the monitor scales 1080p to its native 2560x1440 (EDID); the operator confirmed the stock system's text equally soft — 1080p is the SoC's maximum |
| avatar cut / password pill doubled | only in the grab at +0.4 s; at +2 s and +6 s the greeter is complete and clean | the greeter's entry animation: the CPU path does not run the layer transform overrides (step 3b) |

## What changed (3a)

- The damage tile grid derives from the layout's maximum (30×17 at 1920x1080), with a
  compile-time proof that it covers the layout (the compositor module is OS-only, so its unit
  tests never ran — a finding of its own).
- Line icons are imported as polylines with the set's stroke width and painted as the
  anti-aliased union of capsules: round caps and joins, a shared pixel painted once
  (`ShapeKind::Stroke`).
- The painter anti-aliases every shape: rounded rects (corner coverage from the signed distance,
  straight edges on whole pixels stay hard), rings, ellipses, polygons (four sub-scanlines with
  exact span overlap); the glass reset keeps the uncovered fraction of an edge pixel.
- The inset highlight is the top-facing edge band of the element's own shape — an arc on a
  circle, a line on a flat top.
- The wallpaper bake resamples with a separable Lanczos-3 filter, widened when it shrinks.
- The pixel proof fails on an unpainted band (≥ 64 consecutive flat rows); calibrated on the
  captures: the old 2D capture has 181, every GL capture 0. A `visible-2d` lane judges the CPU
  path's picture in every `test-all`.

## Gates of the board cycle (3a)

1. `scripts/board-test.sh --profile=board-visible`: the 24 rungs, no FAIL.
2. Operator, on the settled greeter: (a) the wallpaper reaches the bottom edge; (b) the three
   bottom-right icons are smooth lines with round ends; (c) no bar above the round buttons;
   (d) ring and pill edges smooth. Known and expected: the first second after the splash shows a
   ghost of the password pill / a cut avatar (the entry animation on the CPU path, step 3b).
3. A higher-resolution wallpaper source (≥ 1920x1080, ideally 3840x2160) is an asset question,
   not a gate: the bake takes any size.

## Verdict (2026-10-04, `board-boot-2026-10-04-picture.txt`)

`[PASS] board-visible: 24 rungs, 1 operator marker(s)`; the operator: "icons and circles look
good now" (gates 2b–2d green). Gate 2a red: "the wallpaper still not full screen". The board ran
the same windowd as the `visible-2d` lane (bundle `c00ad8d6…`), so the 2D scanout reproduces it
— and the timed grabs show WHERE: at the reveal (+0.4 s) the wallpaper reaches row 1080; the
settled greeter (+6 s, with its clock) paints rows ≥ ~840 one flat dark colour. The damage grid
fix made the base pass paint every row; a second 1280x800-era size sits in the desktop-surface
path (the greeter page, app-host's band geometry or windowd's desktop-surface composite) —
recorded in TASK-0251 P2a step 3 "Open findings" together with the other hard-coded sizes, the
tearing at the splash and the reveal, and the long black before the splash; per the operator
not debugged now (the priority is the picture with USB input). The pixel proof needs a settled
snapshot to see this class — its one snapshot sits at the reveal.
