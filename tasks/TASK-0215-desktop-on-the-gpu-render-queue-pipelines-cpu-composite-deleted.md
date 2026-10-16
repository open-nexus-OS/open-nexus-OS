---
title: TASK-0215 The desktop on the GPU — Block 4 layer 5 (RFC-0101 Phase 4): the render queue on the board, dynamic geometry, the compositor's ops as GPU pipelines, the CPU composite deleted
status: In Progress (5a since 2026-10-13)
owner: @gpu @compositor
created: 2025-12-27
recut: 2026-10-13 (from "Compositor v2.2a: gpuabst CPU stub + plane planner + async present + colour spaces" — the plane ring and the async present landed as TASK-0216 on the display side; the GPU abstraction is RFC-0101's `GfxDevice`, on hardware since TASK-0169B; what remains of the old draft is this layer's work and is rewritten here to the end state)
depends-on: [TASK-0169B (layer 4: GfxDevice on the device, the first quad), TASK-0216 (layer 2: the plane ring), TASK-0329 (layer 3: the device online)]
follow-up-tasks: [TASK-0290 (M2: uncached mappings, the frame arena), M5 (purgeable blur cache), S3 (the IPC budget the frame clock rides on)]
links:
  - Contract: docs/rfcs/RFC-0101-gpu-driver-split-contexts-vm-batches-fence-and-the-scanout-contract.md (§8c, §8d Phase 4)
  - Order: tasks/IMPLEMENTATION-ORDER.md ("Block 4 in layers", layer 5)
  - Measurement 5a: docs/board/measurements/2026-10-13-gpu-render-queue/README.md
  - The bar: docs/board/measurements/2026-10-10-gpu-stock-baseline/README.md (the stock stack's frame: `glass-quarter` 23.3 ms; fixed cost 0.64 ms; 1.8 ns/px opaque; a blended full-screen layer 2.7 ms then 1.8 ms; a 21-tap blur pass at 1080p 42.8 ms)
  - The CPU composite today: docs/board/measurements/2026-10-10-mgs-baseline/README.md (150–300 ms a present)
  - Layer 4: tasks/TASK-0169B-gpu-draws-gfxdevice-on-the-device-first-quad-offline-shaders.md
  - Method: .claude/skills/driver-bringup/SKILL.md
---

## Context

Layer 4 proved the chain: the open driver's output for this core, bound at its addresses by our
`GfxDevice`, draws into a plane of the ring — a clear at 0.8 ns a pixel and a textured quad
through the transfer queue. The desktop is still the CPU's: `gpud` composites windowd's command
stream (`CompositeLayer`, `BlurBackdrop`, `DropShadow`, `FillSdf*`, `BlitSurface`, …) in
software at 150–300 ms a present (`dc present us … avg=296539 … layer=221001 blur=62760`).

The hardware has set the end form (stock baseline verdict): **redraw what changed, never the
frame**; blur once at reduced size and cache it; opaque content front to back (hidden pixels
cost nothing); what moves alone goes to a controller plane. The stock stack draws the gate
scene (`glass-quarter`) in 23.3 ms; our bar is below that, and a hover frame — one layer's
re-composite over cached blur — below 8.3 ms.

Blending, rounded corners, shadows, gradients and blur are **fragment-program work of the render
queue** on this architecture (the transfer queue has no source-over; layer 4 measured that). The
render queue needs what the open kernel driver builds on the kernel side — the render context
(geometry + fragment queues with the context-switch registers), the free lists of parameter
memory, the render-target dataset — and what the open userspace driver emits per frame: the
geometry job's control stream (the VDM: PDS state, vertex programs, draw calls), the fragment
job's registers (PBE words for the plane, PDS background programs, ISP control) and the data
those programs read (vertex buffers, texture state words, the shaders' constants). Layer 5a
replays a captured render pass unchanged to prove the kernel-side objects and the two-queue
kick; 5b learns which bytes move with the geometry and writes them on the device; 5c brings the
compositor's ops as captured pipelines and the frame; 5d deletes the CPU composite.

**What 5a's capture showed (2026-10-13, `docs/board/measurements/2026-10-13-gpu-render-queue/`):**
the open driver accepts a **linear** colour attachment and a linear sampled texture for this
core (the plane's layout, no copy); one render pass is **three jobs in one submit** — geometry
(40-byte stream: `vdm_ctrl_stream_base`, `tpu_border_colour_table`, `ppp_ctrl`, `te_psg`,
`vdm_context_resume_task0_size`, `view_idx`; flags FIRST|LAST), a partial-render fragment
(384-byte stream, flags PARTIAL_RENDER|DISABLE_PIXELMERGE) and the fragment proper (flags
DISABLE_PIXELMERGE) — on one render context (static state 192 bytes = the 23 context-switch
registers), one render-target dataset (1920×1080, 1 sample, 1 layer, region headers in the
`RGNHDR` heap at `0xdbf0000000`, tail pointers, two macrotile lists) and two free lists (32
pages local, 512 pages global, both to be filled with PM pages by the kernel side). Between one
and three quads exactly four buffers change: the vertex buffer (`0x80010d1000`), the control
stream (`0x80008da000`), the PDS data (`0xda00001000`) and the descriptor words
(`0x800004b000`); the USC code and everything else are identical.

## Three lenses (architecture-review, before code)

- **Scope.** 5a in: `userspace/nexus-gfx/src/backend/gpu/fwif/{render,hwrt}.rs` (geometry and
  fragment command decoders, the render context's static state, the render-target dataset, the
  free list — the kernel side's structures as host-tested encoders with the open kernel driver's
  formulas), `scene` v2 (render jobs, the dataset's arguments, the free lists, the static
  context state; the `RGNHDR` heap), `resources/gpu/scenes/render-quad.scene`, gpud
  `backend/gpu/{render.rs,…}` (the render context, the free lists' PM pages, the dataset, the
  two-queue kick with the kernel's fence sequence; `QueueKind::Render` executes),
  `Stream::Render`; the rung `gpud: gpu render ok (`; proof manifest + board ladder; RFC-0101
  §8d; this ledger; the measurement folder. **Out (5b–5d):** writing geometry, PDS data and
  descriptors on the device; the compositor's pipelines; windowd's protocol; the CPU composite's
  deletion; free-list growth (handled when the firmware asks — a package, not a guess); M2's
  uncached mapping.
- **Invariant.** A render batch names only memory bound in its context (the dataset's and the
  free lists' addresses are resolved in the space before the kick,
  `test_reject_render_dataset_unbound`); the free list's page numbers are the device's own DMA
  pages, never a client's (`test_reject_freelist_pages_outside_device_memory`); a geometry or
  fragment stream is accepted only in the exact feature-gated length with zero padding
  (`test_reject_geom_stream_length`, `test_reject_frag_stream_length`); the static context state
  is exactly the 23 registers (`test_reject_static_state_length`). The scene remains untrusted
  bytes (v1's reject matrix kept).
- **Contract.** RFC-0101 §2 (the render queue, declared since Phase 0, executed here), §4, §5
  (two queues, one timeline: the geometry's point gates the fragment through the firmware's
  fence command), §7, **§8d Phase 4**. SSOTs extended: `fwif`, `scene`, gpud's GPU backend, the
  proof manifest.

## The layer's four steps (each with a board gate)

| # | Step | Gate (board) |
|---|---|---|
| 5a | **The render queue, static** ✅ 2026-10-14 (cycle 4): the captured render pass (clear + one blended textured quad, three jobs) replayed through `GfxDevice` into a plane of the ring; the kernel side's objects built by us | `gpud: gpu render ok (us=6133 samples=4/4 irqs=3 geom=1 frag=2)`; `[PASS] board-visible` 58 rungs |
| 5b | **Dynamic geometry** ✅ 2026-10-14: from the 1-vs-3-quad diff, the device writes the vertex buffer, the control stream's draw records, the PDS data and the descriptor words itself (the open driver's control-stream word layouts as data); N quads at runtime positions and tints, every pass in fresh memory, the render-target data alternating | `gpud: gpu quads ok (n=3 us=13096 samples=3/3 irqs=4)` |
| 5c | **The compositor's ops as pipelines + the frame**: textured layer with opacity and rounded corners, drop shadow, gradient fill, the blur as two passes at quarter size into a cached surface; the command stream → draw list, damage-driven (buffer age), the plane as render target; the frame clock from the vertical blank | `gpud: gpu desktop ok (frame_us= below the stock 23300)`; `board-visual: gpu-desktop` |
| 5d | **The CPU composite deleted on the board**; a hover frame over cached blur; what moves alone on a controller plane | `hover_us` below 8300; the tolerated present-time lines gone; `cpu_frame` not built for the board |

## P0 — Measurement (5a: the host capture) ✅ 2026-10-13

`docs/board/measurements/2026-10-13-gpu-render-queue/`: the program and shaders, both captures
(one and three quads), the three job streams and headers, the dataset's arguments, the free
lists, the static context state, the open driver's own decoded control stream (1 226 lines),
the buffer diff, the hypotheses.

## P1 — Model (host, 5a) ✅ 2026-10-13

Landed as `fwif/render.rs` (geometry command 112 B, fragment command 480 B, `RenderFeatures`,
the stream reader, the static render-context state), `fwif/hwrt.rs` (the dataset's arguments,
the common block by the kernel's formulas, the render-target data 384 B with the cleanup
control at 320), `fwif/freelist.rs` (the kernel's checks and init, the page-number entry),
`scene/render.rs` (the version-2 trailer), `tests/gpu_render_model.rs` (13 tests) and the
render bundle's goldens in `tests/gpu_scene_goldens.rs`; `KccbCmd::kick_with_cleanup` names the
data's cleanup control on every kick, as the open kernel driver does.

- `fwif/render.rs`: `GeomCmd` / `FragCmd` (the open kernel driver's structs with offsets
  asserted; `cmd_geom_frag_shared` with the dataset's firmware address), their stream decoders
  (feature-gated: `VDM_DRAWINDIRECT`, `VDM_OBJECT_LEVEL_LLS`, `TESSELLATION`,
  `REQUIRES_FB_CDC_ZLS_SETUP`, `S7_TOP_INFRASTRUCTURE`, `CLUSTER_GROUPING`, `GPU_MULTICORE_SUPPORT`,
  `ZLS_SUBTILE`, `ISP_ZLS_D24_S8_PACKING_OGL_MODE`; quirk extension streams refused by name), the
  flag conversions (FIRST→FIRSTKICK bit 0, LAST→LASTKICK bit 1, SINGLE_CORE→bit 3;
  DISABLE_PIXELMERGE→bit 15, …), `StaticRenderContextState` (23 × u64 from the 192-byte stream).
- `fwif/hwrt.rs`: `HwrtDataCommon` (the kernel's formulas: tiles of 16×16, macrotiles for
  parameter format 2, `te_screen`, `te_mtile1/2`, `isp_mtile_size`, `screen_pixel_max`,
  `multi_sample_ctl` for one sample, `teaa`), `HwrtData` (addresses from the arguments, the two
  free lists, `rta_ctl`, the cleanup control), `Freelist` (the kernel's init: `ready_pages`,
  `current_stack_top`, `current_dev_addr` aligned to 16) and the page-number list; goldens: the
  captured arguments produce the words the kernel would.
- `scene` v2: job kinds Geometry / FragmentPartial / Fragment with their flags; the dataset's
  arguments; the free lists (address, initial, max, grow, threshold); the static context state;
  the `RGNHDR` heap in the bounds. `render-quad.scene` packed from `render1`; goldens.
- `Stream::Render { geom, frag_pr, frag }` in v2; the mock refuses it by name.

## P2 — First light (board, 5a) ✅ 2026-10-14 (four cycles, `docs/board/measurements/2026-10-13-gpu-render-queue/`)

Cycle 1 red: "no memory for the fragment queue" (read as the backing pool; pool 192 → 512, the
FAIL line gained `backing_free`). Cycle 2 red: the same line with `backing_free=381` — the heap
map's range table (32) was the limit; 128. Cycle 3 red: the context accepted, the geometry job
ran (`geom_word=1`), then a context reset with `fault=0xdbf0000000` — the first region header,
written by the tile accelerator, bound read-only; data buffers read-write now. Cycle 4 green:
`gpud: gpu render ok (us=6133 samples=4/4 irqs=3 geom=1 frag=2 bind_us=8447 shown row=3240)`,
`[PASS] board-visible: 58 rungs`. H1–H3 decided for; H4 against (the partial-render job's update
runs, three interrupts a pass); H5 recorded (6.1 ms with the full-plane clear).

- gpud `backend/gpu/render.rs`: the render context (one firmware object: geometry + fragment
  common contexts + the static state; two client command buffers; a geometry context state with
  the call-stack pointer, a fragment context state sized by `num_raster_pipes × (1 +
  xpu_max_slaves)`), the free lists (PM pages from the device's DMA memory, their page numbers
  written into the captured list buffers, the firmware structs), the dataset (the two firmware
  structs), the kick: geometry CCB `[GEOM (its partial-render fence = the fragment queue's point
  − 1), UPDATE g]`, fragment CCB `[FENCE_PR (geometry ≥ g), FRAG_PR, UPDATE f1]`, `[FRAG, UPDATE
  f2]` — the open kernel driver's sequence for the three jobs; `QueueKind::Render` in `submit`.
- The rung after `gpu quad ok`: bind the render scene (the plane as the attachment), kick,
  wait for f2, read back four samples — background, the quad's white frame blended at alpha
  0.97, two checker colours blended at their alpha — `gpud: gpu render ok (us= samples=4/4
  irqs=)`; FAIL forms name the sample or the firmware fault, and a `FREELIST_GROW` request if
  one arrives.

## P3 — Gate sweep (5a) ✅ 2026-10-14

- The gate shown red: three real device-side reds (cycles 1–3, each naming its cause) and the
  lane refusing cycle 4's capture without the rung (`cycle4-gate-red.txt`).
- `just test-all` run after cycle 4 (result in the commit message); QEMU unchanged.
- Numbers: the pass 6 133 µs (clear of the plane + one 512² blended quad; the stock full-screen
  quad 4.4 ms), the binding of 37 buffers and the render objects 8 447 µs, three interrupts a
  pass; memory: two 8 MiB buffers (the plane adopted, the partial-render scratch allocated),
  544 PM pages (2.1 MiB), the render objects in the firmware heap. Handed on: pack the small
  firmware objects (a page and a table entry each); bindings' access per buffer in the bundle;
  the partial-render job's cost; the firmware's time variance.

## 5b — dynamic geometry

- **P0 ✅ 2026-10-14** (measurement README, "5b"): five captures with the open driver's decoded
  control stream (base, shifted, tinted, two and three quads) and the byte diff of the four
  buffers that move. A pass is six records (60 bytes); a draw adds a seven-word PPP block naming
  a new fragment uniform-data copy, a bare VDM state and an index list with the vertex offset;
  the uniform data's three descriptors point at the tint (push constants), its descriptor and a
  constant word; the fragment PDS programs are byte-identical per draw; the vertex buffer is one
  buffer of 6 N vertices named once by the vertex PDS data.
- **P1 ✅ 2026-10-14**: `nexus_gfx::backend::gpu::csb` — the geometry-stream, PPP and PDS words
  the device writes, packed as the open driver's word-layout data files describe them, each
  checked against the captured bytes — and `gpu::quads` — the emitter: N quads at runtime
  positions and tints → the vertex buffer, the per-draw blocks in the two arenas (64-byte lines
  past the driver's use), the stream's per-draw records; one quad reproduces the captured stream
  byte for byte, a second quad the captured 24-byte growth; refusals for overflow and a malformed
  template.
- **P2 ✅ 2026-10-14** (six board cycles, measurement README "5b"): gpud `render_quads.rs` — after
  `gpu render ok`, the template read from the live arenas, the target rebound at a fresh plane,
  every pass emitted into fresh lines of the stream, vertex, general and PDS buffers, the
  geometry job's `vdm_ctrl_stream_base` patched to the pass's stream, a sample inside each quad
  read back against texel × tint over the clear; five probes, each a line, then
  `gpud: gpu quads ok (n=3 us=13096 samples=3/3 irqs=4)`. The red cycles, each named: the tint's
  descriptor names 16 zero bytes behind the tint, not the tint (cycle 1); the same failure byte
  for byte (cycle 2, so not that); the probes showed even one unchanged quad failed on the second
  pass — the two render-target data alternate frame by frame in the open driver (two-frame
  capture), the device now uses `data[pass % 2]`; the quad centre lies on a checker boundary, the
  bilinear mix there was the right pixel (the expectation moved inside a block); the second draw
  never landed while the control stream the GPU had read was rewritten — **every pass lands in
  memory the GPU has not read** (H10).
- **P3 ✅ 2026-10-14**: the gate shown red — four device-side reds with their causes, and the lane
  refusing the green capture without the rung (`cycle10-gate-red.txt`); the emitter's goldens
  moved to `tests/gpu_quads_model.rs` (structure gate); numbers: a pass 6.1 ms with one quad,
  9.6 ms with two, 13.1 ms with three — about 3.5 ms per 512² blended quad, far above the stock
  stack's 1.8 ms per further blended full-screen layer: 5c's first measurement (SLC bypass on
  every binding, the partial-render job, pixel-merge and tri-merge disabled in the captured state).
  The lane's judge on the green capture was red only on the desk keyboard's rung — the keyboard
  was unplugged (operator, 2026-10-14); the operator accepted the capture as the board proof.

| # | Hypothesis (5b's first cycle) | Decision line |
|---|---|---|
| H7 | One fragment PDS program copy serves every draw | the three centre colours; a wrong tint on draws ≥ 1 = per-draw program state |
| H8 | The index offset addresses the shared vertex buffer as the open driver's second draw does | the quads at their positions |
| H9 | Our blocks in the arenas' unused tails are read as the driver's are | a fault names the address; a wrong colour names the block |

## 5c — the compositor's ops as pipelines and the frame (in progress since 2026-10-14)

5b's number decides the order: a 512² blended quad costs 3.5 ms, a pass 2.6 ms fixed — at that
rate the gate scene could not be met. So 5c begins with the cost of a pass, and only then builds
pipelines on it.

| Package | Content | Gate |
|---|---|---|
| 5c-1 | **The cost of a pass**: measured factor by factor on the board (the GPU-cache policy per buffer as the open driver sets it, pixel merging, the render area limited to the damage with the back end's clip and no background on empty tiles), then the policy that wins made the device's default and the contract's (RFC-0101 §3/§8d) | decided by the cycle: the per-pixel cost of a blended quad near the stock's, a pass's fixed cost limited to its damage tiles |
| 5c-2 | **The pipelines** (ADR-0072): the compositor's ops written in WGSL and compiled through our IR by our backends into the pipeline library (RFC-0101 §7b) — the textured layer with opacity and rounded corners, the SDF fill and gradient, the drop shadow, the blur's two passes at quarter size, glyphs; the emitter generalised from 5b (constants and textures per draw); the QEMU path on the same library | each pipeline's picture on the board against its host formula, each within its instruction-group budget |
| 5c-3 | **The frame**: windowd's command list as GPU draw lists in gpud's display backend — windowd's own GPU context and its pipeline library supplied over IPC (a VMO, RFC-0101 §7b; moved here from 5c-2 P4 to arrive with its consumer) — damage-driven (buffer age, the render area), the blur cached at quarter size, the transient buffers recycled by the timeline | `gpud: gpu desktop ok (frame_us=)` |
| 5c-4 | **The gate scene** and the desktop seen | our frame of the gate scene below the stock 23.3 ms; `board-visual: gpu-desktop` |

### 5c-1 — the cost of a pass

- **P0 ✅ 2026-10-14** (measurement README, "5c-1"): the open driver's per-buffer creation flags
  (only eight buffers bypass the GPU cache; we bypass every page), its pixel-merge choice, its
  render-area mechanism (the back end's clip, the region clip, `process_empty_tiles` off,
  clearing by a quad); the render bundle repacked with the flags.
- **P1 ✅ 2026-10-14**: `csb::{pbe, isp}` (the clip words and the empty-tile bit, checked against
  the captured words), `GeomCmd/FragCmd::to_stream` (the captured streams round-trip),
  `PageTables::rewrite` (flags changed in place), the bundle's `buffer_flags`.
- **P2 ✅ 2026-10-14** (board cycle 11, `cycle11-gpu-lines.txt`): gpud's cost rung — six series,
  each pass a line. The slope is 13.1 ns a pixel with and without the cache bypass (H11
  refuted), pixel merging shifts a pass by 0.3 ms and leaves the slope (H12 refuted for the
  slope), the background on every tile is 2.1 ms of a 2.5 ms pass (H13), the plane cached is
  visible and gains nothing (H14), the fragment stage carries the cost (H15). The rung ran
  gpud's 384 KiB heap dry (page lists per binding, the policy switch's plans; the heap never
  frees) — the boot hung at the GPU's plane.
- **P3 ✅ 2026-10-14**: the policy decided and adopted — the device maps every buffer by the open
  driver's flags (`Device::page_flags`; the policy switch and `PageTables::rewrite` removed),
  pixel merging stays off, a pass is limited to its damage (the back end's clip, the region
  clip, no background on empty tiles). gpud's heap: a buffer's pages walked while the tables are
  written, the buffer table allocated whole. The gate: `gpud: gpu area ok (` (REQUIRED) — two
  CPU sentinels outside the limited pass's rectangle kept, its sample right, cheaper than the
  whole plane; `gpud: gpu heap (used= of=)` after the rungs.
- **P4 ✅ 2026-10-14** (board cycle 12, `cycle12-gpu-lines.txt`): the program — host: the open
  compiler's 68-group program taken apart (the helper-lane feedback makes every application
  shader punch-through unless it declares early fragment tests; every shader reads the tile
  back; fourteen moves zero the sampler's coordinates); four programs as probes
  (`resources/gpu/programs`, digests in `tests/gpu_scene_goldens.rs`). Board: a pixel costs
  0.15 ns + 0.206 ns per instruction group — the chain (116 groups) 24.0 ns, the base (68)
  14.1 ns, without the feedback (67) 13.9 ns, opaque (55) 10.9 ns (H16 confirmed, H17 refuted);
  the area gate green (H18). The boot stopped at the wallpaper: the cost rung's four buffers
  filled gpud's 64 address-space regions and the cursor layer's mapping failed (H19 refuted by
  the region table, not the heap: 87 KiB of 384 KiB used). Fixed: no rung creates a buffer —
  the quads and cost rungs split the bundle's buffers' tails (`QUADS_TAILS`, `COST_TAILS`).
- **P5 ✅ 2026-10-14** (board cycle 13, `cycle13-gpu-lines.txt`, `cycle13-board-test.txt`): the
  desktop after the rungs, the cursor layer up, the session shell visible; the operator's eight
  confirmations on that boot; `[PASS] board-visible: 60 rungs, … no FAIL marker, manifest clean`;
  the measurement lines repeat cycle 12's to a few microseconds. The gate shown red: the same
  capture without the `gpu area ok` line is refused (`rung missing … rung 32 of 60`,
  `cycle13-gate-red.txt`); `just test-all` green.
- **Carried to 5c-2/5c-3**: the compositor's programs written for this core (the open compiler's
  output is 6–8× the stock's fill; which way — our encoder against the published instruction
  description, or the open compiler with its overhead removed — is 5c-2's first decision); gpud's
  64 address-space regions nearly used by the rungs and the display — the frame path needs
  GPU-only and destroyable buffers (the memory lane); the emitter writes into mapped transient
  memory without allocating (the heap never frees).

| # | Hypothesis | Decision line |
|---|---|---|
| H11 | The cache bypass of every page is most of the per-pixel cost | S2's slope far below S1's, samples right |
| H12 | Pixel merging lowers the per-pixel cost and is safe here | S3's slope below S2's, samples right |
| H13 | The fixed cost is the background object on every tile | S4's 16² pass far below S2's |
| H14 | The firmware makes the back end's writes visible with the plane cached | S6's sample right |
| H15 | The fragment stage carries the cost | `frag_us` against `geom_us` |
| H16 | The fragment stage is bound by the program: a pixel costs in proportion to its instruction groups | `chain` (116 groups) against `base` (68) per pixel: about 1.7× = program-bound |
| H17 | The ISP feedback (punch-through) costs per pixel beyond its one group | `eft` (67, translucent) clearly below `base` (68) |
| H18 | A pass limited to its area writes nothing outside its rectangle | `gpud: gpu area ok (` — both sentinels kept |
| H19 | The rungs leave gpud's heap room for the desktop | `gpud: gpu heap` well below `of`; the desktop presents |

### 5c-2 — the pipelines (in progress since 2026-10-15; ADR-0072, RFC-0101 §7b)

5c-1 priced a pixel at 0.15 ns + 0.206 ns per instruction group; the open compiler spends 55–68
groups on a textured quad, the stock's fill about eight. So the programs are ours: WGSL, lowered
by naga (host only) into our IR, compiled by our backend for this core (ADR-0072).

| # | Package | Gate |
|---|---|---|
| P0 | ADR-0072, RFC-0101 §7b (the library's contract and its rejects), RFC-0059's open question closed, this plan | the ADR accepted (operator, 2026-10-15) ✅ |
| P1 | `nexus_gfx::shader::rogue` — the shader core's instruction-group encoding and its disassembler for the compositor's subset, from the open compiler's published instruction description; the setup-program (PDS) encoding | the four captured programs (`resources/gpu/programs`) disassemble and re-encode byte for byte; every encoding a golden |
| P2 | `nexus_gfx::shader::ir` and its interpreter (the reference); the WGSL frontend as a host build tool (naga 28) lowering into it | the compositor's first WGSL programs lowered; the interpreter's pixels equal the CPU rasterizer's |
| P3 | the `rogue` backend: instruction selection, registers, group packing, the epilog parts; the first pipeline — the textured layer, opaque and blended | groups within budget on the host; on the board the picture right and ns a pixel on 5c-1's line; `gpud: gpu pipeline ok (` |
| P4 | the pipeline library: format, build step (an entry per variant — source, entry point, overrides, blend), our setup programs and state words generated beside each program, gpud validates and binds (`test_reject_library_*`); the client's supply over IPC lands with its consumer in 5c-3 | the rejects green; the rungs draw with library pipelines and our setup programs, the captured fragment programs retire to measurement |
| P5 | the ops a live frame uses (measured 2026-10-15): the layer with opacity and rounded corners over straight-alpha surfaces, its shadow, the blur's two passes with saturation; gradient and glyphs are not on the live path (no live sender; clients bake their glyphs) | each picture against its host formula, each budget |
| P6 | the `tgsi` backend; the QEMU path on the library; the hand-written shader text retired | the QEMU lanes green on library programs |

- **P1a ✅ 2026-10-15** (`nexus_gfx::shader::rogue`, `tests/rogue_isa.rs`): the shader core's
  instruction-group encoding — header (main brief and full, bitwise, control), the source parts'
  eight forms, the select byte, the destinations' five forms, the main ALU's, backend's, bitwise
  and control words of the compositor's subset (iterate-and-sample among them), the group order,
  length, decode assist and padding. The four captured programs decode into groups at the open
  compiler's offsets and re-encode byte for byte on the first run (**H20 confirmed**); writes
  refuse a value wider than its field. Found on the way: one control word iterates the varyings
  and samples the texture at once — the open compiler spends an iterate, fourteen moves, a sample
  and a fence on what it does (H21's lever).
- **P1b ✅ 2026-10-15** (`nexus_gfx::shader::rogue::pds`, `tests/rogue_pds.rs`): the setup
  programs' encoding — `dout` (kick, iterate, DMA), `ddmad`, the adds, `halt`, and the constants
  they read (the kick's program offset and temporaries, an iteration's issue fields and
  coefficient destination, a DMA's source and control). The draw's five programs in the render
  bundle decode and re-encode word for word, and their constants carry what the shader program
  reads: the kick names offset 0x7b80 with five units of temporaries; the iteration fills cf0–3
  (W) and cf4–11 (uv, perspective-correct, last issue); the state load DMAs the texture state and
  the tint into sh24–27; the vertex fetch DMAs position and uv by stride times index. A program
  is its data segment, then its code at the next 16 bytes (the open driver's fragment kick
  carries eight data dwords, its code at +0x20 — the size information the draw state names must
  match what we generate). **P1 complete.**

- **P2a ✅ 2026-10-15** (`nexus_gfx::shader::ir`, `tests/shader_ir.rs`): our IR — types (f32,
  f16, i32, u32, bool; one to four lanes), a declared interface (inputs with interpolation,
  outputs, textures and samplers, the push-constant block), SSA ops including the tile read the
  epilogs use; the validator (operands before use, types per op, loads inside the interface, an
  output written once, bounds); the builder with three epilogs (replace, straight-alpha over with
  the source's alpha as the captured pipeline, premultiplied over); the reference interpreter
  (16-bit floats rounded to nearest even). The textured layer under each epilog equals the CPU
  rasterizer's formulas over the bundle's checker; `examples/rogue_dump.rs` prints a program's
  decoded groups.
- **P2b ✅ 2026-10-15** (`tools/nxgpu`, `tests/lower.rs`; `resources/gpu/shaders/layer.wgsl`): the
  WGSL frontend — naga 28 parses and validates (immediates are `var<immediate>` in its
  dialect), the lowering builds our IR (location inputs, immediates, 2D textures and samplers,
  arithmetic with scalar-vector widening, comparisons, select, conversions, the common math
  functions, constructors, swizzles, component access) and links the pipeline's epilog. The
  layer's source lowers to the interface the pipeline binds and runs to the CPU rasterizer's
  pixels under each blend; control flow, discard, stores, calls and built-ins are refused by
  name. Licence (operator, 2026-10-15: "if we may use it, we do"): naga's WGSL parser needs
  `hexf-parse` (CC0-1.0, a public-domain dedication asking nothing of us) — a per-crate exception
  in `config/deny.toml` with its reason; naga's rustc-hash 1.1 a duplicate skip; `cargo deny`
  green, `just dep-gate` green (nothing of it in the OS graph). **P2 complete.**

- **P3a ✅ 2026-10-15** (`nexus_gfx::shader::rogue::asm`, `tests/rogue_asm.rs`): the group
  assembler — instructions by phase, six sources, the selects, the write ports; the phase
  organisation, the repeat and the smallest source and destination forms picked by the open
  compiler's rule. Its helpers rebuild eleven groups of the base program byte for byte (iterate,
  fence, sample, multiply, saturating and negated adds, fused multiply-add, tile read, unpack,
  pack, output). Learned: the second source's bank field has two bits, so a shared register (bank
  4) can only be the first source — the compiler's four tint copies are avoidable.
- **P3b ✅ 2026-10-15** (`nexus_gfx::shader::rogue::reference`, `tests/rogue_reference.rs`; the cost
  rung; board cycle 14, `cycle14-gpu-lines.txt`): five hand-assembled reference programs, the
  program end aligned as the hardware fetches it, assembled by gpud at run time and measured
  beside two compiler controls — every sample right on the first cycle. The opaque textured
  layer costs **1.3 ns a pixel** (3 groups through iterate-and-sample, 5 through iterate and
  sample) against the stock's 1.8 ns and the compiler's 10.9 ns; three opaque layers on each other
  cost one (the opaque pass type: hidden pixels dropped before shading); the premultiplied blend
  through the tile read **4.5 ns** against the compiler's 14.2 ns. H21–H27 confirmed;
  `[PASS] board-visible`, 60 rungs, the operator's eight confirmations.
- **P3c ✅ 2026-10-15** (`nexus_gfx::shader::rogue::codegen`, `tests/rogue_codegen.rs`; `tools/nxgpu`
  `build` and `src/overrides.rs`, `tests/pipelines.rs`; `resources/gpu/pipelines`; gpud's pipeline
  gate): the backend — instruction selection (iterate-and-sample where a sample reads an
  interpolated input, else iterate and sample; multiply, add, subtract and fused multiply-add
  with a shared operand first; the tile read and its unpack; the pack into the pixel output that
  ends the program), registers first-fit with release at last use and in-place reuse by
  lane-stepping ops, the pass type from whether the program reads the tile. The IR's textured
  layer compiles byte for byte to all six references (texel, tinted, premultiplied × fused,
  unfused) with their temporaries and pass type; what the backend lacks is refused by name (the
  straight-alpha epilog, another stage, a constant other than +0.0 and 1.0 — bit for bit, -0.0
  too). The toolchain: `nxgpu build … [--override key=value] --blend … --out` — WGSL's
  pipeline-overridable constants fixed per pipeline as WebGPU keys them (`@id`, else the name), a
  `select` on one folded while lowering, so a variant carries none of the paths it switched off
  (`test_reject_override_values`: unknown or twice-given keys, values outside the type, an
  override without value). Measured before the cycle: the tint multiply is a third of a tinted
  opaque pixel (cycle 14: 1.97 against 1.34 ns), and the opaque layer at full opacity is the
  compositor's commonest draw — so `layer.wgsl` grew the override `tinted`, and the opaque variant
  (`tinted=false`) compiles to the 3-group program cycle 14 measured at 1.30 ns. The artifacts
  `layer-opaque.usc` and `layer-premultiplied.usc` are rebuilt byte for byte by a test and equal
  the references `opaque-itrsmp` and `premultiplied-itrsmp`; gpud's cost rung runs them from the
  code arena as the pipeline gate (`gpud: gpu pipeline ok (`, REQUIRED): both samples right, the
  opaque layer no slower a pixel than the stock's opaque fill (1.8 ns). The probes of P3b retire
  (their programs are the codegen's goldens). The setup programs and state words go to P4, where
  the library carries them beside each program. Board cycle 15 (`cycle15-gpu-lines.txt`):
  **`gpud: gpu pipeline ok (opaque_ns_px=1.30 premultiplied_ns_px=4.04 samples=2/2 bar=1.80)`** —
  the opaque layer to the microsecond as its reference (1 782 µs), the fused blend 0.45 ns a
  pixel below the unfused one; H28 and H29 confirmed; gpud's heap 28 KB lighter (nothing
  assembled at run time); `[PASS] board-visible`, 61 rungs, the operator's eight confirmations.
  **P3 complete.** Carried: the blended layer against the stock's first blended layer (3.1 ns).
- **P4 ✅ 2026-10-15 — the pipeline library.** Measured before cutting it (gpud and windowd read
  through): the GPU rungs run synchronously at boot before gpud serves requests; windowd cannot
  reach the v2 device (one context, held by gpud's own rungs; requests carry no client identity
  yet); loading a program is a CPU copy into an already-bound buffer. So the client's supply over
  IPC would be a path without a consumer today — it moves to 5c-3, where windowd gets its own
  context and draws; the device API it will call is P4's. Packages, host first:
  - **P4a** — `nexus_gfx::shader::library`: the archive format v1 (header, entry table, entries
    of typed sections: name, interface, program, setup programs, state), every size bounded and
    checked before a byte is copied; the six `test_reject_library_*` of RFC-0101 §7b. The three
    placeholders beside it (`module`, `function`, `reflection`) are deleted: the IR's interface
    is the reflection, a variant is an entry point with its overrides, and no SPIR-V module is
    the system's (ADR-0072 §3).
  - **P4b** — `rogue::setup`: our fragment setup programs from a compiled program and its
    interface — the kick, the varying iteration, the state load (its code fixed, its data a
    declared list of DMAs the device fills per draw) — and the PDS size words; the render
    bundle's captured programs are the goldens, word for word where they describe the same.
  - **P4c** — `nxgpu library <manifest>`: the compositor's variants declared once
    (`resources/gpu/pipelines/compositor.toml`), the archive built and its budgets enforced on
    the host; the single `.usc` files retire into it; a test rebuilds it byte for byte.
  - **P4d** — the device: `Device::load_library` (target, version, digests, features; programs
    and setup programs placed in buffers bound in the client's context; the accepted ids
    answered) and draws by pipeline id (`test_reject_draw_unknown_pipeline`); the cost rung loads
    the library as a client and the pipeline gate draws through it — gpud's pipeline table
    retires. Board cycle 16.
  - Not in P4: the vertex pipeline (the captured vertex program and its fetch stay until a
    vertex package; 5a keeps the captured pass as its measured control), the `tgsi` target
    (P6), the IPC supply (5c-3); the device checking an application program's temporaries
    against the registers it names (with the application API).
  - **Host ✅ 2026-10-15.** P4a: the format v1 (`NXGP` — the retired `NXPL` is the payload ops'
    and the `payload-vmo` gate keeps it retired), the six rejects, the malformed entries, every
    truncation and flipped bit of a library parsed without a panic (`tests/shader_library.rs`).
    P4b: the setup generator (`tests/rogue_setup.rs`) — the iteration the bundle's word for word,
    the kick byte for byte once placed at the captured program (its data fixed at eight dwords:
    the open driver's pixel-shader constant, the hardware runs the code at +0x20), the state
    load's texture and tint loads as captured; the open driver's two further loads (its
    descriptor at sh28, a constant at sh0) are its descriptor-set plumbing, which our programs
    never read, so ours carries two; the size words from the generated programs (units from
    the open driver's state description). P4c: `compositor.toml` → `compositor.nxgp` (624
    bytes), each budget held (3/3, 11/11), the `.usc` files retired into it, a test rebuilding
    it byte for byte. P4d: `backend::gpu::pipelines` — the table (`tests/gpu_pipelines.rs`:
    placed, each kick naming its program; `test_reject_draw_unknown_pipeline`, a second library,
    a program that does not decode, one over budget, a setup it does not take, no room, another
    device); the emitter draws only with a pipeline (its goldens: the captures byte for byte but
    for the pipeline's pass type, size word and two loads); gpud's library rung, 5b and the cost
    rung on the library's pipelines; the captured fragment programs remain only in 5a's replay.
  - **Board ✅ cycle 16** (`cycle16-gpu-lines.txt`): `gpud: gpu library ok (bytes=624 entries=2
    refused=0 code_bytes=152 setup_bytes=136 ids=1,2)`; the pipeline gate at 1.29 and 4.03 ns a
    pixel with both samples right (cycle 15: 1.30 and 4.04 through the captured setup); every
    later pass right from setup programs placed once; 5b's three blended quads in **4 972 µs**
    where the open compiler's program took 12 949 µs (each further 512² quad ≈ 830 µs against
    ≈ 3 500); gpud's heap 7 KB lighter. H30–H32 confirmed; `[PASS] board-visible`, 62 rungs,
    the operator's eight confirmations. **P4 complete.**

- **P5 🔄 2026-10-15 — the ops a live frame uses.** Measured first (gpud's present, windowd's
  scene, the virgl shaders, the CPU rasterizer read through): a live desktop frame sends only
  damage blits (`BlitSurface`/`BlitAbsolute`), `BlurBackdrop` (radius 8/20/30/40, saturation
  140 %), `CompositeLayer` (opacity, corner radius 12, shadow 18/5/90 or 24/8, backdrop blur) and
  the software cursor; `DrawTiles`, `FillSdfRoundedRect`, `FillSdfGradient` and `DropShadow` have
  no live sender, and glyphs are baked into client surfaces. The two paths disagree: the board's
  CPU composite draws layers without their corners or opacity, shadows without their knockout
  and glass blurred twice with a box filter, never saturated; the QEMU path (virgl) draws the
  exact rounded-rect distance with a one-pixel band, opacity, a Gaussian (σ = R/2, 2R + 1 taps)
  and the shadow knocked out. **The GPU pipelines take the semantics windowd asks for** — the
  virgl path's, with saturation applied — so the board's picture gains its corners, opacity,
  Gaussian glass and clean shadows. Surfaces are straight alpha: the layer multiplies its texel
  by its own alpha before the premultiplied blend. Packages:
  - **P5a** — the backend grows the ops' arithmetic: the hardware's constant registers (the
    published table: 0.5, powers of two, ln 2, π, … with their negate and floor forms) for
    constants, the single-source unit (reciprocal, inverse square root, exponent, logarithm),
    absolute value, negation and saturation as source and result modifiers, min and max,
    comparisons and select; every encoding a golden from the open compiler's encoder.
  - **P5a ✅ host 2026-10-15** (`rogue::{asm, constants, codegen/arith}`, `tests/rogue_asm.rs`,
    `tests/rogue_codegen.rs`; `tools/nxgpu` `lower/math.rs`): the compiler's arithmetic probe
    (`resources/gpu/programs/frag-ops.usc`, `quad_ops.frag`) — nine of its groups rebuilt byte for
    byte: min and max (two bypasses, a float test, a conditional move: one group), the
    subtraction, reciprocal, inverse square root, logarithm, exponent, saturation (an add of
    constant register 0), a multiply-add reading |x| and constant register 75 (0.5). The
    backend compiles the IR's negation, absolute value, floor, saturation, reciprocal, (inverse)
    square root (√x as the reciprocal of the inverse square root, so √0 is 0), base-2 exponent
    and logarithm, min, max, division and vector construction (the first part's registers kept
    when it dies with room behind it — the straight-alpha epilog now compiles with one move);
    constants from the hardware's constant registers (the published table of 96: 0.5, powers of
    two, ln 2, π, …; a negative one built once from its magnitude); an operand the second source
    cannot read (a shared register) moved once into a temporary, a fused multiply-add's shared
    factor swapped first. A repeated group steps every register operand, constant registers
    too — operations reading one register for every lane take a group a lane. Arithmetic on
    other than 32-bit floats is refused. The frontend lowers `dot`, `length`, `clamp(x, 0, 1)`
    as the free saturation, and a component of a push-constant vector.
  - **P5b** — the layer: opacity and rounded corners (`layer.wgsl` grows its corner rectangle
    and radius, the corners an override), straight-alpha texels; the host interpreter against
    the virgl layer's formula.
  - **P5b ✅ host 2026-10-15**: `layer.wgsl` grows a push block of 48 bytes (tint, size and minus
    half of it, half the size less the radius with the radius — `Quad::layer` writes it, the
    emitter now writes each quad's push constants as far as its pipeline reads them) and the
    overrides `straight` (a client's straight-alpha texel premultiplied) and `rounded` (the
    rounded rectangle's exact distance at the pixel centre, a one-pixel band, the virgl layer's);
    `layer-straight` (15 groups) and `layer-rounded` (34) join the library, `layer-opaque` and
    `layer-premultiplied` unchanged program for program. The interpreter against the virgl
    formula (`tests/lower.rs`): the straight premultiply, and four pixels of a 64x64 layer with
    radius 12 — the far corner 0, a pixel on the arc partial, the centre and an edge's middle 1.
    On the way: an input both sampled and computed with is iterated once more for the arithmetic;
    a swizzle of consecutive lanes (`.zw`) is a register offset; `|` and `&` of overrides fold
    (`||` short-circuits into control flow); a component of a push vector lowers. The frame
    (5c-3) draws a rounded layer's corner squares with `layer-rounded` and the rest with
    `layer-straight` — at radius 12 the distance runs on 576 pixels a window.
  - **P5c ✅ host 2026-10-15**: `shadow.wgsl` — the distance outside the shifted rounded
    rectangle falling off over the blur, squared, knocked out across the unshifted one (the
    virgl shadow), premultiplied; no texture, a push block of 64 bytes (`Quad::shadow`: the quad
    covers the shape grown by the blur and moved by the offset); 43 groups; against the virgl
    formula at five points (under the window 0, below it, mid-ramp, past the blur, at the
    window's edge). The pipeline gate draws all five pipelines with their reads.
  - **Board ✅ cycle 17** (`cycle17-gpu-lines.txt`): `gpud: gpu pipeline ok (opaque_ns_px=1.29
    premultiplied_ns_px=4.03 straight_ns_px=4.87 rounded_ns_px=8.85 shadow_ns_px=10.81
    samples=7/7 bar=1.80)` — every read right on the first cycle; H33–H35 confirmed; `[PASS]
    board-visible`, 62 rungs, the operator's eight confirmations. gpud's heap after the rungs
    120 KB (cycle 16: 75 KB): more passes allocating their emitter buffers, the table decoding
    each program whole at load. **Carried to before 5c-3: the emitter without allocation per pass
    (gpud's heap never frees); the library load without decoding every program into groups.**
  - **P5d ✅ 2026-10-16 (board cycle 18) — the blur.** Two Gaussian passes at a quarter of the size (the virgl
    kernel: σ = R/2, 2R + 1 taps, normalised; at half resolution half the radius, bilinear
    fetches between texel pairs), saturation (windowd asks 140 %), the glass layer's rounded mask
    on the way back. A blur pass samples what the previous pass wrote, so P5d brings what the
    frame needs anyway:
    - **P5d-1 ✅ host** — our own texture descriptors (`backend::gpu::texstate`): the image words of a
      stride texture (format, size, swizzle, address, stride) and the sampler words (filters,
      address modes) at the dwords the programs read (the block's first and ninth), from the open
      driver's state description; the render bundle's captured descriptor is the golden. The
      open driver's descriptor extras (the image's byte size and its query data, a second
      sampler copy) are for shader size queries, which our programs never make: ours leaves
      them zero. The rungs' texture becomes ours. `tests/gpu_texstate.rs`: the captured checker
      descriptor word for word at the dwords the programs read; swizzles past the table refused.
    - **P5d-2 ✅ host** — the frontend evaluates what the pipeline fixes (`nxgpu`'s
      `lower/{fold,flow,expr}.rs`): a constant is folded, not an instruction until it meets a
      computed value, and then made once a program; a function variable holds what was last
      stored, a load reads it where naga emits the load; an `if` on a fixed condition is taken, a
      loop on a fixed counter runs while lowering (at most 1024 iterations) — a tap loop over an
      override radius becomes straight-line code with its weights as constants. The identities
      x + 0, x · 1, fma(a, b, 0) make an unrolled sum's first tap a multiply (WGSL lets an
      implementation ignore a zero's sign); exponentials, logarithms and powers fold through
      `libm`, so the library builds bit for bit alike on every host. A branch on a computed
      value, a switch, arrays are refused by name. `tests/partial_eval.rs` (6); the committed
      library rebuilds byte for byte — none of the five pipelines changed.
    - **P5d-3 ✅ host** — a pipeline's constant block: constants no constant register holds go
      into a block the library carries and the state load DMAs into shared registers. The
      backend gives each value one register of the block (after the push constants, at a
      multiple of four) — a constant is never an instruction now, the negated-register group
      included; the block rides in the entry's state (its count, then its values), its load is
      the state load's third, and the device decodes both strictly (one load of exactly the
      block, a block no load reads refused). The table places the block once beside the setup
      programs in the setup heap; a draw's state load reads it at its PDS-heap address — the
      open driver uploads its compile-time constants the same way, as a DMA source. Against a
      push constant a block constant is a second shared operand (one move, as for two push
      constants); against a computed value — a blur's weight times its tap — it costs what a
      constant register does. `rogue_codegen` (2 new), `rogue_setup` (2), `gpu_pipelines` (1),
      `gpu_quads_model` (2); the committed library unchanged byte for byte.
    - **P5d-4 ✅ host** — `blur.wgsl`: the horizontal pass (from the full-resolution source, its
      fetches at half resolution doing the downsample: one bilinear fetch at the corner of four
      texels a tap, a tap every two pixels, 21 fetches at radius 20) and the vertical pass (two
      taps a fetch placed by their weights, 11 fetches; the saturation from the push block, so
      140 % and 180 % share it); the composite is the layer sampling the blurred rectangle. The
      library's `blur-horizontal-20` (89 groups) and `blur-vertical-20` (70), opaque, their
      weights the constant block (46 and 47 values). The backend makes a broadcast constant in an
      add, multiply or multiply-add a run of the block (the weight four times over): one repeated
      group where a broadcast takes a group a lane — a tap is a fetch, its wait, its multiply-add
      and the next coordinate, four groups. Host goldens (`tools/nxgpu/tests/blur.rs`): the chain
      run by the interpreter, eight bits between the passes, against the virgl path's full-
      resolution gaussian with the CPU path's saturation — three levels at most, a quarter of one
      on average, at 100 % and 140 %. Measured on the way: at radius 8 the half-resolution grid
      deviates by seven levels on sharp edges, so only radius 20 (windowd's) is built. The five
      earlier pipelines unchanged byte for byte; the archive 4748 bytes, `0xaa0d92e858e9f2dc`.
    - **P5d-5 ✅** — gpud (`render_blur`, the pipeline gate's last
      probe): the CPU writes a backdrop into the plane (stripes and ramps) around a 256x192 glass;
      the horizontal pass renders into rows of the same plane below the picture, the vertical
      pass beside it, the rounded layer over the glass — each pass limited to its quad, so
      nothing else of the plane changes, each texture read through our own descriptor (the whole
      plane; then the vertical result's rectangle) written into fresh lines of the general arena
      (`Need::Descriptor`); the reads against the full-resolution formula gpud computes from the
      same backdrop. Measured on the way: the ring lends one plane at a time, and the plane is
      mapped past the GPU's system cache (the open driver's flags for a CPU-visible buffer), so
      the CPU's backdrop is what the texture unit reads. A pass rendering into another plane
      needs the target address per pass (the end-of-tile program's back-end words) — the
      frame's intermediates are the context's GPU-only buffers (5c-3), not ring planes. Board
      hypotheses H36–H40 in the measurement README.
  - **Board ✅ cycle 18** (`cycle18-gpu-lines.txt`): `gpud: gpu pipeline ok (… blur_h_ns_px=38.92
    blur_v_ns_px=23.19 blur_layer_ns_px=8.66 blur_dev=2 samples=12/12 bar=1.80)` — the chain within
    two levels of the full-resolution formula on the first cycle; H36–H39 confirmed (the constant
    block's DMA, the plane through our descriptor, level 0 bilinear at computed coordinates, a pass
    reading what the pass before wrote); `[PASS] board-visible`, 62 rungs, the operator's eight
    confirmations. **H40 refuted:** a sampling pass costs its fetches, about 2 ns each (38.9 and
    23.2 ns a half-resolution pixel against the group line's 18.5 and 14.6) — the two passes are
    17.6 ns a glass pixel (the whole 1080p plane in about 33 ms — the stock stack's bar is 42.8 ms
    for one of its two 21-tap passes), so **the frame blurs only where a glass's backdrop changed and keeps
    the result (5d's blur cache)**; a cheaper kernel (a separate downsample, pairs in both
    directions: 23 fetches instead of 32) is measured against this line first. gpud's heap after
    the rungs 191 KB of 384 KB (cycle 17: 120 KB): **the allocation-free emitter and a library load
    that does not decode every program whole come before 5c-3.**
  - **Before 5c-3 ✅ 2026-10-16 (board cycle 19) — the device allocates nothing per pass.** gpud's heap never
    frees, and the frame emits passes for gpud's whole life; measured on the host with a counting
    allocator: the library load took 108 KB (101 KB of it every program decoded whole to count its
    groups), each pass its emitter's buffers and three stream vectors. Now: an instruction group
    holds its instructions in place (`rogue::Instrs`, at most six), so a group decodes without
    allocating and the table counts a program's groups as it checks them (`count_groups`) —
    the load keeps 7 KB, each pipeline's placement and setup; the emitter makes every block in
    place and hands it to a `Sink` as it goes (gpud's `PassSink` writes it into the bound buffer,
    the goldens record it), the state-load data (`StateData`) and the command streams
    (`StreamBytes`) are fixed buffers. Host proofs: `gpu_quads_model::a_pass_is_emitted_without_allocating`,
    `tests/gpu_device_alloc.rs` (counting, streams, state data: zero allocations; the table's
    share bounded per entry), the captures byte for byte as before. The board reads it in two
    new fields: `gpu library ok (… table_bytes=)`, `gpu heap (… pass_bytes=)` (hypotheses
    H41–H43 in the measurement README). The third-party notices of the two open GPU drivers are
    written (`THIRD_PARTY_NOTICES`).
  - **Board ✅ cycle 19** (`cycle19-gpu-lines.txt`): `gpu library ok (… table_bytes=7039 …)`,
    `gpu heap (used=64666 of=393216 pass_bytes=0)` — H41–H43 confirmed: the table keeps 7 KB
    where the load took 108 KB, no pass takes a byte, every read and cost as in cycle 18; the heap
    after the rungs 65 KB where cycle 18 left 191 KB. `[PASS] board-visible`, 62 rungs, the
    operator's eight confirmations.
  - **P6 ✅ 2026-10-16 — the QEMU path on the library (5c-2 complete).** Measured first (gpud's virgl path, read
    whole): every live draw runs one vertex program (the clip-space position passed through) and
    fragment programs that work from the fragment's position and constants, no texture
    coordinates. Live: the wallpaper blit, the splash's tinted blit, the layer (a rectangle of the
    atlas texture, scaled, corners cut, opacity, straight-alpha over; the icon and the cursor
    premultiplied over through it), its shadow, the glass blur (two full-resolution passes with a
    radius looped at run time, the final pass rounded) and the blur cache's draws; the cursor's
    fallback draws two gradients. Dead: the old GPU executors of BlurBackdrop, FillSdfGradient and
    DropShadow, the composite of the VMO path, stages 2–4 of the build-up. Found: the rounded blur
    and the gradient both claim shader handle 18 (the later creation wins, so rounded glass is
    drawn without its blur); the samplers filter NEAREST (the library's blur needs LINEAR).
    - **P6a ✅ host** — the backend (`shader::tgsi`: our IR into the host renderer's shader text, a value
      a register's components, a lane or swizzle a swizzle, constants immediates, push constants
      the constant buffer; blending the pipeline's state, a tile read refused) and the vertex
      stage (the frontend lowers a vertex entry point, the clip-space position an output at the
      IR's reserved location; `quad.wgsl`, the quad's position as it comes and its 0..1 corner as
      `uv`); host goldens of the text (`tools/nxgpu/tests/tgsi.rs`: the quad's vertex program
      and the opaque layer word for word in the hand-written shaders' dialect; a tile read
      refused; the target's state and text checks in `tgsi`'s unit tests).
    - **P6b ✅ host** — the library for this target (`compositor-virgl.toml` → `compositor-virgl.nxgp`,
      code target `TGSI` v1): the quad's vertex program; the layer variants the path draws (the
      wallpaper's opaque blit, the splash's tinted one, the atlas layer straight or premultiplied,
      rounded — `layer.wgsl` gains an `atlas` override that maps `uv` into a rectangle of the
      texture through the push block, compiled out of the board's variants), the shadow, the blur
      passes at the radii windowd asks (8, 20, 30, 40), the gradient for the cursor's fallback.
      Built: 15 entries, 36 880 bytes (`0x7189c506645a6fb2`), every budget its program's count;
      the board's library rebuilt for the layer's 64-byte block (its programs unchanged, its
      state loads four dwords more; `0x04c43695d21690f9`).
    - **P6c ✅ QEMU** — gpud's virgl path on it (`virgl_pipelines.rs`): the archive parsed for its
      target, every program's text checked, each program one shader object (handle 0x100 + id; a
      long one in 4 000-byte chunks, each its own `SUBMIT_3D` — four blur programs exceed a control
      command's 4 KiB), the pipelines resolved by name, the blur pair nearest windowd's radius. A
      draw (`emit_draw`) binds the quad's vertex program, the pipeline's program and blend, the
      push block as the constant buffer, the texture through the one bilinear sampler, over a
      viewport that is the quad. Every draw is one: the wallpaper (`layer-opaque`, the splash's
      pulse `layer-tinted`), a client's surface and the icon (`layer-atlas`, `Quad::with_texture`
      naming the atlas rectangle), the pointer (`layer-premultiplied`), the shadow (`shadow`), the
      pointer's fallback (`gradient`, `Quad::gradient`), the glass (`backend/glass_blur.rs`: the
      snapshot and its halo, `blur-horizontal-R` into the scratch at half resolution,
      `blur-vertical-R` into the cache rows — or the snapshot texture when none are left —, the
      rounded `layer-atlas` back over the glass; the result one texel wider on every side, so the
      layer's bilinear edge fetches never read a neighbour's rows; a cache hit is that layer draw
      alone, the cache rows half-resolution). The probe's proofs draw library programs: `virgl
      draw ok` (the gradient red over a blue clear), `virgl gradient ok` (red at the top, blue at
      the bottom — `uv` runs down the quad as the programs assume; the marker for a failure is now
      `virgl gradient wrong (flat or upside down)`), `layer composite ok` (`layer-atlas`). Gone:
      the hand-written shader text (`FS_BLIT`, `FS_BLIT_TINT`, `FS_LAYER`, `FS_SDF_GRAD`,
      `FS_SHADOW`, `FS_BLUR`, `FS_BLUR_ROUND`; `virgl_blur_shaders.rs`), the dead executors and the
      VMO blur (`execute_on_gpu`, `submit_virgl_*`, `composite_layer_gpu`, `virgl3d_blur.rs`, the
      separable CPU blur only they selected), the build-up's debug stages and spin demo, the
      display texture of the non-build-up present (4 MB of arena), the handle-18 collision.
      Measured on QEMU (`ci-os-visible`, log `visible--2026-10-16T12-07-14`): every probe proof,
      `gl scanout ok`, `gl flip on`, `layer composite live`, `rt backdrop dst ok`; the pixel proof
      green (desktop mean luma 11.93 against 11.97 before). Rounded glass is frosted now — before,
      the collision ran the gradient's program with the blur's constants (alpha 0), so a rounded
      glass layer showed its backdrop unblurred under its tint; the greeter's glass circles and
      pill read darker and less blue, the wallpaper's contours blurred inside them. Found, left to
      its owner (windowd's scene): a glass region blurs the window body drawn before it
      (`scene.rs`: "the regions then blur the drawn body"), and the body holds the region's own
      text — the greeter's initial "J" shows a soft halo under its sharp copy.
    - **P6d ✅** — the QEMU lanes green on library programs (2026-10-16): `ci-os-visible`
      (`visible--2026-10-16T12-07-14`), `ci-os-visible-fhd` (`visible-fhd--2026-10-16T12-16-44`,
      the probe's proofs and the pixel proof at 1920×1080), `ci-os-visible-2d` (the 2D build's CPU
      executor, `gpud: cpu fallback`), `ci-os-usb-visible` (35/35 chain markers — input, modal,
      tiling, clipboard, capture, GPU, display); the extended recipes `just check`, `diag`,
      `dep-gate`, `test-host` (4 109 tests in 812 runs). Board cycle 20
      (`board--2026-10-16T12-47-10`, `cycle20-gpu-lines.txt`): `[PASS] board-visible`, 62 rungs,
      the operator's eight confirmations; H44–H46 confirmed — the rebuilt library loads whole, every
      read right (`samples=12/12`, `blur_dev=2`), the layer pipelines cost as in cycle 19 with their
      four more push dwords, the same 26 lines, `table_bytes=7039`. 5c-2 is complete; `just
      test-all` waits for the end of 5c.
  - Board cycles after P5c and after P5d: each picture against its host formula on the board,
    each cost a pixel.
  - Not in P5: gradient, glyphs, the solid fills (no live sender); the vertex pipeline (uv and
    push constants carry every op). Found on the way, left to their owners: the virgl path
    creates shader handle 18 twice (the gradient's and the rounded blur's — gone with P6c, which
    retired both); scene_raster may write premultiplied values that gpud composites as straight
    alpha.

| # | Hypothesis | Decision line |
|---|---|---|
| H20 | The instruction-group encoding as the open compiler's description states it reproduces the four captured programs | P1's round trip, byte for byte |
| H21 | A textured opaque layer fits in about eight groups once the program is opaque to the tile hardware (no feedback, no tile read-back, no coordinate zeroing) | the host count; on the board ns a pixel ≈ 0.15 + 0.206 × groups |
| H22 | Source-over blending fits in an epilog of a few groups in 16-bit arithmetic (the stock's further blended layer ≈ 4 groups) | the host count; the board |
| H23 | The opaque pass type lets the tile hardware drop hidden opaque pixels before shading | three stacked opaque layers cost one on the board |
| H24 | A 2D sample reads only its two coordinates | `opaque-ditr`'s sample right without the zeroing moves |
| H25 | A repeated multiply steps every register operand | `tinted`'s sample reads (40, 45, 50) |
| H26 | A pack may write the pixel output directly and end the program | every reference's sample right |
| H27 | Iterate-and-sample addresses its state as iterate and sample do | an `opaque-itrsmp` sample right |
| H28 | The toolchain's opaque layer runs as the reference it equals byte for byte | `layer-opaque`: sample right, 1.30 ± 0.05 ns a pixel |
| H29 | Iterate-and-sample feeds the premultiplied epilog (tile read, unpack, fused multiply-add, pack) as iterate and sample did | `layer-premultiplied` (11 groups): sample right, at most the unfused 13 groups' 4.49 ns |
| H30 | Our setup programs drive the pipelines as the captured ones did: the kick placed once, the iteration generated, a state load of two loads with its own size word | `gpu library ok (… refused=0 …)`; `gpu pipeline ok (samples=2/2 …)` at 1.30 and 4.04 ± 0.05 ns |
| H31 | Setup programs placed once and never rewritten serve every later pass (H10 forbids rewriting, not rereading) | 5b's four probes, the area gate's two passes and the pipeline gate's two all right |
| H32 | Our programs read neither the open driver's descriptor (sh28–30) nor its constant (sh0) | no sample changes for dropping those two loads (5b's tinted quads, the area gate) |
| H33 | The new arithmetic runs as encoded: the single-source unit, min and max through test and conditional move, the modifiers, the constant registers, the vector moves | `layer-straight`, `layer-rounded` and `shadow` read right (`gpu pipeline ok (… samples=7/7 …)`) |
| H34 | An input sampled inside iterate-and-sample and iterated again for arithmetic is the same coordinate | the rounded layer's corner pixel at the clear, its interior the texel; the shadow's edge where the formula puts it |
| H35 | A pixel's cost follows its executions on the 5c-1 line | `layer-straight` ≈ 0.8 ns above `layer-premultiplied`; `layer-rounded` and `shadow` reported for 5c-3's segmentation |

### 5c-3 — the frame (in progress since 2026-10-16; P0 measured from the code and cycle 20's present windows; P1, P2a, P2b ✅)

**Measured first** (gpud's board present, windowd's encoder and the device read through; board
cycle 20, `board--2026-10-16T12-47-10`):

- **The board's present today**: a free slot of the ring, the retained plane (windowd's
  CPU-rendered scene, Plane 1) copied into it whole (about 3 ms), windowd's commands run by the
  CPU executor (`cpu_frame`), the damage cleaned, the flip at the next blank. Cycle 20: 100–200 ms
  a present (`gpud: dc present us (…)`: `layer` 71–150 ms, `blur` 13–46 ms, `blit` 3–6 ms,
  `shadow`, `fill`, `other` 0), the damage the whole screen in nearly every window.
- **What windowd sends** (its encoder's `composite_layer_full`): per glass layer the CPU path's
  recipe — its backdrop restored from the retained plane (`BlitSurface`), blurred in place
  (`BlurBackdrop`), its cache rows written or read (`BlitAbsolute`) — then the layer
  (`CompositeLayer`: an atlas rectangle, opacity, corner radius, shadow, and the blur radius once
  more: the CPU executor blurs a glass backdrop twice). The virgl path reads the `CompositeLayer`
  set alone — the wallpaper (Plane 0), then each layer's shadow, its glass blur from the
  destination so far (cached), its content: the frame is the layer model; the restore, blur and
  cache recipe is the CPU path's own.
- **The device** (`backend/gpu/device_v2.rs`): one context, owned by gpud's boot rungs; the render
  queue's firmware objects created once from the captured bundle's setup, in the bundle's
  buffers; one pipeline table; a pass waited by spinning on the fragment timeline word between
  interrupts.
- **The emitter** (`quads::emit`): one pipeline and one texture descriptor a pass; the target the
  captured stream's (the plane bound at the captured address); every block bumped into the
  bundle's buffers' tails, never reused.
- **So a frame needs**: a pass of mixed draws (each its own pipeline — pass type, setup — and
  texture descriptor), a target per pass (the end-of-tile program's back-end words: a ring slot,
  a GPU-only scratch), pass memory reused once the timeline has reached the pass that read it,
  the frame planned from the layer set, windowd's context with its library over IPC.

**The frame's form (from the measurements above):**

- **One compositor model on both paths.** A frame planner (`nexus_gfx`, host-tested) turns the
  retained layer set — with the scroll and transform overrides — into the frame's draws: the
  wallpaper, then per layer its shadow, its glass step (the cache's read, or the blur written into
  its rows; the key the destination-so-far fold the QEMU path keys by), its content; the frame's
  area from the damage (buffer age). The virgl path executes the plan with its draws, the board's
  display backend with GPU passes; `BlitSurface`, `BlurBackdrop` and `BlitAbsolute` stay the CPU
  path's until 5d deletes it.
- **One pass writes the slot**, limited to the frame's area and cleared inside it — everything
  that touches the area is drawn again, so nothing needs the old pixels loaded. A glass whose
  cache is stale gets three passes first: the scene below it rendered at half resolution straight
  into a scratch (the draw is the downsample), the horizontal and the vertical blur into its cache
  rows; the slot's pass samples the cache.
- **Memory**: the pass arenas as rings reclaimed by timeline points; the scratch and the cache
  GPU-only buffers of the context; nothing per pass from gpud's heap.

**The context model (operator, 2026-10-16: B — several contexts now).** Compared first (the
open kernel driver of this GPU family, Fuchsia's GPU driver model, the desktop and phone systems
of a render-server design, OHOS): every one of them gives each client its own GPU address space —
its page tables and the firmware's memory context — and owns the client's render context, free
lists and dataset per client, torn down by the firmware's cleanup when the client goes; the UI is
drawn in one privileged context of the highest priority (a compositor client, or a render server
drawing the clients' layer trees — our windowd and gpud). So the device grows several contexts in
this sub-step, each its own space, before windowd's context draws the desktop.

| # | Package | Gate (board) |
|---|---|---|
| P1 | **The mixed pass**: a pipeline and a texture descriptor per draw, a target per pass (the end-of-tile back-end words), the arenas as rings reclaimed by timeline points; the present-mix instrument (`gpud: dc present mix (` — commands and pixels by kind) | a rung of mixed draws into a second target, and a frame's worth of passes through the rings without growth: `gpud: gpu mixed ok (` |
| P2a | **Memory for many contexts** (the Fuchsia-grade base, operator 2026-10-16: "production grade as Fuchsia would do it"): a binding maps a range of a buffer (`Mapping { bo, offset, len, va, access }`, the GPU-memory model of every production stack — few large buffers, ranges bound), GPU-only buffers that are no region of gpud's address space (`CacheMode::GpuPrivate`), buffers destroyed by their client, the firmware heap and its backing returning what a context held (a page bitmap each, recycled pages zeroed), the page tables' storage reused, nothing per context from gpud's heap; the scene loader packing a bundle into one buffer per flag class | the rungs unchanged on the new memory: the ladder green, gpud's regions and heap reported |
| P2b | **Several contexts**: each context its own GPU address space (a page catalogue, the firmware's memory context), its transfer queue on first use, its render context, free lists and dataset (the open kernel driver's per-client objects), torn down in the open driver's order by the firmware's cleanup (common contexts, the two render-target data, the free lists; a busy object retried after the firmware's messages, never freed while the firmware may hold it); contexts numbered with a generation (a destroyed id stays refused), points carrying their context; `max_contexts` above one | two contexts drawing side by side through the same addresses into their own memory, each refused the other's buffers and addresses, both torn down with every page back: `gpud: gpu contexts ok (` |
| P2c | **A fault stays in its context**: the firmware's context-reset message attributed to its context by the page catalogue it names (5a cycle 3 measured one: `pc=` and `fault=` of a write fault), that context lost — its submits refused by name, its points released — the others untouched; the firmware's free-list reconstruction answered (the open driver's handler), so the device runs on | a context that names another's address faults alone, the other keeps drawing, a fresh context afterwards draws |
| P3 | **windowd's context**: created at windowd's request at the highest priority, its library over IPC (RFC-0101 §7b's wire form, identity `sender_service_id`, `test_reject_*` for the op), torn down when windowd goes (a supervision restart gets a fresh one) | `gpud: gpu library ok (client=windowd …)` |
| P4 | **The desktop on the GPU**: the frame planner (host, shared with virgl — the QEMU lanes unchanged) and the board's executor in windowd's context — the layer set composed into the slot, flipped at the blank; the CPU composite kept behind a switch until 5d | `gpud: gpu desktop ok (frame_us=)`; the operator's desktop |
| P5 | **What changed, only**: the frame's area from the damage (buffer age), the blur cached at half resolution | a hover-sized damage's `frame_us`; cache hits counted |

- **P1 ✅ 2026-10-16 — the mixed pass** (board cycles 21–22, measurement README "5c-3 P1"):
  the emitter takes draws of different pipelines and textures (`quads::emit_draws`, `Draw`): each
  draw its pipeline's setup and its own texture descriptor, its block re-stating the ISP words
  (pass type) where it differs from the draw before; `quads` split into the types and the
  emitter (`quads/emit.rs`). A target per pass (`backend::gpu::eot`): the end-of-tile program's
  two state words carry the target's address — a copy per target, placed once
  (`render_template::Targets`), the fragment jobs' pixel event naming it. The pass memory as
  rings (`quads::Rings`): a pass's lines taken until the timeline reached it, an emptied ring
  starting again at its first line. The present mix measured (`gpud: dc present mix (`). Board:
  cycle 21 red on the gate's own FAIL form (the second pass found no room — the rings kept their
  head past what an empty ring holds; fixed on the host), cycle 22 `gpud: gpu mixed ok
  (draws=3 targets=2 reads=8/8 … ring_passes=48 ring_reads=96/96 reused=49 stale=0
  heap_bytes=0)`, `[PASS] board-visible` 63 rungs, the operator's eight confirmations; H47–H50
  confirmed — data lines are reused once their pass is reached (H10 narrowed: programs stay
  placed once). Carried: a mixed pass's cost (7.5 ms for three draws against ~1.3 ms predicted)
  split in P2's first cycle; the desktop asks for 3–9 layers a present over about one screen,
  1–7 glass layers of 70–300 K pixels.
- **P2 — measured first (2026-10-16, from the code, the open kernel driver and the 5a/5c logs).** What stands between the device and a second context: (1) every buffer is a region of gpud's address space, which holds 64 — the bundle alone binds 36, gpud sits at about 61 after the ladder (the 5c-1 cursor-layer failure); (2) the firmware heap's map is a table of 128 ranges, one per object, and a render context takes about twenty; (3) its backing pool (512 pages) and the page tables taken from it are never returned (a bump allocator); (4) gpud's own heap never frees — the page tables' slot vectors (7 KiB), the free lists' entry vectors and the pipeline table are allocated per context; (5) the free lists' parameter pages are CPU-mapped buffers the CPU never touches; (6) nothing sends the firmware's cleanup (`KCCB CLEANUP`, 106): the one context's objects live for the boot, and `destroy_context` drops every buffer of its client. The open kernel driver per client: a VM context (its page catalogue and a firmware memory context naming it, `page_cat_base_reg_set` left to the firmware), contexts on it (each queue a common context), the dataset and the free lists as their own objects; destroyed queues first (cleanup of each common context once its jobs are done), then the render-target data, then the free lists (a busy free list retried after the firmware's messages), the memory context last. The firmware's reset message names the page catalogue of the context that faulted (5a cycle 3: `pc=0x5c43000 fault=0xdbf0000000`, then a free-list reconstruction request). Hence P2a–P2c above: the memory first, then the contexts, then the fault.
- **P2a + P2b ✅ 2026-10-16 — memory for many contexts, several contexts** (board cycles 23–24, measurement README "5c-3 P2a/P2b"). P2a: a binding is a range of a buffer (driverkit `Binding.offset`, `Mapping`), a buffer is destroyed by its client once nothing binds it (`destroy_bo`, `test_reject_destroy_bound_bo`, `test_reject_mapping_past_the_buffer`), `GpuPrivate` buffers are the GPU's alone (no region of gpud's address space — the free lists' parameter pages, the partial render's scratch), the firmware heap a page bitmap (`HeapMap`, no table to run out of), its backing and the page tables returning what a context held (recycled pages zeroed), the tables' slot storage kept per context slot (`TableStorage`), the scene loader packing a bundle by flag class (three regions where there were 36). P2b: each context its own catalogue and firmware memory context, its transfer queue on first use, its render objects, numbered by slot and generation, points carrying their context; destroyed in the open kernel driver's order by the firmware's cleanup (`KCCB CLEANUP`, busy asked again after the next interrupt, nothing freed — the slot quarantined — when the firmware will not let go); the G1 rungs give their spaces back; the render rungs draw in the render rung's context, which the contexts rung destroys. Board: cycle 23 red on a kernel defect P2 uncovered — an unmap left its emptied page table linked, and every later superpage over that span was refused (`VM-MAP-FAIL reason=pte … Overlap`: B's target, then every screenshot); fixed in the kernel (operator: as Zircon does — the tables an unmap empties are retired and freed after its TLB shootdown, a superpage over a table falls back to pages; `KSELFTEST: vm unmap ok` proves both). Cycle 24 `gpud: gpu contexts ok (rounds=2 contexts=2 samples=16/16 refused=20/20 … pc_a=0x6443000 pc_b=0x6445000 backing=957/957 bos=0 quarantined=0 cleanup_busy=0 heap_delta=0)`, `[PASS] board-visible` 64 rungs, the operator's eight confirmations; H51–H55 confirmed, H56 refuted (two contexts' passes run one after the other), H57: a mixed pass's time is its geometry stage's. Next: P2c.

## Hypotheses 5a's first cycle decides

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | The firmware accepts our render context (two common contexts + the captured 23 context-switch registers) and runs the geometry job | `UPDATE g` reached, one interrupt; an FWCCB fault inside a scene range = our binding, outside = the context |
| H2 | The free lists as the kernel builds them (544 PM pages, the kernel's init words) suffice for one quad; no growth request | no `FREELIST_GROW` in the FWCCB; if one arrives, its handler is 5a's second cycle |
| H3 | The fragment job writes the blended quad into the linear plane with the PBE words the open driver emitted for a linear attachment | the four samples; wrong colours = PBE/layout, a fault = the dataset's addresses (region headers, tail pointers, macrotile lists) |
| H4 | The partial-render fragment command is skipped when the geometry fit | `irqs` per pass (2 or 3) and f1's word |
| H5 | One pass (geometry + fragment, 512² blended quad) costs about the stock fixed cost plus the fill | `us=` in `gpu render ok` against 0.64 ms + 0.5 ms |
| H6 | (carried) the firmware's time variance and the controller's alpha handling | `boot_us`, a flush's µs; the quad's corner with alpha 0 described by the operator |

## Definition of Done (per step; 5a first)

1. The measurement folder exists and answers its question with archived raw data.
2. The host model reproduces the captured streams and the kernel's words (goldens) and refuses
   the malformed ones.
3. The board lane is green on its REQUIRED ladder with the step's gate; no tolerated red lacks a
   reference.
4. The gate has been shown red once, with the transcript archived.
