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
| 5c-3 | **The frame**: windowd's command list as GPU draw lists in gpud's display backend, damage-driven (buffer age, the render area), the blur cached at quarter size, the transient buffers recycled by the timeline | `gpud: gpu desktop ok (frame_us=)` |
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
| P4 | the pipeline library: format, build step, windowd supplies it, gpud validates and binds (`test_reject_library_*`) | the rejects green; the rungs draw with library pipelines, the captured programs retire to measurement |
| P5 | the remaining ops: rounded-rect SDF, gradient, shadow, the blur's passes, glyphs | each picture against its host formula, each budget |
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
- **P3c ⬜**: the backend — the IR's textured layer compiled into these programs (the references
  are its goldens), the setup programs and state words generated, the pipeline's gate.

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
