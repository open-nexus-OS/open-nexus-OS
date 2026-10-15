<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-13 — the render queue: one blended textured quad through geometry and fragment jobs, captured on the host (TASK-0215 5a, layer 5)

## Question

What does the open userspace driver for this core submit for a **render pass** — a clear and one
textured quad drawn with source-over blending into a linear 1920×1080 BGRA target — beyond what
the transfer queue needed in layer 4: which kernel-side objects (render context, free lists,
render-target dataset), which jobs in which order, which buffers; and which of those bytes move
when the geometry changes (one quad → three quads with different positions and tints)? Does the
same pass, with the kernel side's objects built by us, draw the blended quad into a plane of the
ring on the board?

## Instrument

**Host (the capture).** The same shimmed open driver and capture hook as layer 4
(`../2026-10-13-gpu-first-quad/capture/pvr_dump_hook.h`), the program `capture/render_quad.c`
with the shaders `capture/quad.{vert,frag}` (vertex: pixel positions and uvs from a vertex buffer
of stride 16; fragment: `texture(tex, uv) * tint`, the tint a 16-byte push constant), a render
pass with one colour attachment (load CLEAR to (0.10, 0.12, 0.16, 1.0), store), a pipeline
with blending `src_alpha / one_minus_src_alpha` (alpha `one / zero`), the 256×256 checker texture
of layer 4 as a **linear**, host-written sampled image, and a **linear** colour attachment —
both accepted by the driver for this core (its linear restriction is for 3D images only). Two
runs: `render1` (one quad at (704, 284), 512², tint 1) and `render3` (three quads at (704, 284),
(304, 134), (1104, 434) with tints (1,1,1,1), (1,.5,.5,1), (.5,.5,1,.6)). `PVR_DEBUG=cs` made the
driver print its own decoded control stream (`capture/render1/control-stream.txt`, 1 226 lines:
VDM `PDS_STATE_UPDATE`, vertex-shader and PDS data, the draw).

**Board (the replay).** `gpud: gpu render ok (us= samples=4/4 irqs=)` after `gpu quad ok`, its
FAIL forms (sample, firmware fault, a free-list growth request), the operator's eyes.

## Results — the capture (host, 2026-10-13)

**Objects created** (`capture/render1/log.txt`): two free lists — handle 1 at `0x8000009000`
(initial 512, max 65 536, grow 256, threshold 13 %) and handle 2 at `0x80010c5000` (initial =
max = 32, no growth) — four contexts (one transfer, two compute-type, one **render** with 192
bytes of static state and a call stack at `0x800006d000`), one render-target dataset
(`hwrt1.args`, 136 bytes, decoded):

| Field | Value |
|---|---|
| width × height, samples, layers | 1920 × 1080, 1, 1 |
| tail pointers (`tpc_dev_addr`, stride, size) | `0x80010c7000`, 32 768, 0 |
| vheap table / rtc | `0x80010c6000` / 0 |
| rt data 0: mlist, macrotile array, region headers | `0x80010cf000`, 0, `0xdbf0000000` (the `RGNHDR` heap, base `0xdbf0000000`, 256 MiB) |
| rt data 1: mlist, macrotile array, region headers | `0x80010d0000`, 0, `0xdbf00032c0` |
| free list handles `[local, global]` | `[2, 1]` — the 32-page list is the local one (the kernel's minimum for this core: 25 pages) |
| isp merge lower/upper/scale | all 0 |
| region header size | 2 160 |

**Jobs** — three in one submit, all on the render context (handle 4), dataset 1 / data index 0:

| # | Type | Flags | Stream | Firmware command |
|---|---|---|---|---|
| 0 | geometry | `0x3` = FIRST \| LAST | 40 B: `vdm_ctrl_stream_base`, `tpu_border_colour_table`, `ppp_ctrl`, `te_psg`, `vdm_context_resume_task0_size`, `view_idx` (no `VDM_DRAWINDIRECT`, `VDM_OBJECT_LEVEL_LLS`, `TESSELLATION` on this core) | `GEOM` (201), flags FIRSTKICK \| LASTKICK, the dataset's address, the partial-render fence |
| 1 | fragment | `0xc0` = PARTIAL_RENDER \| DISABLE_PIXELMERGE | 384 B | `FRAG_PR` (204) |
| 2 | fragment | `0x80` = DISABLE_PIXELMERGE | 384 B: 6 × u64, `pbe_word[8][3]`, border table, `pds_bgnd[3]`, `pds_pr_bgnd[3]`, `usc_clear_register[8]`, then `usc_pixel_output_ctrl`, `isp_bgobjdepth`, `isp_bgobjvals`, `isp_aa`, `isp_ctl`, `event_pixel_pds_info`, `view_idx`, `event_pixel_pds_data`, `isp_oclqry_stride` (multicore), `zls_stride`, `sls_stride`, `execute_count` (multicore) — no `fb_cdc_zls`, `isp_xtp_pipe_enable`, `pixel_phantom`, `isp_zls_pixels`, `rgx_cr_blackpearl_fix` | `FRAG` (203), flags DISABLE_PIXELMERGE (bit 15) |

The open kernel driver pairs job 0 with job 1 (the partial-render fragment depends on the
geometry through a sync object): the geometry command carries the fragment queue's point minus
one as its partial-render fence, the fragment command buffer gets a `FENCE_PR` on the geometry's
point before `FRAG_PR`; job 2 follows on the fragment queue with no explicit fence — the
dataset's state machine orders it behind the geometry.

**Buffers** — 111 bindings seen across the three submits (the static areas, the contexts'
state, the texture, the plane, the free lists, the dataset's memory, PDS and USC code). Between
`render1` and `render3` exactly four change (everything else, the USC code included, is
byte-identical):

| GPU address | Size | Non-zero bytes 1 → 3 quads | What it is |
|---|---|---|---|
| `0x80010d1000` | 4 KiB | 30 → 54 | the geometry control stream (VDM: `PDS_STATE_UPDATE`, the draw records) — `vdm_ctrl_stream_base` of the geometry job, decoded by the driver's own dump |
| `0x80008da000` | 4 KiB | 154 → 114 | the vertex buffer (6 → 18 vertices of 16 bytes: pixel positions and uvs) |
| `0xda00001000` | 128 KiB | 1 139 → 1 211 | PDS programs and data — the per-draw constants (the tint) |
| `0x800004b000` | 128 KiB | 115 → 176 | descriptor words — the texture/sampler state per draw |

Decisions this takes: 5a replays `render1` unchanged (the kernel side's objects are ours); 5b
writes those four buffers on the device from the open driver's word layouts; the plane is the
attachment with no copy; the `RGNHDR` heap joins the device's heaps.

## Hypotheses 5a's first cycle decides

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | The firmware accepts our render context (two common contexts + the 23 captured context-switch registers) and runs the geometry job | the geometry's `UPDATE` reached; an FWCCB fault inside a scene range = our binding, outside = the context |
| H2 | The free lists as the kernel builds them (544 PM pages) suffice for one quad, no growth request | no `FREELIST_GROW` in the FWCCB |
| H3 | The fragment job writes the blended quad into the linear plane with the open driver's PBE words | four samples (B G R, ±4): background `29 1f 1a` at (10,10); the frame at alpha 252/255 → `fc fc fc` at (1210,790); checker (40,90,200) at alpha 215/255 → `af 51 26` at (1150,700); checker (230,120,30) at alpha 188/255 → `21 61 b0` at (1040,700) |
| H4 | The partial-render fragment is skipped when the geometry fit | interrupts per pass, the first fragment point's word |
| H5 | One pass costs about the stock fixed cost plus the fill | `us=` against 0.64 ms + 0.5 ms |
| H6 | (carried) the firmware's time variance; the controller's alpha handling | `boot_us`; the quad's alpha-0 corner as the operator sees it |

## Results — the board

**Cycle 1 (2026-10-14, `build/logs/board--2026-10-14T08-52-45`) — red, named:** layers 3 and 4
unchanged (`gpu fence ok (rt_us=82)`, `gpu fill ok (ns_per_px=0.8)`, `gpu quad ok (samples=4/4)`),
then `gpud: FAIL gpu render (no memory for the fragment queue)`: the firmware heap's backing pool
(192 pages, a bump allocator whose page-table pages are never returned) was spent by layer 4's
scene, the render scene's rebinding and the geometry queue's buffers before the fragment queue
could be allocated — so read; the pool went to 512 pages and the FAIL line gained `backing_free`.
Also seen: `unmap_us=9696` for the MMU-cache flush after an unmap (46–50 µs in other boots):
intermittent, handed on with the firmware's time variance.

**Cycle 2 (2026-10-14, `build/logs/board--2026-10-14T09-13-42`) — red, the real cause:**
`gpud: FAIL gpu render (no memory for the fragment queue: 4 bytes, backing_free=381)` — pages
were plenty; the firmware heap **map's range table** (32 entries, one per object, every small
object its own page) was full: the firmware's ~15 objects, the transfer context's 5, the render
context's and geometry queue's 5, then the fragment queue's timeline word had no slot. Fix: the
table at 128 entries. The first hypothesis (the backing pool) was wrong by one layer of the
allocator; the FAIL line's numbers decided it.

**Cycle 3 (2026-10-14, `build/logs/board--2026-10-14T09-29-14`) — red, H1 decided for, a binding
of ours:** the render context, the free lists and the dataset were accepted and **the geometry
job ran** (`geom_word=1`: its `UPDATE` reached, `irqs=3`); then the firmware reset the context —
`FAIL gpu context reset (ctx=2 reason=2 dm=2 job=1 flags=0x1 pc=0x5c43000 fault=0xdbf0000000)`,
followed by a `FREELISTS_RECONSTRUCTION` request (type 104) — and the fragment point never
came. The fault address is the first region header: the tile accelerator **writes** region
headers, macrotile lists, tail pointers and context state, and the device had bound every data
buffer read-only (the transfer scene only read them). Fix: data buffers read-write, the texture
alone read-only.

**Cycle 4 (2026-10-14, `build/logs/board--2026-10-14T09-41-20`) — green** (`cycle4-gpu-lines.txt`):

```text
gpud: gpu render ok (us=6133 samples=4/4 irqs=3 geom=1 frag=2 bind_us=8447 shown row=3240)
```

| What | Measured | Bar |
|---|---|---|
| Binding the render bundle (37 buffers, two 8 MiB among them) + the render objects | 8 447 µs | — |
| The pass: geometry + partial-render fragment + fragment, clear of the plane + one 512² blended quad | 6 133 µs | the stock stack's full-screen textured quad 4.4–5 ms (its `wall`), its fixed cost 0.64 ms |
| Interrupts per pass | 3 (the geometry's point, both fragment points) | — |
| Sample pixels after the flush | 4 of 4 within ±4 | 4 of 4 |
| The lane | `[PASS] board-visible: 58 rungs, 7 operator marker(s), no FAIL marker` | — |

| # | Decided | By |
|---|---|---|
| H1 | ✅ the firmware accepts our render context, free lists and dataset and runs the geometry job | cycle 3's `geom_word=1`; cycle 4 whole |
| H2 | ✅ the free lists as the kernel builds them suffice for one quad; no growth request in a successful pass | no `FREELIST_GROW` in cycle 4 (cycle 3's `FREELISTS_RECONSTRUCTION` was the reset's aftermath) |
| H3 | ✅ the fragment job writes the blended quad into the linear plane with the open driver's PBE words | the four samples: clear `29 1f 1a`, frame `fc fc fc`, both checker colours at their alpha |
| H4 | ✗ the partial-render fragment is **not** skipped: three interrupts and both fragment points — the firmware runs the `FRAG_PR` command's update path even when the geometry fit | `irqs=3`, `frag=2` |
| H5 | ◐ the pass costs 6.1 ms, not the fixed cost plus a small fill: the clear by the background program over the whole plane plus the quad; against the stock's 4.4 ms full-screen quad it is the same order — the frame budget work is 5c's | `us=6133` |
| H6 | ◐ (carried) the firmware's time variance (`unmap_us` 46–9696 µs across cycles) and the controller's alpha handling | the lines; the operator |

**The gate shown red** (`cycle4-gate-red.txt`): the capture without the `gpu render ok` line is
refused by the lane — `rung missing: 'gpud: gpu render ok (' (rung 30 of 58, last present: 'gpud:
gpu quad ok (')`; the device-side red fired for real in cycles 1–3, each line naming its cause.

## Verdict

The render queue runs on the board with the kernel side built by us: one render context of two
queues with the captured context-switch state, two free lists of the device's pages initialised
as the open kernel driver does, a render-target dataset from the open kernel driver's formulas,
and the open driver's three jobs kicked in its sequence — the geometry fence, the partial-render
fragment, the fragment — into a **linear plane of the ring**, pixel-exact at four samples of the
blended quad. Three red cycles each named their cause in a line (the heap map's table, a
read-only binding of memory the tile accelerator writes), the fourth was green. Learned for 5b
and beyond: data the GPU writes needs read-write bindings (the bundle should say which — a kind
`Written`, or access per buffer); every small firmware object costs a page and a table entry
(pack them); the partial-render job is executed, not skipped, so its cost is part of every pass;
a pass is 6.1 ms with a full-plane clear, so 5c's frame must avoid the clear (damage) to meet
the 23.3 ms gate scene with margin.

## 5b — what moves per draw (host, 2026-10-14; `capture/variants/`)

Five captures with the open driver's own decoded control stream (`PVR_DEBUG=cs`): the base
(one quad), the quad shifted by 96 px, the quad tinted, two quads, three quads
(`capture/variants/*/control-stream.txt`, the byte diff `diff-dynamic-buffers.txt`; ranges in
the driver's unused sub-allocations hold host-pointer garbage and are noise).

**The geometry control stream is six records, 60 bytes** (`vdm.xml` of the open driver names the
words): `PDS_STATE_UPDATE` (a VDM-targeted PDS program: data 16 B at PDS heap `0x28c0`, code at
`0x2800`) · `PPP_STATE_UPDATE` 26 words at general arena `+0x8200` (header, ISP state, **STATE_PDS**
— the fragment PDS programs: shaderbase `0x2900`, texunicodebase `0x2700`, varyingbase `0x2940`,
uniformdatabase `0x2880`, size words `0x01800411 / 0x01004000` — region clip 0..3808 × 0..2144,
viewport (960, 960, 540, 540), wclamp, output selects vtxsize 7, varying 2 × f32 linear, PPP
control) · `VDM_STATE_UPDATE` (vertex PDS data `0x2840` / code `0x2740`, output size 7, max
instances 16, sizes `0x444`) · `INDEX_LIST` (tri list, 6 indices, no index buffer) ·
`PPP_STATE_UPDATE` 3 words at `+0x40` (terminate: clip 1888 × 1056 in 32-px units) · `TERMINATE`.

**A second draw adds 24 bytes to the stream and four small blocks:** `PPP_STATE_UPDATE` 7 words
(header `0x2c0` = pds pointers 0, 1, 3; STATE_PDS with a **new uniformdatabase** `0x2980` and copies
of the shader and varying programs at `0x29c0` / `0x2a00` — byte-identical to the first draw's,
so one copy serves) · `VDM_STATE_UPDATE` with nothing present (`0x40000f80`) · `INDEX_LIST` with
`index_offset` 6 (the vertex buffer is one buffer of 6 N vertices; the vertex PDS data that names
it, `0x2840`: `[0x80008da000, +8]` for the two attributes, is unchanged). The per-draw **fragment
uniform data** (48 bytes, three DMA descriptors and a tail `14800010 04000310 03800310 01000090`)
points at: the texture state words (`+0x80`, shared), the **push constants** (16 bytes, the tint:
`+0x100` for draw 0, `+0x8280` for draw 1), a 16-byte descriptor `{tint address, 4, 0}` (`+0x4140`,
`+0xc2c0`) and a constant word `0x01fffffe` (`+0x4180`, `+0xc300`). Shifting the quad changes only
the vertex buffer's floats; tinting changes only the 16 bytes at `+0x100`.

So the device writes, for N quads at runtime positions and tints: the vertex buffer (6 N × 16
bytes), N tint blocks, N descriptor pairs, N − 1 uniform-data copies with three addresses patched,
N − 1 seven-word PPP blocks, and the stream's per-draw records — everything else comes from the
bundle. Hypotheses 5b's first cycle decides:

| # | Hypothesis | Decision line |
|---|---|---|
| H7 | One fragment PDS program copy serves every draw (the per-draw copies the open driver makes are byte-identical) | the three quads' centre colours; a wrong tint on draws ≥ 1 = per-draw program state |
| H8 | The index offset addresses the shared vertex buffer as the open driver's second draw does | the quads at their positions |
| H9 | Our blocks in the arenas' unused tails (general `+0x10000`, PDS heap `0x3000`) are read as the driver's are | a fault names the address; a wrong colour names the block |

**5b cycle 1 (2026-10-14, `build/logs/board--2026-10-14T11-25-48`, `cycle5-gpu-lines.txt`) — red,
named:** the pass with three draws completed its points (790 µs, four interrupts) but quad 0's
centre read `88 65 6f` instead of `79 3d 21`, and the firmware then reset the context from the
fragment stage (`reason=1` lockup, `dm=3`, `fault=0x4840`). Read against the capture: the
descriptor the uniform data names is `{tint + 0x40, 4, 0}` — it points at **16 zero bytes behind
the tint**, not at the tint (the driver's +0x4140 names +0x140, its +0xc2c0 names +0x82c0, both
zero); the emitter had named the tint itself, so the fragment stage loaded the tint floats as that
block. Fix: the open driver's shape, a 64-byte line each — tint, zero block, descriptor, constant,
PPP block.

**5b cycle 2 (2026-10-14, `build/logs/board--2026-10-14T11-40-07`, `cycle6-gpu-lines.txt`) — red,
the same line to the byte:** `quad 0 centre (960,540) expected bgr 793d21 read 88656f`, then the
fragment stage's reset with `fault=0x4840`. The descriptor's target was not it; the failure is
deterministic and lies elsewhere in the per-draw emission or its placement. Also seen this boot:
`unmap_us=22192` for the flush after the first unmap (the firmware's time variance, 46 µs to
22 ms). Next cycle: three probes in one boot — one quad (our writes alone), two quads at the
capture's offsets (the content, placed as the open driver placed it), three quads at our offsets
(the placement) — each a line.

**5b probe cycle (2026-10-14, `build/logs/board--2026-10-14T12-03-05`, `cycle7-gpu-lines.txt`) —
the cause named:** the first probe — **one** quad, the captured records and blocks untouched but
for our tint, vertices and stream bytes identical to the capture's — failed exactly as the
three-quad pass had (`read 88656f`, the fragment stage's reset, this time `fault=0x0`); the
following probes timed out on the dead context. So the per-draw emission was never the problem:
**a second pass on the same render-target data fails.** The two-frame host capture
(`capture/variants/r_2frames/`) shows what the open driver does between frames: the geometry and
fragment jobs of frame 2 name **render-target data 1** (`hwrt=1/1` after `hwrt=1/0`) — the dataset's
two data alternate frame by frame; the streams differ only in fresh allocations of the control
stream, the scissor block and the per-frame PDS data blocks. Fix: the device alternates
`data[pass % 2]` (both were built in 5a). Also seen: the render rung's pass counted four
interrupts this boot (three before).

**5b cycle with alternation (2026-10-14, `build/logs/board--2026-10-14T12-21-02`,
`cycle8-gpu-lines.txt`) — two things decided:** (1) the single-quad pass on render-target data 1
**ran whole** (6 086 µs, two interrupts, no reset) and read `59 42 45` at the quad's centre — which is
the **right** value: the centre sits on a checker block boundary, where the bilinear filter mixes
(40,90,200) and (230,120,30) to (111,101,136), and that over the clear at alpha 128/255 is exactly
`59 42 45`; the earlier cycles' `88 65 6f` was that mix unblended. The rung's expectation was the
error, not the picture — the sample moves 272 px into the quad, inside one block. (2) The next
pass, on data 0 again, reset the fragment stage with `fault=0xe00004b080` — the texture state
block's offset with the USC heap's high byte — and the probe was the first with two draws and
the fragment PDS programs **shared** between them, so H7 (sharing) and data-0 reuse are
confounded. Next cycle's probes separate them: one quad (data 1), one quad again (data 0),
two and three quads with per-draw program copies as the open driver makes them, two quads with
the programs shared last.

**5b probe cycle 2 (2026-10-14, `build/logs/board--2026-10-14T12-30-59`, `cycle9-gpu-lines.txt`):**
`probe one ok` (data 1), `probe one-again ok` (data 0 reused — the alternation works and so does
reuse), then `probe two-copied FAIL`: quad 0 right, **quad 1 never drawn** (its sample is the
clear colour), the pass 52 ms long and the fragment stage reset without a fault address. Every
byte of the second draw's blocks and records was the capture's, at the capture's offsets, with
the program copies — so the content is right and the difference to the open driver is **where
the bytes land**: the driver allocates afresh every frame (the two-frame capture: a new control
stream buffer, new scissor and PDS blocks), we rewrote the control stream the GPU had read in
the pass before — the records past the old `TERMINATE`. H10: the GPU's caches hold lines of a
buffer across kicks; a buffer read by one pass must not be rewritten for the next. Next: every
pass in fresh memory — the stream, the vertices, the vertex PDS data, the 26-word PPP block, the
uniform data, each draw's blocks — and the geometry job's `vdm_ctrl_stream_base` patched to it.

**5b green (2026-10-14, `build/logs/board--2026-10-14T13-38-29`, `cycle10-gpu-lines.txt`):**

```text
gpud: gpu quads probe one ok (n=1 us=6093 irqs=3)
gpud: gpu quads probe one-again ok (n=1 us=6122 irqs=4)
gpud: gpu quads probe two ok (n=2 us=9590 irqs=4)
gpud: gpu quads probe three ok (n=3 us=13096 irqs=4)
gpud: gpu quads probe two-shared ok (n=2 us=9655 irqs=4)
gpud: gpu quads ok (n=3 us=13096 samples=3/3 irqs=4 shown row=2160)
```

| # | Decided | By |
|---|---|---|
| H7 | ✅ one fragment PDS program copy serves every draw | `probe two-shared ok` (the same picture as `two`) |
| H8 | ✅ the index offset addresses the shared vertex buffer | the three quads at their positions, each sample right |
| H9 | ✅ our blocks in the arenas' unused tails are read as the driver's | every probe placed by the bump placer |
| H10 | ✅ **a pass must land in memory the GPU has not read** — the control stream, the vertices, the vertex PDS data, each draw's blocks fresh per pass, the geometry job's `vdm_ctrl_stream_base` pointing at the pass's stream | the probes that rewrote the stream failed (cycles 1–2, probe cycle 2); with fresh placement five passes in a row succeed |

New number, for 5c's budget: a pass costs **6.1 ms with one 512² blended quad, 9.6 ms with two,
13.1 ms with three — about 3.5 ms a quad**, far above the stock stack's 1.8 ms for a further
full-screen blended layer. Candidates: every binding bypasses the system-level cache
(`slc_bypass`, taken from layer 3's single page), so texture fetches go to DRAM; the
partial-render job runs each pass; `DISABLE_PIXELMERGE` and `pds_tri_merge_disable` in the
captured state. Measured next, in 5c's first cycle, binding by binding.

The lane's judge on this capture: red on an unrelated rung — the desk keyboard
(`xhcid: device enumerated (vid=3434 pid=0123 …)`) was not enumerated this boot (the hub was;
Block 2's USB lane); every GPU rung present. The operator confirmed the keyboard was unplugged
(2026-10-14) and accepted this capture as 5b's board proof without a further cycle. The gate shown red (`cycle10-gate-red.txt`): the
same capture without the `gpu quads ok` line is refused by name.

## Verdict 5b

Three quads at positions and tints the device chose, drawn by the captured pass's programs with
every per-draw word written by the device: the geometry control stream's records, the PPP
blocks, the fragment PDS uniform data, the vertex PDS data, the vertices — from the open
driver's word-layout data, checked byte for byte against two captures. Four red cycles each
named a cause and two taught the rules this layer builds on: the render-target data alternate
frame by frame, and a pass lands in fresh memory. What remains for 5c is the compositor's
pipelines and the frame; its first measurement is the per-quad cost above.

## 5c-1 — the cost of a pass (TASK-0215 5c; host and board 2026-10-14)

**Question.** A pass costs us about 3.5 ms per 512² blended quad and about 2.6 ms of fixed cost
(5b); the stock stack draws a further blended full-screen layer in 1.8 ms. Where does the time
go, and which of the open driver's choices that we do not make closes the gap?

**What the host shows** (`capture/variants/c_full`, `c_area`, `c_load`, `c_area_load`; the
capture hook now logs every buffer's creation flags):

- **The GPU's system-level cache.** The open driver creates only eight buffers with "bypass
  device cache": the two free lists and the mlist (also "PM/firmware protect"), the render
  context's two context-state pages, the vheap table with the RTC, the tail pointers, the region
  headers. Everything else — vertices, control streams, PDS and USC code, uniforms, textures, the
  colour attachment — is cached. We have mapped every page bypassing the cache since layer 3.
  The render bundle now carries the flags per buffer (`buffer_flags`), repacked from `c_full`.
- **Pixel merging.** The open driver disables it for every render ("until it is safe").
- **The render area.** For a render area smaller than the target, the open driver changes three
  things: the pixel back end's clip (`REG_WORD0.minclip_x`, `REG_WORD1.minclip_y/maxclip_x/
  maxclip_y`, pixel-precise), the draws' region clip (16-px tiles), and `ISP_CTL.
  process_empty_tiles` off — clearing by a quad over the area instead of the background object
  on every tile. With `LOAD` it keeps processing empty tiles (the background loads them).

**Instrument.** gpud's cost rung after `gpu quads ok`: one quad of 16², 512² and 1024² per
condition, each pass a line — `gpud: gpu cost (series= size= us= geom_us= frag_pr_us= frag_us=
irqs= write_us= sample=)`, the stage times stamped at the interrupts — in fresh lines of four
transient buffers: S1 every page out of the cache (today); S2 the open driver's flags (the plane
kept out); S3 S2 with pixel merging; S4 S2 limited to the quad (clip, no empty tiles); S5 S4 with
pixel merging; S6 S2 with the plane cached. The policy switch rewrote every page's entry in
place (`PageTables::rewrite`, removed with the policy decided) with one waited flush, its cost
in `gpud: gpu cost policy (…)`.

| # | Hypothesis | Decision line |
|---|---|---|
| H11 | The cache bypass of every page is most of the per-pixel cost | S2's slope (1024² − 512²) far below S1's; S2's samples right |
| H12 | Pixel merging lowers the per-pixel cost and is safe for this pipeline | S3's slope below S2's, samples right |
| H13 | The fixed cost is the background object on every tile of the plane | S4's 16² pass far below S2's 16² pass |
| H14 | The firmware makes the back end's writes visible at the end of a pass with the plane cached | S6's sample right (and its cost against S2's) |
| H15 | The fragment stage carries the cost | `frag_us` against `geom_us` in every line |

**Results — the board, first cycle** (`cycle11-gpu-lines.txt`; one pass a cell, µs from the kick
to the fragment stage's timeline write; every 512² and 1024² sample right):

| Series | 16² | 512² | 1024² |
|---|---|---|---|
| S1 every page out of the cache | 2648 | 6124 | 16392 |
| S2 the open driver's flags | 2535 | 5959 | 16241 |
| S3 S2 + pixel merging | – | 5595 | 15974 |
| S4 S2 limited to the quad | 417 | 7853 | 15259 |
| S5 S4 + pixel merging | – | – | 15175 |
| S6 S2 with the plane cached | – | – | 16243 |

The geometry stage took 0.22–0.28 ms in every pass, the partial-render job under 0.05 ms; the
CPU's writes of a pass's blocks about 0.63 ms before the kick (not in the cells). The policy
switch rewrote 4859 pages in 1.1–1.2 ms. S4's 512² pass is the one cell off its line (7853 µs
against S2's 5959) — a single pass, not repeated in this cycle.

- **H11 refuted.** The slope between 512² and 1024² is 13.1 ns a pixel under S1 and S2 alike;
  the open driver's flags save 2–4 % of a pass, a fixed share, not a per-pixel one.
- **H12 refuted for the slope.** Pixel merging shifts the pass by 0.27–0.36 ms and leaves the
  slope at 13.2 ns a pixel; the open driver disables it as not yet safe — it stays off.
- **H13 confirmed.** The background object on every tile of the plane is 2.1 ms of S2's 2.5 ms
  16² pass: limited to its quad, the pass costs 0.42 ms.
- **H14 confirmed.** With the plane cached, the back end's writes are visible after the pass;
  nothing gained (16243 µs against 16241).
- **H15 confirmed.** The fragment stage carries everything but the geometry's quarter
  millisecond.

13 ns a pixel is the bar's problem: the stock stack fills an opaque textured quad at 1.8 ns a
pixel and blends a further full-screen layer at 0.9 ns (`../2026-10-10-gpu-stock-baseline/`).
The memory system is not where the gap is.

**The boot after the rung hung** at the GPU's plane — the operator saw the two checkerboards
and nothing after. gpud's 384 KiB heap, which never frees, ran dry
(`alloc-fail svc=gpud … size=0x1000`): every binding collected its page list (16 KiB for a
plane), each policy switch collected a plan of every bound page (about 40 KiB, three times),
the buffer table grew by doubling. gpud died, windowd's presents went unanswered. The device
now walks a buffer's pages while it writes the tables, allocates the buffer table whole, and
the policy switch is gone with the policy decided (below).

**Verdict, first cycle.** The device maps every buffer by the open driver's flags (RFC-0101
§3); pixel merging stays off; a pass is limited to its damage — the back end's clip, the region
clip, no background on empty tiles — which is what 5c-3's damage-driven frame builds on, and
the second cycle's gate proves it pixel by pixel. The per-pixel cost is the open question.

**The program, on the host** (`resources/gpu/programs/README.md`; the open compiler's listing,
`PCO_DEBUG_PRINT=fs,stats`). The fragment program of `texture(tex, uv) * tint` with
source-over blending is 68 instruction groups for 17 temporaries: the sample-mask and
helper-lane prologue (14 groups), fourteen moves zeroing the unused part of the sampler's
coordinate block, the sample, four moves of the tint from shared registers before four
multiplies, the dynamic alpha-to-coverage path (14 groups, a state bit tested at run time), the
blend as an unpack of the tile's colour, eleven groups of arithmetic and a pack, and an ISP
feedback instruction that discards the helper lanes — which makes the pass type punch-through.
Two rules of the compiler explain the last part: every application fragment shader gets the
helper-lane feedback unless it declares early fragment tests, and every one reads the tile's
previous colour back for its sample check — so no program of this compiler is opaque to the tile
hardware. If the fragment stage is bound by its program, 8 clocks a pixel (13 ns at 614.4 MHz)
for 68 groups is 8.5 groups a clock, and a program of about nine groups would meet the stock's
opaque fill.

**Instrument, second cycle.** The cost rung keeps the gate and probes the program: after a 16²
pass over the whole plane, two sentinels written by the CPU — a pixel above the quad in the
quad's own tile (the back end's clip) and one in the plane's first tile (the region clip) — must
survive the same pass limited to the quad, the quad's sample must read right, and the limited
pass must cost less (`gpud: gpu area ok (`, REQUIRED). Then one 1024² area-limited pass per
program of `resources/gpu/programs` — each written into the free tail of the bundle's code
arena, named by a copy of the fragment kick word, its pass type set in a copy of the draw's
state block — each a line `gpud: gpu cost (program= groups= pass= size=1024 us= geom_us=
frag_us= irqs= sample=)`; the sample proves the program ran (the chain saturates to white, the
opaque program writes the texel unblended). Last, `gpud: gpu heap (used= of=)`.

| # | Hypothesis | Decision line |
|---|---|---|
| H16 | The fragment stage is bound by the program: a pixel costs in proportion to its instruction groups | `chain` (116 groups) against `base` (68): about 1.7× the per-pixel cost = program-bound; about equal = bound elsewhere; `opaque-eft` (55) on the same line |
| H17 | The ISP feedback (punch-through) costs per pixel beyond its one group | `eft` (67, translucent) clearly below `base` (68, punch-through) |
| H18 | A pass limited to its area writes nothing outside its rectangle | both sentinels kept, the sample right, the limited pass cheaper (`gpud: gpu area ok (`) |
| H19 | The rungs leave gpud's heap enough room for the desktop | `gpud: gpu heap` well below `of`, the desktop presenting after the rung |

**Results — the board, second cycle** (`cycle12-gpu-lines.txt`; one pass each, the quad
1024² limited to its area; the per-pixel cost is the pass less the same boot's 16² limited
pass, 420 µs, over 1 048 576 pixels):

| Program | Groups | Pass type | Pass | Fragment stage | Per pixel | Per group and pixel | Sample |
|---|---|---|---|---|---|---|---|
| `base` | 68 | punch-through | 15 253 µs | 14 987 µs | 14.1 ns | 0.208 ns | right |
| `chain` | 116 | punch-through | 25 615 µs | 25 351 µs | 24.0 ns | 0.207 ns | right (white) |
| `eft` | 67 | translucent | 15 039 µs | 14 763 µs | 13.9 ns | 0.208 ns | right |
| `opaque-eft` | 55 | translucent | 11 828 µs | 11 560 µs | 10.9 ns | 0.198 ns | right (unblended) |

The gate: `gpud: gpu area ok (us=420 full_us=2547 geom_us=226 frag_us=193 inside=ok near=kept
far=kept)`; the heap after the rungs: `gpud: gpu heap (used=87418 of=393216)`.

- **H16 confirmed.** The chain's per-pixel cost is 1.70× the base's for 1.71× its groups; all
  four programs lie on one line, 0.15 ns + 0.206 ns a group, a pixel. At 614.4 MHz that is
  about eight pixel-groups a clock, and almost nothing per pixel outside the program.
- **H17 refuted.** Without the ISP feedback, translucent instead of punch-through, the pass
  costs its one group less (−1.5 %), nothing more.
- **H18 confirmed.** Both sentinels kept — the one a pixel above the quad in its own tile
  proves the back end's clip pixel-precise, the far one the region clip; the limited pass
  420 µs against 2547 µs over the whole plane.
- **H19 refuted — but not by the heap.** gpud used 87 KiB of its 384 KiB. The boot stopped at
  the wallpaper: the cursor upload's mapping failed, `VM-MAP-FAIL reason=table-full regions=64
  peak=64`, then `gpud: FAIL dc cursor layer (block: no contiguous block for the controller)`,
  and the session never started (`FAIL stage wait svc=sessiond stage=session-start`). Every
  buffer is a region of gpud's address space, which holds 64; the cost rung's four transient
  buffers took the last ones. The rungs now draw only in the bundle's buffers' tails, split
  between the quads rung (lower halves) and the cost rung (upper halves) — no rung creates a
  buffer of its own.

**Verdict 5c-1.** A pass costs its damage and its program. The device maps buffers by the open
driver's flags and limits a pass to its damage tiles (pixel-precise at the back end); what is
left per pixel is the fragment program: 0.21 ns an instruction group. The open compiler spends
55–68 groups on a textured quad — the helper-lane prologue, the coordinate zeroing, the
alpha-to-coverage path, the tile read-back for its sample check — where the work itself (iterate,
sample, tint, blend, pack, emit) is a fraction; the stock stack's opaque fill at 1.8 ns a pixel is
about eight groups on this line. So 5c-2's pipelines cannot be the open compiler's output as it
stands. Their programs must be written for this core — by an encoder of ours against the open
compiler's published instruction description, or by the open compiler with that overhead taken
out — each with its group count as its budget and its ns a pixel measured against the stock's;
which of the two is 5c-2's first decision.
Carried: gpud's address space holds 64 regions and the rungs plus the display use nearly all —
the frame path needs buffers that the GPU alone maps and buffers that end (the memory lane's).

**Results — the board, third cycle** (`cycle13-gpu-lines.txt`, `cycle13-board-test.txt`; the
rungs in the bundle's tails, no buffer of their own): the desktop after the rungs, the cursor
layer up (`gpud: dc cursor layer ok`), the session shell visible; the operator confirmed all
eight points on this boot; `[PASS] board-visible: 60 rungs, 7 operator marker(s), no FAIL
marker, manifest clean`. The lines repeat the second cycle's to a few microseconds — `gpu area
ok (us=410 full_us=2538 … near=kept far=kept)`; `base` 15 244 µs, `chain` 25 608, `eft` 15 019,
`opaque-eft` 11 825; `gpu heap (used=87418 of=393216)`. A boot in between (the third cycle's
first) ended with the operator re-plugging the mouse's receiver: `xhcid: hid interface lost`
there is the unplug (transaction error, then the receiver enumerated again), not the driver.

## 5c-2 — our own programs on the board (TASK-0215 5c-2 P3b; host and board 2026-10-15)

**Question.** Our encoder (`nexus_gfx::shader::rogue`, every encoding a golden against the open
compiler's) can write the textured layer the short way: does the hardware run it, and does a
pixel then cost what its groups say?

**Instrument.** The cost rung's program probes, a 1024² quad limited to its area each: the
compiler's `base` and `opaque-eft` as controls, then the reference programs gpud assembles at
run time (`nexus_gfx::shader::rogue::reference`) — each written into the code arena's tail,
named by a kick word carrying its own temporaries, its pass type set in the draw's state block,
its sample checked: `opaque-ditr` (iterate, fence, sample, fence, pack into the pixel output —
5 groups, pass type opaque), `opaque-ditr-x3` (the same quad three times on itself), `tinted`
(+ one multiply repeated over four channels, the tint (1, 0.5, 0.25, 1)), `premultiplied` (the
tile read, unpacked, `src + dst·(1 − a)` with the tint 0.5 — 13 groups, translucent),
`opaque-itrsmp` and `opaque-itrsmp-count` (iterate-and-sample in one word, six- and seven-byte
forms — 3 groups). The setup programs are the bundle's: W at cf0, uv at cf4, texture state at
sh4, sampler state at sh12, tint at sh24.

| # | Hypothesis | Decision line |
|---|---|---|
| H21 | A textured layer costs its few groups: 0.15 ns + 0.206 ns a group, a pixel | `opaque-ditr` near 1.2 ns, `opaque-itrsmp` near 0.8 ns a pixel, samples right |
| H22 | The premultiplied blend runs from the tile read and costs its groups | `premultiplied`'s sample right; its cost on the line |
| H23 | The opaque pass type lets the tile hardware drop hidden opaque pixels before shading | `opaque-ditr-x3` about `opaque-ditr`'s cost, not three times its fragment work |
| H24 | A 2D sample reads only its two coordinates (the compiler's fourteen zeroing moves are not needed) | `opaque-ditr`'s sample right |
| H25 | A repeated multiply steps every register operand, the shared one included | `tinted`'s sample reads (40, 45, 50) |
| H26 | A pack may write the pixel output directly and end the program | every reference's sample right |
| H27 | Iterate-and-sample addresses its state as iterate and sample do (offsets in fours) | `opaque-itrsmp`'s or `-count`'s sample right; a lost pass names the form |

**Results — the board, cycle 14** (`cycle14-gpu-lines.txt`, `cycle14-board-test.txt`; one 1024²
pass each, limited to its area; per pixel = the pass less the same boot's 16² limited pass,
417 µs, over 1 048 576 pixels; every sample right; `[PASS] board-visible`, the operator's eight
confirmations, the desktop after the rungs):

| Program | Groups | Pass type | Pass | Per pixel | Sample |
|---|---|---|---|---|---|
| `base` (compiler, control) | 68 | punch-through | 15 250 µs | 14.15 ns | right |
| `opaque-eft` (compiler, control) | 55 | translucent | 11 821 µs | 10.88 ns | right |
| `opaque-ditr` | 5 | opaque | 1 826 µs | **1.34 ns** | right |
| `opaque-ditr-x3` (three on each other) | 5 | opaque | 1 916 µs | 1.43 ns | right |
| `tinted` | 6 | opaque | 2 479 µs | 1.97 ns | right (40, 45, 50) |
| `premultiplied` | 13 | translucent | 5 124 µs | **4.49 ns** | right |
| `opaque-itrsmp` | 3 | opaque | 1 782 µs | **1.30 ns** | right |
| `opaque-itrsmp-count` | 3 | opaque | 1 751 µs | 1.27 ns | right |

- **H21 confirmed.** Our opaque textured layer costs 1.3 ns a pixel — below the stock stack's
  opaque fill (1.8 ns) and an eighth of the compiler's opaque program. A repeated group costs
  its repetitions: `tinted`'s one extra group (a multiply over four channels) adds 0.63 ns, four
  executions on the 5c-1 line; `premultiplied` is 22 executions, 4.5 ns.
- **H22 confirmed.** The premultiplied blend from the tile read draws right at 4.5 ns a pixel —
  a third of the compiler's blended program (14.2 ns); its unpack, multiply-adds and pack are
  where the next groups are.
- **H23 confirmed.** Three opaque layers on each other cost one: 1 577 µs of fragment work
  against 1 566 µs; only the geometry grows (339 µs against 260). With the opaque pass type the
  tile hardware drops hidden pixels before shading — the compiler's programs, all translucent
  or punch-through, never get this.
- **H24 confirmed.** The 2D sample needs only its two coordinates: the compiler's fourteen
  zeroing moves are not needed.
- **H25 confirmed.** A repeated multiply steps every register operand, the shared one included.
- **H26 confirmed.** A pack writes the pixel output directly and ends the program.
- **H27 confirmed.** Iterate-and-sample works in both forms with offsets in fours — the first
  use of this word by any program we know of; three groups for a textured layer.

**Verdict 5c-2 P3b.** The hardware runs our encoder's programs as written, and a pixel costs
what the program asks: the textured layer at 1.3 ns opaque (free under further opaque layers)
and 4.5 ns blended. The compositor's pipelines are now a question of program length, which is
ours. P3c makes the backend emit these programs from the IR, the reference programs its goldens.

## 5c-2 P3c — the toolchain's pipelines on the board (TASK-0215 5c-2 P3c; board 2026-10-15)

**Question.** The backend now compiles `resources/gpu/shaders/layer.wgsl` into the programs P3b
measured, byte for byte (`tools/nxgpu/tests/pipelines.rs`). Do the compiled pipelines draw right
from their files at their references' cost, and is the opaque layer below the stock stack's
opaque fill?

**Instrument.** The cost rung's pipeline gate (it replaces P3b's probes): `layer-opaque.usc`
(`tinted=false`, replace — 3 groups, opaque; byte for byte `opaque-itrsmp`) and
`layer-premultiplied.usc` (the tinted default, premultiplied over — 11 groups, translucent; byte
for byte `premultiplied-itrsmp`: iterate-and-sample feeding the tile read's epilog, never run
before), each written into the code arena's tail, named by a kick word with the temporaries the
compiler gave it, its pass type in the draw's state block; a 1024² quad limited to its area, the
tint white and 0.5. The bar was measured before the cycle: cycle 14's tinted opaque program
(1.97 ns) lies above the stock's 1.8 ns — its tint multiply is a third of that pixel — so the
opaque pipeline is the untinted variant (`layer.wgsl`'s override `tinted`), the program cycle 14
measured at 1.30 ns.

| # | Hypothesis | Decision line |
|---|---|---|
| H28 | The toolchain's opaque layer runs as the reference it equals byte for byte | `layer-opaque`: sample right, 1.30 ± 0.05 ns a pixel |
| H29 | Iterate-and-sample feeds the premultiplied epilog (tile read, unpack, fused multiply-add, pack) as iterate and sample did | `layer-premultiplied` (11 groups): sample right, at most the unfused 13 groups' 4.49 ns |

**Results — the board, cycle 15** (`cycle15-gpu-lines.txt`, `cycle15-board-test.txt`; per pixel =
the pass less the same boot's 16² limited pass, 414 µs, over 1 048 576 pixels; `[PASS]
board-visible`, 61 rungs, the operator's eight confirmations, the desktop after the rungs):

| Pipeline | Groups | Pass type | Pass | Per pixel | Sample | Its reference, cycle 14 |
|---|---|---|---|---|---|---|
| `layer-opaque` | 3 | opaque | 1 782 µs | **1.30 ns** | right | `opaque-itrsmp`: 1 782 µs, 1.30 ns |
| `layer-premultiplied` | 11 | translucent | 4 653 µs | **4.04 ns** | right | `premultiplied` (unfused, 13 groups): 5 124 µs, 4.49 ns |

`gpud: gpu pipeline ok (opaque_ns_px=1.30 premultiplied_ns_px=4.04 samples=2/2 bar=1.80)`;
gpud's heap after the rungs 82 550 bytes against cycle 14's 110 698 (no program assembled at run
time any more).

- **H28 confirmed.** The toolchain's opaque layer runs exactly as its reference — the same pass
  time to the microsecond, 1.30 ns a pixel, 28 % below the stock's opaque fill.
- **H29 confirmed.** Iterate-and-sample feeds the blend's epilog: the sample is right, and the
  fused program costs 4.04 ns against the unfused 4.49 — the two groups it drops are 0.41 ns on
  the 5c-1 line, 0.45 ns measured. The same fusion gained the opaque layer only 0.04 ns (5 → 3
  groups, cycle 14): a program of three groups is bound by something other than its groups (the
  texture fetch or the pixel output — not yet taken apart).
- **The gate against the measurement.** Red on the measured failure — cycle 14's tinted opaque
  program (1.97 ns) is above the bar — and green on the measured success (1.30 ns).

**Verdict 5c-2 P3c.** WGSL, our IR, our backend, a file, the board GPU — and the picture right:
the compositor's opaque layer at 1.30 ns a pixel, below the stock's 1.8 ns; the blended layer at
4.04 ns. **P3 complete.** Carried: the blended layer against the stock's first blended layer
(3.1 ns) — the epilog's unpack, multiply-adds and pack are where its groups are; P4 brings the
library with our own setup programs.

## 5c-2 P4 — the pipeline library on the board (TASK-0215 5c-2 P4; board 2026-10-15)

**Question.** The device now takes the compositor's pipeline library (`compositor.nxgp`, RFC-0101
§7b) and draws with what it placed: programs untouched in the code heap, our own setup programs
in the setup heap — the kick placed once with its program's offset, the varying iteration, a
state load of two loads (the open driver's carried four: its descriptor at sh28 and a constant
at sh0 belong to its descriptor sets, which our programs never read) with its own size word.
Do the pipelines draw as they did through the captured setup programs, and do setup programs
placed once serve every later pass?

**Instrument.** The new library rung (`gpud: gpu library ok (`) after the render rung; then 5b's
four probes (one, one again, two, three 512² quads, whole-plane passes), the area gate and the
pipeline gate — all with the library's pipelines: 5b and the area gate with the premultiplied
layer, the pipeline gate with both. The captured fragment programs remain only in 5a's replay
(the control).

| # | Hypothesis | Decision line |
|---|---|---|
| H30 | Our setup programs drive the pipelines as the captured ones did | `gpu library ok (… refused=0 …)`; `gpu pipeline ok (samples=2/2 …)` at 1.30 and 4.04 ± 0.05 ns |
| H31 | Setup programs placed once and never rewritten serve every later pass (H10 forbids rewriting, not rereading) | 5b's four probes, the area gate's two passes and the pipeline gate's two all right |
| H32 | Our programs read neither the open driver's descriptor (sh28–30) nor its constant (sh0) | no sample changes for dropping those two loads |

**Results — the board, cycle 16** (`cycle16-gpu-lines.txt`, `cycle16-board-test.txt`; `[PASS]
board-visible`, 62 rungs, the operator's eight confirmations, the desktop after the rungs):

- `gpud: gpu library ok (bytes=624 entries=2 refused=0 code_bytes=152 setup_bytes=136 ids=1,2)`.
- The pipeline gate: `layer-opaque` 1 771 µs, **1.29 ns** a pixel; `layer-premultiplied` 4 643 µs,
  **4.03 ns** — both samples right (cycle 15: 1.30 and 4.04 through the captured setup).
- 5b, three blended 512² quads in whole-plane passes, now with our 11-group pipeline where it
  had the open compiler's 68-group program:

| Probe | Cycle 15 (the compiler's program) | Cycle 16 (the library's pipeline) |
|---|---|---|
| one quad | 5 954 µs | 3 304 µs |
| two quads | 9 439 µs | 4 149 µs |
| three quads (the gate, samples 3/3) | **12 949 µs** | **4 972 µs** |
| each further quad | ≈ 3 500 µs | ≈ 830 µs |

- The area gate as before (`us=414 full_us=2539 … inside=ok near=kept far=kept`); 5a's replay of
  the captured pass, the control, unchanged (6 013 → 5 995 µs); gpud's heap after the rungs
  75 446 bytes (cycle 15: 82 550 — no captured fragment blocks read, no program copies per pass).

- **H30 confirmed.** Every entry accepted, both pipelines at their cycle-15 cost to a hundredth of
  a nanosecond, both samples right: our kick, iteration and state load run as the captured ones.
- **H31 confirmed.** Every pass after the library's placing — 5b's four, the area gate's two, the
  pipeline gate's two — drew right from setup programs written once.
- **H32 confirmed.** No sample changed for the two loads not carried.

**Verdict 5c-2 P4.** The pipeline library is the device's way in for GPU code: a client's
archive parsed, checked entry by entry and placed, its pipelines named by id, gpud holding and
patching no program of its own. The whole fragment half of a pass is now ours — program and
setup — and a scene of blended quads drew 2.6 times faster for it. Carried: the client's supply
over IPC (5c-3, with windowd's own context), the vertex pipeline (the captured vertex program and
fetch remain), the blended layer against the stock's 3.1 ns.
