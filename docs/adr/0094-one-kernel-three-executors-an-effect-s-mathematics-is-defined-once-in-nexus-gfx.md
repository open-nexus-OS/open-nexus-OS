<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0094: One kernel, three executors — an effect's mathematics is defined once in nexus-gfx, the shaders and the raster interpret it, the raster is the oracle, performance is a series

- Status: Proposed (2026-11-04, TASK-0351 P5/P7/P8 prove it)
- Date: 2026-11-04
- Links:
  - Tasks: `tasks/TASK-0351-layer-8-on-the-gpu-an-occluded-window-costs-nothing-bindings-grow-a-fullscreen-a.md`
  - RFCs: RFC-0101 (the GPU driver split: the planner in nexus-gfx, the device server executes),
    RFC-0108 (the budgets and the lines a cycle reads), RFC-0033 (the host as the oracle)
  - Related ADRs: ADR-0078 (gpud: one pool), ADR-0093 (tables grow), ADR-0067 (the compositor
    boundary: windowd states the scene, gpud draws it)

## Context

A frame is drawn by three executors because there are three machines: the board's GPU (the
rogue programs `nexus-gfx/src/shader/rogue`, lowered by `tools/nxgpu` from the WGSL in
`resources/gpu/shaders/`), the virtual GPU of the QEMU lanes (the same WGSL lowered to TGSI by
`nxgpu`) and the CPU of the headless lanes and the 2D fallback (`nexus-gfx/src/raster`). The
shaders are already one source. The raster is not: it is a hand-written mirror of the shaders'
mathematics (`raster/blur.rs`: the separable gaussian "the GPU-shader reference"), and it is
already the oracle the board's blur test computes its expected pixels with
(`gpud/backend/gpu/render_blur.rs`). Nothing ties the two: a shader that changes its kernel
leaves the raster behind, and the oracle then proves the wrong picture.

A third copy existed and went dark: `userspace/ui/effects` (nexus-effects — box blur, dual
Kawase blur, nine-slice shadow; integer-only, allocation-free, 21 tests) is used by no product
path since the GPU path landed. The dual filter TASK-0351 P5 was about to derive again for the
shader is in that crate, finished. This is the pattern the operator named on 2026-11-04: an
optimization is made once per executor, the earlier one is lost with its crate, and a frame is
slower than it was with nobody able to say since when — the gates have a bar (`gpud: gpu gate
scene`) but no memory.

The direction of the stack was decided before this: what a frame is and how it is drawn lives
in nexus-gfx (the planner, RFC-0101 §8, the layer wire), a device server is the thin executor
of it — so that the next driver (a second GPU, a display engine, an NPU) is a new executor,
not a new renderer. An effect's mathematics living in each executor contradicts that.

## Decision

**An effect is defined once, as data, in nexus-gfx; every executor interprets that definition;
the raster is the reference the others are judged by; a gate's value is a series.**

- **One kernel.** Each effect's mathematics is a table in nexus-gfx (`frame::kernels`): the
  blur as its chain (the scales, the tap offsets and weights per radius — the dual filter from
  nexus-effects, harvested, not re-derived), the rounded corner as its coverage function, the
  shadow as its falloff. The table is pure and host-tested on its own terms (weights sum to one,
  the deviation from the gaussian reference per radius, the chain's cost in taps per pixel).
- **Three interpreters.** The WGSL reads the kernel through its uniforms (or `nxgpu` emits the
  constants from the table at build time — never a literal typed into a shader); the raster
  reads the same table; a future executor reads it too. An executor may hold a trick of its
  machine (the board's texture levels, the stride texture) — named at the table entry, judged
  against the raster. No tap weight, radius table or coverage formula lives in
  `source/drivers/gpud` or in a shader source.
- **The raster is the oracle.** The golden gate of every cut is the picture: the GPU's readback
  (virgl in QEMU; the board through `gpud/readback.rs`) against the raster of the same scene,
  within the kernel's declared tolerance, per effect and radius — never against expected pixels
  typed into a test. The raster is therefore a product path and a first-class one: it is never
  "the fallback that does not matter".
- **Performance is a series.** Every gate line a board cycle reads (`gpud: gpu gate scene`,
  `gpud: gpu work ns_per_px`, `ux frame` p99, the hops) carries, beside its bar, the value of
  the last recorded cycle (`last=`), and a value worse than the last by more than the declared
  margin is `FAIL` naming that cycle. The cycles' values are a table under
  `docs/board/measurements/` the ledger reads; "why is this slower than before" is answered by a
  cycle number, not by wondering.
- **Harvest, then delete.** A crate whose knowledge is not yet in a kernel table is not deleted;
  nexus-effects goes when its dual Kawase chain and its shadow are in `frame::kernels` and the
  raster runs them. Out of scope: the planner (what is drawn; RFC-0101 §8 stays), the executors'
  pass structure, the DSL's own raster (`nexus_svg`, text) — this ADR is about the effects a
  frame composes with.

## Consequences

- **Positive**: an optimization is written once and reaches the board, QEMU and the CPU path
  together; the host proves the picture before the board sees it; a new driver is an executor
  of the planner and the kernels, which is how a layered graphics stack keeps its drivers thin;
  a regression names the cycle it entered.
- **Negative / accepted cost**: the shaders lose their literals (uniforms or generated
  constants); the raster gains a dual-filter path and a tolerance contract; one lane compares
  readbacks (virgl in QEMU) and is slower than a marker check; the series needs a table the
  ledger reads and a `last=` field on the gate lines.
- **Follow-ups**: TASK-0351 P5 (the dual filter as a kernel table, the raster and the shader
  from it, the golden raster-against-readback), P7 (nexus-effects harvested and deleted, no
  effect mathematics left outside nexus-gfx), P8 (the gate values as a series with `last=`);
  RFC-0101 §8 gains the kernel table as its contract; RFC-0108 §hops gains the `last=` field.

## Alternatives considered

- **Delete the raster, GPU only.** Rejected: the headless lanes and the 2D fallback need it,
  and without it there is no oracle — the board's pixels would be judged by the board.
- **Keep three implementations, add a cross-check test.** Rejected: the test catches the drift
  after the fact; the knowledge still lives three times and dies with the crate it is in.
- **Generate the raster from the WGSL.** Rejected for now: a WGSL interpreter on the host is a
  fourth executor to maintain; the kernel-as-data table is smaller and is what both read.
- **A bar per gate without history.** Rejected by experience: a bar says "too slow", never
  "slower than cycle 61 since cycle 80".

---
