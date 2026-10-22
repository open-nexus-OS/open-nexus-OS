<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-21 — the GPU on the vendor system: what a pixel, a layout and a render cost there (TASK-0335 P2, the questions cycle 50 left)

## Question

Cycle 50 on our tree (`../2026-10-20-memory-lifecycle-baseline/README.md`, "cycle 50") read
10–30 ns of GPU time per textured pixel in a desktop frame against 1.2–8.9 ns in our
per-program gate and the vendor baseline's 1.8 ns opaque fill (`../2026-10-10-gpu-stock-baseline/`).
Three things could carry the difference, and only the vendor stack on the same board can price
them: the shader and state of a blended and a rounded layer, a linear (strided) texture layout
like our atlas against the driver's own, and the fixed cost of a render (one pass, one kick).

## Instrument

The vendor system (kernel 6.6.63, `PowerVR B-Series BXE-2-32`, driver build `24.2@6603887`)
over adb as root, read-only; `sweep2.py` (this folder: the baseline's `sweep.py` header, three
new scene groups) run from the volatile `/tmp` and removed afterwards — the operator's rule of
2026-10-10. Off-screen 1920×1080 RGBA8 target, every frame from scratch, `glFinish` per frame,
120 frames after 20 of warm-up, the median taken. Outputs: `vendor-run-1.txt`, `vendor-run-2.txt`.

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | Our blended/rounded programs cost what the vendor's do: the gap is not the shader | the vendor's slope 512² → 1024² (ns a pixel) against our gate's `gpu cost (… ns_px=)` |
| H2 | A linear texture (our atlas's layout) is a slow path on this GPU | `from LINEAR` against `from driver layout` at the same pixels |
| H3 | A render (a pass) has a fixed cost the hardware owns, so our 4–12 passes a frame are the lever | eight renders of 16×16 against one, and eight draws into one target |

## Results

**Q1 — a pixel by program (the slope 512² → 1024², 786 432 pixels more):**

| Program | vendor 512² | vendor 1024² | vendor slope | ours (`gpu cost`, cycle 50) | ours ÷ vendor |
|---|---|---|---|---|---|
| opaque textured | 1.07 ms | 2.52 ms | **1.85 ns/px** | 1.21 ns/px | 0.65 |
| straight blended (texture × alpha) | 1.43 ms | 3.93 ms | **3.18 ns/px** | 4.89 ns/px | 1.5 |
| rounded blended (SDF corners) | 1.64 ms | 4.57 ms | **3.72 ns/px** | 8.85 ns/px | 2.4 |
| 21-tap blur (baseline, full res) | — | — | 20.6 ns/px | 48.8 ns/px (quarter res) | 2.4 |

A blended full-screen layer costs the vendor 6.6 ms of GPU time: this GPU affords about two of
them in a 60 Hz frame. **H1 partly: the shader explains 1.5–2.4×, not the 10–30 ns a pixel of
cycle 50's frames.**

**Q2 — the layout (full screen, 2 073 600 pixels):**

| Source | opaque | blended |
|---|---|---|
| a linear dma-buf (stride 7680) | 4.19 ms | 6.76 ms |
| the driver's own layout | 4.40 ms | 7.19 ms |

**H2 refuted: a strided texture costs nothing extra** — our atlas's layout is not the problem.

**Q3 — a render:**

| Frame | median | GPU interrupts |
|---|---|---|
| one render of 16×16 | 0.48 ms | 2 |
| eight renders of 16×16, eight targets | 4.13 ms | 21 |
| eight draws of 16×16, one target | 0.92 ms | 2 |

**H3 confirmed: a render costs 0.52 ms fixed on this GPU, a draw 0.06 ms.** Our frames run 4–12
passes (one per glass write and three per re-blur chain): 2–6 ms of fixed cost before a pixel,
and each pass waits for the one two back. The vendor's compositors draw a frame in one or two
renders.

## Verdict

The frame budget on this GPU is: ~0.5 ms per render, ~1.9 ns per opaque pixel, ~3.2 ns per
blended pixel, 20 ns per blur tap-row — so a 60 Hz frame with glass is two full-screen blended
layers OR a few renders, never both. The order of TASK-0335's cuts follows: (1) **the passes:
one scene below for every glass written, one blur pass per radius and direction, the slot — three
to four renders a frame instead of 4–12** (P3, pulled first); (2) **the pixels: occluded surfaces
not drawn, opaque windows through the opaque program** (P2 — 1.2 instead of 4.9 ns a pixel on
most of the screen); (3) the programs: rounded 8.9 → ~4, the blur 49 → ~21 (the compiler's,
later). The atlas stays linear.

## Cycle 51 (board, 2026-10-21; `cycle51-lines.txt`) — P3 on the board, the stack, the three window paths

**51a (`board--2026-10-21T09-40-10`):** gpud faulted at the first present (`[USER-PF] STORE`
6 KiB below its 64 KiB stack, after `chain G2 parse ok`), `init: service exit name=gpud
reason=fault`, the screen stayed on the boot pattern. P3's board carries every write's chain
(7.4 KiB, `size_of::<Board>()`) on the frame path; the 64 KiB declared in cycle 36 had no
margin left. gpud declares 32 pages now (the manifest).

**51 (`board--2026-10-21T09-55-28`), the operator's series without tiling and screenshot:**

| window | avg | max | passes | draws | area_px | glass_w / 32 | wait | `gpu work` slot / scene / blur px | ns/px |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 33 ms | 109 | 4.7 | 30 | 1.58 M | 43 | 31 ms | 3.24 M / 0.23 M / 11 k | 8 |
| 2 | 39 ms | 128 | 4.5 | 24 | 1.04 M | 47 | 38 ms | 2.61 M / 0.46 M / 15 k | 12 |
| 3 | 64 ms | 257 | 3.4 | 19 | 1.14 M | 26 | 62 ms | 3.88 M / 1.63 M / 57 k | 11 |
| 4 | 81 ms | 313 | 4.7 | 44 | 1.14 M | 40 | 79 ms | 4.02 M / 1.18 M / 41 k | 15 |
| 5 | 77 ms | 112 | 12.8 | 106 | 1.44 M | 126 | 74 ms | 3.34 M / 0.40 M / 24 k | 19 |
| 6 | 58 ms | 149 | 8.5 | 75 | 1.07 M | 81 | 54 ms | 2.55 M / 0.23 M / 12 k | 19 |
| 7 | 68 ms | 169 | 9.4 | 103 | 1.16 M | 98 | 64 ms | 2.93 M / 0.37 M / 16 k | 19 |
| 8 | 71 ms | 185 | 9.8 | 89 | 1.16 M | 94 | 66 ms | 2.93 M / 0.33 M / 18 k | 20 |

Reading: **P3 holds where the writes may share** — windows 1–4 run 3.4–4.7 passes a frame at
1–1.5 writes a frame (cycle 49: 4–13), windows 5–8 (several windows open, 3–4 writes a frame)
8.5–12.8: there a glass lies over another written glass or a non-write layer sits between, so
the chains run one by one (`merge::mergeable` refuses, by design — the next cut is the scene
shared *per group*). **The wait did not move with the passes**: 31 ms at 4.7 passes and 3.5 Mpx,
74 ms at 12.8 passes and 3.8 Mpx — the pixels rule, 8–20 ns each, and `dc present mix` says
where they are: **8.3 layers drawn a frame, 2.46 Mpx of layers + 1.25 Mpx of blits (the
wallpaper under everything) + 0.32 Mpx of glass over a damage of 1.0–1.4 Mpx a frame: 3.4×
overdraw.** The vendor pays 1.85–3.72 ns a pixel once; we pay 4.9–10.8 (straight / rounded /
shadow programs) three times over. So, in order: **P2 occlusion** (the wallpaper and the
layers under an opaque window not drawn — the blits and most of the layers) and the opaque
program (1.2 instead of 4.9 ns), then the programs, then the scene shared per group.

`windowd: loop` printed nothing this cycle: its line needs ≥ 8 presents in a one-second
window and the board presents fewer — the gate hides the slow case it was meant to show; the
next package reports every window with traffic. `ux input` FAIL 0.8–2.3 s (the compositor
parked behind frames of 60–80 ms: ADR-0075), `ux launch` warn 0.5–0.9 s, FAIL 1.1–6.9 s.

**The three window paths (the operator: "stash, settings and calculator behave differently,
sometimes it flickers"):** read from `apphost: window intent` and the band lines of this log —
calculator (inst 3) and chat (inst 4, 7) run `style=titlebar`: windowd draws its legacy
`ShellWindow` title bar over a plain surface; chat's content exceeds the band cap
(`apphost: band too tall, plain-path fallback`, 135 text runs), so it renders and presents
whole frames on content changes. settings (inst 5) and stash (inst 6) run `style=plain
mode=freeform resizable=true`: chromeless, the chrome is window-kit's `WinAppWindow` in the
surface, the band path (the compositor scrolls, row-span damage). Three constructions of a
window on one screen; `docs/dev/ui/patterns/windowing/windows-as-widgets.md` names the one
that stays (the widget's chrome in the surface, windowd draws no frame). Package before P2:
calculator and chat onto window-kit, the windowd title bar retired, the band cap lifted.

## The display planes on the stock tree (2026-10-21, read-only over adb — TASK-0336 P4's P0)

| Question | Instrument | Result |
|---|---|---|
| How many composer planes does the controller give one CRTC? | `/sys/kernel/debug/dri/1/state` | eight: plane-0 (zpos 0, the frame, XR24 1920×1080, `crtc-pos` and `src-pos` named), plane-1 (zpos 1, the pointer, AR24 64×64 at its position), plane-2..7 idle |
| What can a plane do (scaling, per-plane alpha, formats)? | the kernel log (`dpu plane init ok`), debugfs | not printed, no property listing; no `modetest` on the stock image |

Verdict: the plane for a dragged window exists; its register sequence is the next measure — the
stock driver's writes for an overlay plane carrying a buffer (a `kmssink` on plane-2), then the
window plane brought up the way the pointer plane was (TASK-0251 P2a: the stock channel's words,
the rectangle, the blend, the latch). **Trap:** reading the driver's debugfs `dump` file makes it
write every framebuffer to the stock system's `/tmp` (`spacemit_drm_dump_show`: a 1920×1080 XR24
capture of the stock desktop) — never read it; the two files it wrote on 2026-10-21 were removed
at once.

## Cycle 52 (board, 2026-10-21; `cycle52-lines.txt`) — the window as one surface, the materials, occlusion

**52a (`board--2026-10-21T12-05-30`) — no signal at all:** init found no service on the volume
(`init: volume spawn FAIL svc=gpud reason=not-found` for every service, then `fatal
err=missing-elf`). The board image had been built while a stopped QEMU lane was rewriting
`build/system-bundles`; it packed them half-written. `scripts/board-image.sh` now refuses an
incomplete volume (every listed service whole, every app bundle with its payload, the
provenance) — such an image cannot reach the board again.

**52 (`board--2026-10-21T12-17-47`), the operator: "much, much better — still far from instant":**

| | cycle 51 | cycle 52 |
|---|---|---|
| `gpu frame us` avg | 33–81 ms | 19–37 ms (one window 55) |
| passes a frame | 3.4–12.8 | 1.7–5.4 |
| layers drawn a frame (`dc present mix`) | 8.3 | 3.7–5.8 |
| `ux frame` dropped of 32 | 12–32 | 13–26 |
| glass writes a window (`gpud: glass writes`) | — | 8–44 (band 4–40, backdrop 3–14, new ≤ 6, moved 0, full 0) |
| starting surface | — | at every launch's intent (1440×853, 320×270) |

The launches by stage (`apphost: ux launch`): calculator 404–462 ms (render 392–453), the shell
607–816 ms, **stash 2979, settings 4016, chat 3426 ms — the render stage is all of it**
(`mount_ms` 42–132). Chat grew from 868 ms: it is a freeform 1440×853 window now (it was the
320×240 probe size), and the first frame is a CPU raster of every pixel.

What the numbers say next:

1. **The first frame's CPU raster** is the launch. On the host the row raster of settings at
   1440×853 took 27 ms — every box interior went pixel by pixel through `blend`, and every
   material box's 1-px border ring ran two rounded-rect distance fields on every pixel of every
   row. Laid as spans (`blend_span`, `set_span`), the ring computed only where it is not zero,
   the straight edges one coverage a row: **27 → 4.9 ms (settings), 17 → 3.8 (stash), 14 → 1.4
   (chat)**, byte-identical (`span_identity.rs`: every path against the pixel path over sizes,
   radii, ring widths, alphas, rows, a scroll shift and a scissor; the design goldens unchanged).
   Cycle 53's launch line splits `vmo_ms` from the paint.
2. **Full-screen frames from the shell:** 21 window-feed pushes, 25 full-screen flushes
   (`flushed_px=2073600`) — the shell repainted the whole desktop on every window open, close
   and focus. The feed now re-renders the rows its layout diff names, like a tap.
3. **`windowd: ux input` read idleness, not waiting:** every staged sample started its clock,
   a pointer move the controller's pointer plane shows owes no frame, so 0.8–9.1 s were the
   time to the next unrelated present (windowd's loop ran 70–160 times a second with 1–7
   presents). The clock now starts at the first damage queued after a submit — an applied
   input that changed the scene, an app's present, a tick — and stops at the submit.

## Cycle 53 (2026-10-21) — launches fast, frames still throttled: the ring overflows

Launches (first frame): calculator 140 ms, shell 299 ms, chat 601/653 ms (vmo stage 332/404),
settings 591 ms (render 515), stash 635 ms, greeter 727 ms (cycle 52: 3–4 s). QEMU `headless`
and `visible` green on the same tree.

Still red: `gpud: ux frame FAIL` (15–32 of 32 dropped, max 47–136 ms), `windowd: ux input FAIL`
(0.42–5.7 s), windowd presents only 1–7 times a second while damage queues up to 19.6 Mpx/s.

**Root cause (read from the log, not yet fixed):** all 21 present NACKs (`status=0x02`) are
`gpud: chain reason: resource exhausted` — `ring.begin()` found no free plane. The ring has three
planes (scanned, queued, waiting), and windowd's two in-flight presents fill it exactly. windowd
also sends `OP_SET_LAYER_TRANSFORM` fire-and-forget on every spring tick of open/close and on every
drag step, and gpud draws each one into the ring by itself (`present_chain::recompose`), outside
windowd's budget. windowd's next present then finds no plane, is NACKed, and requeues
**full-screen** damage with a full recompose, which slows the loop and lets more ticks pile up.
The NACK bursts sit exactly at chat's open and close transitions.

**Fix (next package):** windowd is the only frame producer. A transform (and a scroll) rides in
the next present (`Command::LayerTransform` is already in every present; the planner damages the
layer's old and new extent), inside the two-present budget and coalesced to the display's blank.
gpud draws no frame of its own. A refused present that never touched the ring does not force a
full recompose.

## Cycle 54 (2026-10-21) — one frame producer (TASK-0338): the overflow is gone

The operator: "slowly usable — not instant yet, but usable". Lines: `cycle54-lines.txt` (cycle 53's:
`cycle53-lines.txt`). Ladder complete (76 rungs, no new red); acks only for what was checked
(desktop, pointer, gpu-desktop).

| | cycle 53 | cycle 54 |
|---|---|---|
| present NACKs (`status=0x02`, ring full) | 21 | **0** |
| `windowd: ux input` (worst wait per 16 frames): median / max | 1 324 / 5 697 ms | **833 / 1 531 ms** |
| `gpud: ux frame`: frames over 16.7 ms / worst | 242 of 320 / 136 ms | 376 of 480 / 145 ms |
| `gpud: gpu frame us` avg per 32-frame window | 24–65 ms | 22–59 ms |
| launch to first frame: calculator, shell, settings, stash, chat | 140, 299, 591, 635, 601–653 ms | 171–195, 404, 521–580, 660, 755–996 ms |

**Verdict:** the ring holds only windowd's presents now (no NACK, no `gpud: FAIL dc present (`);
the cascade of full-screen recomposes is gone and the worst input wait fell 3.7×. Two costs are
left, both measured as "not the ring":

1. **The GPU frame itself**, 22–59 ms on average: the slot pass draws 2.7–3.2 Mpx per frame at
   7–11 ns a pixel (`gpud: gpu work`), the damage itself is 1.0–1.4 Mpx. Frames over budget:
   ~78 % in both cycles. This is the frame-cost lane (overdraw within the damage, the programs).
2. **windowd's wait before a submit**, 0.3–1.5 s at the worst frame of each window, while its loop
   runs 50–180 times a second and presents 1–9 times: the low-cadence seconds (`hz=5–8`) sit at
   the app launches (settings, stash, chat). Suspects, to be MEASURED before cut (no line splits
   the wait yet): windowd copies each app present into the atlas with two syscalls per row
   (`vmo_read` + `vmo_write` in `render_app_surface` — a tall band is thousands of rows),
   rewrites the wallpaper into the retained plane for every content rectangle
   (`write_damage_rect`, one `vmo_write` per row — read on the CPU executor only), and may park on
   a full app queue (ADR-0075). Also: the `apphost: ux launch` line is cut at 95 characters
   before its `budget_ms=` (the probe's marker buffer) — fixed with the next package.

## Cycle 55 (2026-10-21) — the compositor never waits on a client (TASK-0339, ADR-0075)

The operator: "a little better again". Lines: `cycle55-lines.txt`. Ladder complete; the kernel's
`KSELFTEST: waitset writable ok` in its place on the board too.

| | cycle 54 | cycle 55 |
|---|---|---|
| present NACKs / refused presents | 0 / 0 | 0 / 0 |
| a client behind (one-second windows) | — (windowd parked) | 12 of 147, at most 1 frame waiting, 0 dropped |
| `windowd: ux input`: median / max of the window maxima | 833 / 1 531 ms | 1 147 / 9 210 ms (one window `ok`) |
| `gpud: ux frame`: frames over 16.7 ms | 376 of 480 | 334 of 416 |
| launch to first frame (the line now whole, `budget_ms=300`) | 0.17–1.0 s | 0.17–1.3 s (calculator 0.17–0.22 `ok`; chat 1.0–1.3 `FAIL`: `vmo_ms` 707–937) |

**The split the new line gives (`work(…)`, all 147 windows):** windowd rewrote **84 561 wallpaper
rows into the retained plane for 10.2 s** of its own CPU — **120 µs a row** (a scaled copy and one
`vmo_write` per row), 95–220 ms for every window-sized content rectangle; it copied 42 081
app-surface rows into the atlas for 1.6 s — 37 µs a row (two syscalls a row; 853 rows ≈ 30 ms for
a full window). The longest flush: 254 ms; the median of the per-window maxima: 30 ms.

**Verdict:** the parking is gone (12 windows had a client behind; before, each was a park), but
the input's wait is windowd's OWN work: every app or desktop frame blocks the loop 100–250 ms in
the retained-plane rewrite — a plane the GPU executors (the board's, QEMU's virgl) read only while
a capture freezes the screen; the CPU 2D executor is its only steady reader. The 9.2 s window sat
under the shell's modal (full-screen desktop frames, 1 080 rows rewritten each, ~1 present a
second). Next cut, by these numbers: no retained-plane rewrite where the GPU composes; the atlas
copy without two syscalls a row. Also seen: `gpud: gpu frame us max=100000` (a 100 ms cap on a GPU
wait — to be read), chat's `vmo_ms` 0.7–0.9 s at launch.

## Cycle 56 (2026-10-21) — no retained-plane rewrite where the GPU composes (TASK-0340)

Lines: `cycle56-lines.txt`. The operator: hover still re-renders instead of a layer under the
text; scrolling seems to render the background with the content — nothing like the reference
platform's technique or smoothness.

| | cycle 55 | cycle 56 |
|---|---|---|
| `work(base_rows=)` over the session | 84 561 rows, 10.2 s | **0** |
| windowd's slowest flush per second: median / max | 30 / 254 ms | **2.2** / 198 ms |
| `windowd: ux input`: median / max of the window maxima | 1 147 / 9 210 ms | 842 / 2 780 ms |
| `gpud: ux frame`: frames over 16.7 ms | 334 of 416 (80 %) | 430 of 576 (75 %) |
| a screenshot (freeze → thaw) | — | works (`screencapd: saved`) |

**The operator's two observations, checked:**

1. **Scrolling draws the background with the content.** windowd's band composite makes the
   scrolled body layer — and the header and footer slices — GLASS (a backdrop blur, never
   `LAYER_OPAQUE`), although TASK-0337 made the window's content pane opaque. The shared planner
   on the host, one scroll step of a 1440×853 window, the slot area being the frame's damage (the
   ring's buffer age while scrolling): **3.93 Mpx drawn** with the body as glass (wallpaper, the
   body's blurred backdrop, the content, the header re-blurred — its backdrop is the moving body),
   2.75 Mpx without the glass, **1.58 Mpx with the body opaque** (occlusion cuts everything below
   it). At 8–11 ns a pixel: 31–43 ms against 13–17 ms — the difference between ~25 fps and 60 fps.
2. **A hover is a re-render, not a layer.** A hover change goes MOVE → app-host re-rasterizes the
   full-width rows of the old and the new control on the CPU → presents them → windowd copies them
   into the atlas (two syscalls a row) → gpud redraws that strip with every layer below it. No
   layer's opacity changes anywhere.

**The red rung, explained:** `SELFTEST: rss loop FAIL (cycles=10 leaked=876544 vmos=2
kobjects=4)` on the board only (QEMU: `leaked=0`) — the calculator was launched (line 1475) while
the boot loop measured the system-wide record (until line 1577): one launch's channels and
surface read as a leak. The rung now judges the smallest step of its ten cycles (a holder's leak
repeats ten times; a launch elsewhere is one step) with the same tolerance per cycle.

**Next (by these numbers, the reference platforms' technique):** a window's content is ONE opaque
layer under its functional glass bar — the scroll moves its rows, one pass over the viewport;
glass inside a window blurs what is behind the window, not the content that scrolls; hover and
press are state layers whose opacity the compositor changes — no app raster, no copy; then
zero-copy client surfaces.

## Cycle 57 (2026-10-22) — surface layers: the app scrolls and hovers, the compositor composes (TASK-0341)

Lines: `cycle57-lines.txt`. The session: calculator, chat, settings, file manager, chat (40
wheel notches), calculator, then the launcher on the desktop — one window at a time.

| | cycle 56 | cycle 57 |
|---|---|---|
| `windowd: ux input`: median / max of the window maxima | 842 / 2 780 ms | 857 / 2 136 ms |
| `gpud: ux frame`: frames over 16.7 ms | 430 of 576 (75 %) | 412 of 480 (86 %) |
| `gpud: gpu frame us` avg per window: median / max | 30.8 / 54.8 ms | 33.7 / 79.9 ms |
| windowd's slowest flush per second: median / max | 2.2 / 198 ms | 1.1 / 168 ms |
| rows copied into the atlas over the session | 102 234 | 77 284 |

**What held:** every page declared its scroll layers (`apphost: scroll layers (n=1 band_rows=452)`
chat, `(n=2 band_rows=1424)` settings, `(n=1 band_rows=696)` the launcher's list); the desktop
re-created with its store when the launcher opened (no `FAIL atlas`, no `store refused`); hover in
settings and the file manager ran as motion frames — seconds of `moved=2..4 … copy_rows=0`; the
file manager's full presents (`copy_rows=869` = its base and patch) were taps that navigated.

**What broke, and why:** `windowd: surface motion refused (id=17 reason=EmptyWash)` (chat) and
`(id=20 …)` (the desktop, with the launcher open). The hovered control resolved to a box without
area, the app sent a wash of zero size, and windowd refuses a motion WHOLE — its scroll offsets
too. Said once per surface, every later motion of that surface was refused silently: during the
chat's 40 notches windowd composed 3–5 frames a second, all `content` (the band filling) and no
`moved` frame — the transcript's band filled while windowd kept the old offset. Fixed in the
producer: app-host sends no wash for a box without area and checks each motion against the
structure it last sent (the rule windowd applies) — a wash that would be refused waits, the
offsets never do; windowd counts every refused structure and motion on its loop line
(`layers(refused=N)`).

**Not this package's, measured for the next:** the GPU frame is 25–80 ms with glass re-blurred in
most frames (`glass writes (band=…)` — damage below a glass within its reach, mostly the desktop's
glass regions); the input's wait (~0.9 s median) is the app's raster of what a tap changes plus
the row copy (37–48 µs a row) — zero-copy client surfaces and the glass's damage are the levers.

## Cycle 58 (2026-10-22) — the empty-wash fix; window moving broken; the frame rate measured

Lines: `cycle58-lines.txt`. The operator: scrolling slow, typing slow, windows cannot be moved.

| | cycle 57 | cycle 58 |
|---|---|---|
| `windowd: ux input`: median / max of the window maxima | 857 / 2 136 ms | 909 / 2 634 ms |
| `gpud: ux frame`: frames over 16.7 ms | 412 of 480 (86 %) | 323 of 384 (84 %) |
| `gpud: gpu frame us` avg per window: median / max | 33.7 / 79.9 ms | 39.2 / 81.2 ms |
| `layers(refused=)` over the session | (not counted) | **0** in 132 seconds |

**Refusals gone:** no motion was refused — the empty wash is fixed.

**Windows cannot be moved — an old bug the session reached:** `WINDOWD: control win (no window
for id)` after each grab of a window's chrome row. app-host packed its surface id into the 4 bits
`CONTROL_WIN_*` values left for it (`id << 4 | arg`, `id & 0x0F`): the window being dragged was
surface 18 (ids are monotonic and never reused; a launch takes two), so the control named surface 2.
Every window control — move, minimize, close, zoom, tile — fails once ids pass 15. Fixed as the
wire's own docs always said: windowd applies a `CONTROL_WIN_*` verb to the SENDER's own window,
resolved from the kernel sender id (`control_gate::own_window`, host-tested with its rejects); the
value carries only the verb's argument.

**The frame rate, measured for the first time (cycles 55–58 alike):** 32 GPU frames take 9–14
active seconds — **about 3 composed frames a second** while anything moves (the chat scroll: 2
presents a second). A GPU frame costs 25–40 ms (`gpud: gpu work (slot_px=3682236 …
ns_per_px=8)`: 3.7 Mpx a frame, 1.8 screens, at 8 ns a pixel), which alone would allow ~25. windowd
holds two frames in flight and waits for gpud's acks; gpud acks a frame when its flip lands, and
writes a flip only when its blank wake finds the scan inside the blank (~45 of 1 125 lines) — but
gpud blocks on the GPU's fence for most of each frame, so a wake served late defers the flip a
whole frame more. The suspect, not yet the proof: cycle 59 prints the blank wakes and the late ones
on `gpud: dc flips (… wakes= late=)` and each frame's issue → ack time on windowd's loop line
(`acks(n= avg_us= max_us=)`).

## Cycle 59 (2026-10-22) — the window controls by sender; the frame's wait measured

Lines: `cycle59-lines.txt`. The operator: asks for hop markers along the important chains.

- **Fixed and held:** no `control win (no window …)`; a window dragged to the left edge took the
  left half (`windowd: wm tile (zone=left-half id=app0)`); `layers(refused=0)` in all 127 seconds.
- **The frame's wait, measured — the ~3 frames a second explained:** from windowd's send to gpud's
  credit a frame takes **631 ms** (median of the per-second averages; per-second maxima: median
  963 ms, worst 2 839 ms) — while its GPU work is 18–60 ms. And gpud's blank wakes:
  `gpud: dc flips (n=384 … wakes=302883 late=257131)` — **~790 wakes per flip, 85 % outside the
  blank**: an interrupt storm. `INT_MASK_ALL_BUT_VSYNC` is the stock word `0xf212` plus the two
  cfg bits, which leaves six sources live (bits 5–8, 10, 11), while the ack clears the vsync bit
  alone — a raised source the ack never clears keeps the level line up, and gpud is woken again
  the moment it completes the interrupt (the boot's `min_period_us=231`). gpud serves the storm
  instead of its frames; a flip lands only when a wake happens to fall into the blank.
- **Next:** one interrupt a frame (every source but the vsync held back, what the ack sees raised
  cleared), measured by the same two lines; then the hop markers along the input and frame chains.
