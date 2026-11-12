<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-11-12 — what the harts offer and what soft float costs (TASK-0362 P0)

Status: **measured on the board's stock system (three builds of both rasters) and on QEMU; the
userspace baseline is decided from these numbers in ADR-0099.**

## Question

Every userspace program of the image is built without floating-point instructions
(`riscv64imac`), and the kernel keeps neither the floating-point nor the vector registers of a
thread. What do the harts of the two machines offer, and what does that build cost the two CPU
rasters — the row painter of the app surfaces and the SVG raster — against a build that uses
the hardware?

## Instrument

`rasterbench/` in this folder: the two raster crates of the image, unchanged
(`nexus-scene-raster`, `nexus-svg`), as one static program without a C library for the stock
system. `rasterbench/build.sh` builds it three ways with the repo's pinned toolchain and the
image's flags; only the float and vector features differ.

| Build | Target | Says |
|---|---|---|
| `soft` | `riscv64imac`, `+zbb` | the image's userspace today |
| `hard` | `riscv64gc`, `+zbb` | the same code with F and D |
| `hardv` | `riscv64gc`, `+zbb,+v` | F, D and the vector extension, the compiler's own vectorisation — no hand-written kernel |

One line per workload: `rasterbench build= scene= px= min_ns= ns_per_px= first_ns= digest=`.
`min_ns` is the fastest of five runs after one warm-up on CPU 0 (the stock system's governor
at its highest clock, 1.6 GHz), `ns_per_px` is `min_ns` over the pixels of the target (rows ×
width, painted or not — the measure of `apphost: hops motion paint_us`), `digest` the FNV-1a of
the pixels. An SVG workload adds its three stages (`parse_ns`, `plan_ns`, `raster_ns`); the
vector build states the registers' width (`vlenb`).

The row painter's scenes are a band of 240 rows at 1920 px: `fill` (one opaque box),
`cards` (six translucent rounded cards with a hairline and a shine), `shadow` (the cards
opaque, each with a 24 px drop shadow), `circles` (twelve round buttons), `strokes` and
`filled` (24 line icons, 24 filled symbols), `page` (a gradient, the shadowed cards, the
icons). The SVG workloads are the logo (`resources/icons/logos/open-nexus.svg`) and an app icon
(`resources/app-icons/productivity/recipes.svg`) from their text to pixels.

The harts' instruction sets: `riscv,isa`, `riscv,isa-base` and `riscv,isa-extensions` of the
trees, and the kernel's own line `KINIT: isa from hart=`.

## Hypotheses (stated before the run)

| # | Hypothesis | Decision criterion |
|---|---|---|
| H1 | Float is the larger share of both rasters: everything but a flat fill computes a coverage in `f32` per pixel. | `soft` at least 3× `hard` on every scene but `fill` → one hard-float userspace target is the baseline. |
| H2 | The compiler's own vectorisation gains nothing where the time is: the coverage paths are scalar by their shape. | `hardv` within 10 % of `hard` on the float scenes → the vector extension is no build flag of the image; it arrives as kernels with a scalar reference, chosen at run time. |
| H3 | The build does not change the picture. | Every digest equal across the three builds and across the machines; one difference → the baseline waits for its cause. |

## What the harts offer

| Machine | The tree | The kernel's line |
|---|---|---|
| The board, the tree our kernel boots with (`board--2026-11-12T12-31-01`) | `riscv,isa-base = rv64i`, `riscv,isa-extensions = i m a f d c v zicbom zicboz zicntr zicond zicsr zifencei zihintpause zihpm zba zbb zbc zbs sscofpmf sstc svinval svnapot svpbmt`; no `riscv,isa` string | `KINIT: isa from hart=0 base=rv64i f=1 d=1 v=1 zfh=0 zvfh=0 zba=1 zbb=1 zbs=1 zicbom=1 zicboz=1 svpbmt=1 sscofpmf=1` |
| The board, the stock system's tree (all eight harts the same) | `riscv,isa = rv64imafdcv`, and the list above plus `zfh zfhmin zkt zvfh zvfhmin zvkt` | — (the stock kernel adds what `c` and `v` imply: `zca zcd zve32f zve32x zve64f zve64d zve64x`) |
| QEMU `virt`, `-cpu max` | both bindings: `riscv,isa = rv64imafdcbvh_…` and the list; F, D, V, half floats, the bit and the crypto extensions | `KINIT: isa from hart=0 base=rv64i f=1 d=1 v=1 zfh=1 zvfh=1 zba=1 zbb=1 zbs=1 zicbom=1 zicboz=1 svpbmt=1 sscofpmf=1` |

- Both machines carry F, D and the vector extension on every hart.
- The vector registers differ in width: **32 bytes on the board, 16 bytes on the emulator**
  (`rasterbench build=hardv vlenb=`). A program that assumes a width is wrong on one of them.
- The two trees we boot carry different bindings (the board's the list, the emulator's both);
  the tree the stock system runs names four half-float extensions ours does not.

## What soft float costs (the board, `board-soft.txt`, `board-hard.txt`, `board-hardv.txt`)

ns per pixel of the target:

| Scene | soft | hard | hardv | soft / hard | hard / hardv | hard, the workload | Digest, three builds |
|---|---|---|---|---|---|---|---|
| `fill` | 3.3 | 3.2 | 0.5 | 1.0× | 5.43× | 1.5 ms | `b1cbc9f3b8eaa325` = |
| `cards` | 174.5 | 46.9 | 21.3 | 3.7× | 2.20× | 21.6 ms | `4303b247540b2465` = |
| `shadow` | 1379.7 | 157.5 | 152.5 | 8.8× | 1.03× | 72.6 ms | `33362f18d26ca595` = |
| `circles` | 406.0 | 64.4 | 61.7 | 6.3× | 1.04× | 29.7 ms | `c8f7de22ed47b385` = |
| `strokes` | 1420.2 | 115.0 | 112.3 | 12.3× | 1.02× | 53.0 ms | `00bc0187353c8585` = |
| `filled` | 253.2 | 27.7 | 27.5 | 9.1× | 1.01× | 12.8 ms | `523bad3b1b26f94e` = |
| `page` | 2263.5 | 226.1 | 222.5 | 10.0× | 1.02× | 104.2 ms | `d75351079323010b` = |
| `svg-logo-256` | 460.3 | 94.4 | 92.9 | 4.9× | 1.02× | 6.2 ms | `1d915840361fb87d` = |
| `svg-logo-1024` | 313.2 | 54.4 | 52.0 | 5.8× | 1.05× | 57.1 ms | `ab43ec2dffcc34c1` = |
| `svg-icon-64` | 21959.7 | 4251.7 | 4517.3 | 5.2× | 0.94× | 17.4 ms | `b6cf0e3a171e5b56` = |
| `svg-icon-512` | 14731.1 | 2304.8 | 2511.9 | 6.4× | 0.92× | 604.2 ms | `94daecdabb0d720d` = |

A first run of the same three programs gave the same numbers within 3 %. Under the user-mode
emulator the three builds print the same eleven digests as on the board (its times are no
measurement).

The program itself: 648 kB soft, 559 kB hard — the soft-float routines are a seventh of it.

## Verdict

| # | Verdict |
|---|---|
| H1 | **Holds.** 3.7× to 12.3× on every scene that computes a coverage, 4.9× to 6.4× on the SVG raster; a flat fill is integer work and does not move. |
| H2 | **Holds for the float paths, and names where vectors pay.** Within 5 % on shadows, circles, strokes, filled symbols and the logo, 6–8 % slower on the app icon; 5.4× on the flat fill and 2.2× on the translucent cards — the integer spans. |
| H3 | **Holds.** Eleven digests, three builds, two machines: one picture. |

The apps on our system measured 100–355 ns per row pixel (soft,
`board--2026-11-10T14-55-42`); `cards` to `circles` of this table are that kind of row
(175–406 ns soft), and their hard build is 47–64 ns.

## What the numbers say beyond the question

1. **Hard float is necessary and not enough for the frame.** A full window is 2.07 Mpx: at the
   `page` scene's 226 ns that is 469 ms, at `cards`' 47 ns 97 ms, against 16.7 ms. What reaches
   it is the span that does no per-pixel coverage: 3.2 ns scalar, 0.5 ns with vectors (1 ms a
   window). The consumer's phases (TASK-0367: a planned pass, bands on every hart, vector
   kernels for fills, blends and glyphs) start from these rows; its P0 measures the split
   inside a row on our system.
2. **A drop shadow is the most expensive thing a row paints** (`shadow`: 157 ns hard, against
   47 ns for the same six cards without one): two distances with a root per pixel of the
   shadow's band, every time the row is painted.
3. **The app icon costs 40 times the logo per pixel**, and all of it is the raster stage
   (`svg-icon-512`, hard: parse 0.2 ms, plan 0.5 ms, raster 603 ms; `svg-icon-64`: 16.7 ms for
   4 096 pixels). The icon is a gradient under 24 shapes (circles, ellipses, strokes); the logo ten flat paths.
   This is the algorithm's cost per shape and row, not the float's — recorded at TASK-0367 P0.

## Reproduce

```bash
rustup target add riscv64gc-unknown-none-elf        # once, for the pinned toolchain
docs/board/measurements/2026-11-12-float-and-vectors/rasterbench/build.sh /tmp/rasterbench
for b in soft hard hardv; do
  adb push /tmp/rasterbench/rasterbench-$b /tmp/ && adb shell /tmp/rasterbench-$b
done
qemu-riscv64 -cpu max /tmp/rasterbench/rasterbench-hardv   # the digests, not the times
```
