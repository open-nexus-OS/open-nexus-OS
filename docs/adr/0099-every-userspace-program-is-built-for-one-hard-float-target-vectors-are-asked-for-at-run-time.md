<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0099: Every userspace program is built for one hard-float target — the vector extension is asked for at run time, never assumed by the build

- Status: Accepted (2026-11-12, TASK-0362 P0 — measured on the board's stock system)
- Date: 2026-11-12
- Links:
  - Tasks: `tasks/TASK-0362-hardware-floating-point-and-vectors-the-kernel-keeps-each-thread-s-fp-and-vector.md` (P1: floating-point state, P2: vector state and the probe, P3: the one target), `tasks/TASK-0367-app-host-paints-on-every-hart-unseen-by-the-dsl-a-pass-planned-once-row-bands-in.md` (P4: the first vector kernels)
  - RFCs: `docs/rfcs/RFC-0118-n-harts-from-the-device-tree-a-64-bit-cpu-set-the-topology-exported.md` (what a program learns about the harts)
  - Related ADRs: `docs/adr/0048-userspace-runtime-integrity-detectors.md` (the standing detectors), `docs/adr/0080-a-process-runs-from-a-sealed-image-text-mapped-from-its-frames-data-copied-no-other-exec-path.md` (the image's link flags)
  - Measurement: `docs/board/measurements/2026-11-12-float-and-vectors/README.md`

## Context

Every userspace program of the image is built for `riscv64imac`: no floating-point
instruction, every `f32` operation a library call. The kernel keeps neither the floating-point
nor the vector registers of a thread, so no program may use them. Both machines we run carry F,
D and the vector extension on every hart.

The two CPU rasters of the image — the row painter of the app surfaces and the SVG raster —
were built three ways, unchanged, and timed on the board's stock system (ns per pixel of the
target):

| Workload | soft float | hard float | hard float, compiler vectorised |
|---|---|---|---|
| a flat fill | 3.3 | 3.2 | 0.5 |
| translucent rounded cards | 174.5 | 46.9 | 21.3 |
| cards with a drop shadow | 1379.7 | 157.5 | 152.5 |
| line icons | 1420.2 | 115.0 | 112.3 |
| a page (gradient, shadowed cards, icons) | 2263.5 | 226.1 | 222.5 |
| the SVG logo at 1024 px | 313.2 | 54.4 | 52.0 |
| an SVG app icon at 512 px | 14731.1 | 2304.8 | 2511.9 |

- Soft float costs 3.7× to 12.3× on every path that computes a coverage.
- The compiler's vectorisation moves the integer spans (5.4× on the fill, 2.2× on the cards)
  and nothing else; one workload loses 8 %.
- All three builds paint the same pixels: eleven digests equal, on the board and under the
  emulator.
- The vector registers are 32 bytes wide on the board and 16 on the emulator.

## Decision

1. **One userspace target with hard float.** Every userspace program is built for
   `riscv64gc-unknown-none-elf` (F and D, arguments in floating-point registers), with the
   image's flags as they are. The soft-float target leaves the userspace build in the same task
   (TASK-0362 P3); there are never two userspace targets side by side.
2. **The kernel stays integer-only** (`riscv64imac`). It has no floating-point or vector state
   of its own, so a trap saves none; it keeps a thread's registers only for a thread that used
   them (TASK-0362 P1, P2).
3. **What the image requires is checked, not assumed.** The kernel reads every hart's
   instruction set from the device tree (both bindings: the list and the string). A hart
   without F and D is not started; a boot hart without them ends the boot with a line that
   names what is missing.
4. **The vector extension is no build flag of any program.** A vector kernel is a function
   with a scalar reference it equals byte for byte under test, chosen at run time from what the
   kernel reports, and it reads the registers' width instead of assuming one. The kernel
   reports the extension only when every hart that runs threads offers it at the same width —
   a thread migrates.
5. **Float results are part of the determinism contract.** No build flag changes what a float
   operation yields (no relaxed math, no multiply-add the source does not write). The move to
   the hard-float target is gated on the pinned digests staying what they are.

Out of scope: half floats and every other optional extension — they follow rule 4. The bit
extension the image already requires stays as decided
(`docs/board/measurements/2026-10-23-os-build-flags/README.md`).

## Consequences

- **Positive**: the rasters, the text shaper, every decoder and every program built against the
  standard library later run their float work on the hardware without a source change; a
  program is a seventh smaller (the soft-float routines leave it).
- **Positive**: the standard-library target (TASK-0332) and the image share one instruction set
  and one calling convention.
- **Negative**: a hart without F and D cannot run the image. Every application-class RISC-V
  profile requires both; a microcontroller-class hart was never a target.
- **Negative**: a thread that uses floats carries 264 bytes of register state, one that uses
  vectors 32 registers of the hart's width (1 KiB on the board). Both are kept only from first
  use — the reason rule 4 keeps the compiler from spraying vector instructions into every
  program.
- **Churn accepted**: every service is rebuilt and re-proven once (TASK-0362 P3); the build
  recipes and the toolchain file name two targets, one for the kernel and one for userspace.
- **Detectors**: `SELFTEST: f32 soak ok` (ADR-0048) keeps its host-pinned sums and from P3 on
  proves the hardware's float; `fpsoak` and `vsoak` prove the registers across preemption and
  migration.
- **What this does not buy**: the frame. A full window at the page's 226 ns per pixel is
  469 ms. The spans without a per-pixel coverage (3.2 ns scalar, 0.5 ns vectorised) are what
  reaches 16.7 ms — TASK-0367's phases.

## Alternatives considered

- **Stay soft-float and make the algorithms cheaper.** Rejected: the factor applies to every
  float consumer the system gets (decoders, the web view, standard-library programs), and the
  cheaper algorithms are needed on top, not instead.
- **Two targets — soft for services without floats, hard for the UI.** Rejected: two builds of
  every shared library, two calling conventions in one image, and the kernel keeps the
  registers either way.
- **Build the image with the vector extension.** Rejected by the numbers: nothing on the float
  paths, a loss on one; the image would not start on a hart without it, the two machines of
  today already differ in width, and every thread would carry vector state because the
  compiler vectorises copies and fills everywhere.
- **Float in the kernel as well.** Rejected: the kernel computes no float, and using the
  registers there would force saving them on every trap.
