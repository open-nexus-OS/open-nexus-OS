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

## 5c-2 P5a–P5c — the layer's corners, straight surfaces and the shadow on the board (board 2026-10-15)

**Question.** A live frame needs the layer with opacity and rounded corners over a client's
straight-alpha surface and the window's drop shadow (measured from windowd's scene, gpud's present
and the virgl shaders, ledger 5c-2 P5). The backend grew the arithmetic for them — the
single-source unit, min and max through the test and conditional move, the modifiers, the
hardware's constant registers, vector construction — each form a golden from the open compiler's
own code (`resources/gpu/programs/frag-ops.usc`), each variant's picture a golden of the
interpreter against the virgl formula. Do the compiled programs draw those pictures on the board,
and what does a pixel cost?

**Instrument.** The pipeline gate as a probe table: the four layers on the 1024² quad limited to
its area — opaque, premultiplied, straight at opacity 0.75, rounded with radius 64 (its interior
and its corner pixel read) — and the drop shadow of a 512x384 window with radius 12 (blur 18,
offset (0, 5), black at alpha 90/255), read under the window's centre and six pixels below its
bottom edge on the vertical axis through it (where the formula needs no square root); each cost a
pixel over its own quad.

| # | Hypothesis | Decision line |
|---|---|---|
| H33 | The new arithmetic runs as encoded | `layer-straight`, `layer-rounded` and `shadow` read right (`samples=7/7`) |
| H34 | An input sampled inside iterate-and-sample and iterated again for arithmetic is the same coordinate | the rounded corner pixel at the clear, the interior the texel; the shadow where the formula puts it |
| H35 | A pixel's cost follows its executions | `layer-straight` ≈ 0.8 ns above `layer-premultiplied` |

**Results — the board, cycle 17** (`cycle17-gpu-lines.txt`, `cycle17-board-test.txt`; `[PASS]
board-visible`, 62 rungs, the operator's eight confirmations; per pixel = the pass less the same
boot's 16² limited pass, 413 µs, over the probe's quad):

| Pipeline | Groups | Pixels | Pass | Per pixel | Reads |
|---|---|---|---|---|---|
| `layer-opaque` | 3 | 1 048 576 | 1 774 µs | 1.29 ns | right |
| `layer-premultiplied` | 11 | 1 048 576 | 4 649 µs | 4.03 ns | right |
| `layer-straight` | 15 | 1 048 576 | 5 529 µs | **4.87 ns** | right |
| `layer-rounded` | 34 | 1 048 576 | 9 702 µs | **8.85 ns** | right (interior, corner at the clear) |
| `shadow` | 43 | 230 160 | 2 903 µs | **10.81 ns** | right (knocked out under the window, the shadow below) |

`gpud: gpu library ok (bytes=2252 entries=5 refused=0 …)`; 5b's three quads in 4 970 µs as in
cycle 16. gpud's heap after the rungs 120 177 bytes (cycle 16: 75 446): eleven passes where there
were eight, each allocating its emitter's buffers, and the table decoding every program whole to
count its groups at load — on a heap that never frees.

- **H33 confirmed.** Every read right on the first cycle: the single-source unit, min and max,
  the modifiers, the constant registers and the vector moves run as the compiler's own forms do.
- **H34 confirmed.** The rounded layer leaves its corner pixel at the clear and draws its
  interior; the shadow is knocked out under the window and shows below it at the formula's alpha.
- **H35 confirmed.** The straight layer costs 0.84 ns a pixel more than the premultiplied one for
  its four more executions; the rounded layer 8.85 ns, the shadow 10.81 ns.
- The known board red `KSELFTEST: ipc call budget FAIL (rt=65us budget=64)` appeared in this boot
  (cycle 16: `ok (rt=51us)`): the boot-time roundtrip's spread, handed to S3, allow-listed.

**Verdict 5c-2 P5a–c.** The compositor's live operations but the blur run as our own programs on
the board, pixel for pixel the QEMU path's pictures. The distance costs: a rounded layer is 8.85
ns a pixel, so the frame (5c-3) draws only a layer's corner squares with it (576 pixels a window
at radius 12) and the rest with the straight layer (4.87 ns); the shadow's 10.81 ns are paid on
its ring outside the window only — the knocked-out inside is not drawn at all. Before 5c-3 the
emitter must stop allocating per pass (gpud's heap never frees).

## 5c-2 P5d — the glass blur on the board (host 2026-10-15; board cycle 18)

**Question.** The glass layer blurs what lies beneath it: the virgl path's separable gaussian
(σ = R/2, taps out to ±R, radius 20 for windowd's glass) with the CPU path's saturation (140 %).
Our toolchain builds it at half resolution (`resources/gpu/shaders/blur.wgsl`): the horizontal
pass reads the full-resolution backdrop a tap every two pixels, each one bilinear fetch at the
corner of four texels (the downsample); the vertical pass reads that result two taps a fetch and
lifts the saturation; the rounded layer samples the result back over the glass. Every loop is
unrolled while lowering, the weights are the pipeline's constant block (DMA'd by the state load
from the setup heap), and every texture is read through our own descriptor. On the host the chain
is the full-resolution formula within three levels (`tools/nxgpu/tests/blur.rs`). Does the board
draw it, and what does a pixel cost?

**Instrument.** The pipeline gate's last probe (`render_blur`): the CPU writes a backdrop into the
plane (red stripes 16 pixels wide, a green ramp down, a blue one across) over a 256x192 glass at
(512, 96) and 32 pixels around it; the horizontal pass (`blur-horizontal-20`, 89 groups) renders
into rows below the picture (128x116 at (512, 640)), the vertical pass (`blur-vertical-20`, 70
groups) beside it (128x96 at (800, 640)), the rounded layer (radius 16) over the glass — each
limited to its quad, the first two reading the plane through a descriptor of the whole plane,
the layer through one of the vertical result's rectangle. Four reads inside the glass against the
formula computed by gpud from the same backdrop (tolerance six levels), the corner pixel at the
pass's clear; each pass's cost a pixel over its own output.

| # | Hypothesis | Decision line |
|---|---|---|
| H36 | The state load's third DMA brings the constant block from the setup heap into the shared registers the program reads | the reads within six levels (`blur_dev=`); a block not loaded reads as zero weights — black |
| H37 | A plane is a stride texture through our descriptor, its words blue-green-red-alpha by the swizzle | the reads' red and blue where the formula puts them, not swapped |
| H38 | A computed coordinate's implicit level of detail on a one-level texture samples level 0 bilinear, so the corner fetch averages its four texels | the reads within the tolerance; a wrong level or filter blurs or aliases the stripes off it |
| H39 | A pass reads what the pass before it wrote into the plane (no stale texture-cache lines between jobs) | the vertical pass's and the layer's sources right, so the final reads are |
| H40 | A sampling pass costs its groups on the 5c-1 line (0.15 ns + 0.206 ns a group) | `blur-horizontal-20` ≈ 18.5 ns, `blur-vertical-20` ≈ 14.6 ns a half-resolution pixel |

**Results — the board, cycle 18** (`cycle18-gpu-lines.txt`, `cycle18-board-test.txt`; `[PASS]
board-visible`, 62 rungs, the operator's eight confirmations; per pixel = the pass less the same
boot's 16² limited pass, 415 µs, over the pass's own output):

| Pass | Groups | Output pixels | Pass | Per pixel | Reads |
|---|---|---|---|---|---|
| `blur-horizontal-20` | 89 | 14 848 (128x116, half resolution) | 993 µs | **38.92 ns** | — |
| `blur-vertical-20` | 70 | 12 288 (128x96, half resolution) | 700 µs | **23.19 ns** | — |
| `layer-rounded` over the glass | 34 | 49 152 (256x192) | 841 µs | 8.66 ns | four inside within **2 levels** of the formula, the corner at the clear |

`gpud: gpu pipeline ok (… blur_h_ns_px=38.92 blur_v_ns_px=23.19 blur_layer_ns_px=8.66
blur_dev=2 samples=12/12 bar=1.80)`; `gpud: gpu library ok (bytes=4748 entries=7 refused=0
code_bytes=2808 setup_bytes=852 …)`; the earlier pipelines at their cycle-17 costs to the hundredth.
gpud's heap after the rungs 191 250 bytes (cycle 17: 120 177): three passes more, each allocating its
emitter's buffers, and the two largest programs decoded whole at load — on a heap that never frees.
`KSELFTEST: ipc call budget ok (rt=53us)` this boot.

- **H36 confirmed.** The state load's third DMA brings the constant block from the setup heap:
  the weights are where the program reads them (zero weights would have read black).
- **H37 confirmed.** The plane is a stride texture through our descriptor; red and blue lie where
  the formula puts them.
- **H38 confirmed.** A computed coordinate samples level 0 bilinear on a one-level texture, and
  the corner fetch averages its four texels: the reads lie within two levels of the
  full-resolution formula (the host model's bound is three).
- **H39 confirmed.** The vertical pass and the layer read what the pass before them wrote into the
  plane, on the first cycle — no stale texture-cache line between jobs.
- **H40 refuted.** A sampling pass costs more than its groups: the horizontal pass 38.9 ns a
  half-resolution pixel against the line's 18.5, the vertical one 23.2 against 14.6 — about 1.85
  and 2.1 ns a fetch (21 and 11 fetches). The fetches carry these passes, not the arithmetic: the
  texture unit's rate, the horizontal pass's fetches two texels apart each touching fresh lines.

**Verdict 5c-2 P5d.** The glass blur runs as our own programs on the board within two levels of
the virgl path's full-resolution blur, on the first cycle: the loops unrolled in the frontend, the
weights in the pipeline's constant block, every texture read through our own descriptor. Its cost
is the texture unit's — about 2 ns a fetch: the two passes come to 17.6 ns a glass pixel
((578 + 285) µs over 49 152 pixels), so a 1920x80 bar costs some 2.7 ms to blur. Against the
stock stack's bar (one 21-tap blur pass over 1080p in 42.8 ms, `2026-10-10-gpu-stock-baseline/`)
our two passes would blur the whole plane in about 33 ms (20.9 + 12.0) — the stock's two take
some 86. The frame
therefore blurs only where the backdrop under a glass changed and keeps the result (the blur
cache of 5d), and a cheaper kernel is measured against this line before it is built (a separate
downsample with the pair trick in both directions fetches 23 times a half-resolution pixel instead
of 32). Before 5c-3: the emitter without allocation per pass, the library load without decoding
every program whole (191 KB of 384 KB used).

## Before 5c-3 — the device allocates nothing per pass (host 2026-10-16; board cycle 19)

**Question.** gpud's heap never frees (a bump allocator), and the frame will emit passes for
gpud's whole life. Cycle 18 left 191 KB of 384 KB used after the rungs. Measured on the host with a
counting allocator: the library load took 108 KB — 101 KB of it every program decoded whole into
groups only to count them — and each pass its emitter's buffers and three stream vectors. After
the change (an instruction group holds its instructions in place, the table counts groups as it
decodes them, the emitter writes every block through a sink as it makes it, the state-load data
and the command streams are fixed buffers) the host shows zero allocations while emitting a pass,
counting a program, writing its streams and its state data, and a load that keeps 7 KB. Does the
board agree?

**Instrument.** Two fields the rungs now print: `gpud: gpu library ok (… table_bytes=)` — the heap
the table load took — and `gpud: gpu heap (used= of= pass_bytes=)` — the most any pass of the area
and pipeline gates took (the heap cursor before the pass's emission and after its wait); every
earlier read and cost as the comparison.

| # | Hypothesis | Decision line |
|---|---|---|
| H41 | The table load keeps only each pipeline's placement and setup | `table_bytes` near the host's 7 KB, not 108 KB |
| H42 | A pass takes nothing of gpud's heap | `pass_bytes=0` |
| H43 | The passes are the same bytes as before | every read right and every cost a pixel as in cycle 18 (± noise); the heap after the rungs well below 191 KB |

**Results — the board, cycle 19** (`cycle19-gpu-lines.txt`, `cycle19-board-test.txt`; `[PASS]
board-visible`, 62 rungs, the operator's eight confirmations):

`gpud: gpu library ok (… table_bytes=7039 …)`; `gpud: gpu heap (used=64666 of=393216
pass_bytes=0)`; `gpud: gpu pipeline ok (opaque_ns_px=1.30 premultiplied_ns_px=4.03
straight_ns_px=4.86 rounded_ns_px=8.86 shadow_ns_px=10.81 blur_h_ns_px=39.53 blur_v_ns_px=23.03
blur_layer_ns_px=8.70 blur_dev=2 samples=12/12 bar=1.80)`.

- **H41 confirmed.** The table load takes 7 039 bytes — the host's 6 990 and the placements' own
  alignment — where it took 108 KB: each pipeline's placement and setup, no program kept.
- **H42 confirmed.** No pass of the area and pipeline gates takes a byte of gpud's heap.
- **H43 confirmed.** Every read right, every cost a pixel as in cycle 18 to the noise (the blur's
  horizontal pass 39.5 against 38.9 ns, its vertical one 23.0 against 23.2); the heap after the
  rungs 64 666 bytes where cycle 18 left 191 250.
- Seen in this boot, not this package's: the early rungs ran beside the USB controller's PHY
  bring-up — `gpu fence ok (rt_us=446 worst_us=15296 … irqs=63)` (cycle 18: 83 µs, 97 µs worst) and
  the allow-listed `KSELFTEST: ipc call budget FAIL (rt=1251us)`; the fence rung's spread has a
  precedent (`board--2026-10-14T17-18-54`: 502 µs, 22 ms worst) and goes with the boot's
  contention to S3.

**Verdict.** The device is ready for a frame that emits passes for gpud's whole life: a pass takes
nothing of the heap, a context's library what its table keeps (7 KB for seven pipelines).

## 5c-2 P6 — the board after the QEMU path's library (board cycle 20)

**Question.** P6 put the QEMU path on the library (TASK-0215 5c-2 P6): a new backend
(`shader::tgsi`), the frontend's vertex stage, gpud's virgl modules — none of it runs on the board
— and one change the board does run: the layer's push block grew to 64 bytes (a fourth vec4
naming a rectangle of the texture for the QEMU path's atlas variant; `Quad::layer` writes it as
the whole texture), so the board's library was rebuilt (4 748 bytes, `0x04c43695d21690f9`, its
programs unchanged) and the layer pipelines' state loads DMA four more dwords a draw. Does the
board read and cost as in cycle 19?

**Instrument.** The rungs' lines as in cycle 19: `gpu library ok (…)`, every `gpu cost (…)` line
and `gpu pipeline ok (…)`, `gpu heap (…)`.

| # | Hypothesis | Decision line |
|---|---|---|
| H44 | The rebuilt library loads and every pipeline reads right | `gpu library ok (bytes=4748 entries=7 refused=0 …)`; every cost line `reads=ok`; `samples=12/12`, `blur_dev` ≤ 2 |
| H45 | Four more push dwords a draw cost nothing a pixel | every `ns_px` of the pipeline line within 3 % of cycle 19 |
| H46 | The QEMU path's code stays off the board | no new or missing line against cycle 19; `table_bytes` within a few bytes of 7 039; the heap after the rungs near 64.7 KB |

**Results — the board, cycle 20** (`board--2026-10-16T12-47-10`, `cycle20-gpu-lines.txt`,
`cycle20-board-test.txt`; `[PASS] board-visible`, 62 rungs, the operator's eight confirmations):

`gpud: gpu library ok (bytes=4748 entries=7 refused=0 code_bytes=2808 setup_bytes=852
table_bytes=7039 …)`; `gpud: gpu pipeline ok (opaque_ns_px=1.27 premultiplied_ns_px=4.03
straight_ns_px=4.87 rounded_ns_px=8.86 shadow_ns_px=10.83 blur_h_ns_px=37.98 blur_v_ns_px=23.51
blur_layer_ns_px=8.62 blur_dev=2 samples=12/12 bar=1.80)`; `gpud: gpu heap (used=64402 of=393216
pass_bytes=0)`.

- **H44 confirmed.** The rebuilt library loads whole, every cost line `reads=ok`, the pipeline
  rung's twelve samples right, the blur within two levels of its formula.
- **H45 confirmed.** The layer pipelines — the ones whose state load grew by four dwords — cost
  as in cycle 19 to a hundredth of a nanosecond (1.27/4.03/4.87/8.86 against 1.30/4.03/4.86/8.86,
  the rounded layer over the glass 8.62 against 8.70). The blur passes, whose push block did not
  change, moved by −3.9 % (horizontal, 37.98 against 39.53) and +2.1 % (vertical): their fragment
  time is 752 µs against 762, a pixel's share computed after the pass's fixed cost — the small
  passes' spread between cycles (cycle 18 had 38.92).
- **H46 confirmed.** The same 26 lines as cycle 19, none new or missing; `table_bytes` 7 039 as
  before; the heap after the rungs 64 402 bytes (cycle 19: 64 666). The fence rung is back at
  83 µs (worst 92) — cycle 19's spread went with that boot's USB bring-up.

**Verdict.** The board is unchanged by the QEMU path's move onto the library: one set of WGSL
sources now feeds both GPUs' libraries, and the layer's larger block costs the board nothing.

## 5c-3 P1 — the mixed pass (host 2026-10-16; board cycle 21)

**Question.** The frame (TASK-0215 5c-3) draws its wallpaper opaque and its layers translucent,
each with its own texture, into a ring slot or a scratch, pass after pass for gpud's whole life.
Three things a pass could not do: mix pipelines and textures (one pipeline and one descriptor a
pass), name its target (the captured end-of-tile program writes the captured plane), reuse its
memory (every pass bumped into fresh lines, H10). Read on the host first: the end-of-tile program
(code heap `0x7e80`, 40 bytes) is two moves of an immediate — the back end's state word 1
(`0x00082020`: the address's bits 34–39 beside the stride and the format) and state word 0
(`0x0002c400`: the address's bits 2–33) — and the emit; the pixel event data (PDS heap `0x2980`)
names it. The open kernel driver issues no cache invalidation; the open userspace driver reuses
its transient memory once its job is done.

**Instrument.** `gpud: gpu mixed ok (draws= targets= reads= mixed_us= ring_passes= ring_reads=
reused= stale= heap_bytes=)`: one pass of three draws (opaque over the checker, straight over a
colour block the CPU wrote — its own descriptor —, rounded over the checker; the pass type
changes at the second draw, whose block re-states the ISP words) into the plane through a copy
of the end-of-tile program, and into the plane 540 rows down through another; then 48 passes of
one to three draws, each draw its own tint, through small rings in the tails earlier rungs drew
in — each pass reached before the next begins, so every pass writes the very lines the pass
before read (`reused=`) — every sample read after its pass. Beside it `gpud: dc present mix (…)`: what the desktop's
presents ask for, by kind (layers, glass, shadowed, copies, blurs; their pixels).

| # | Hypothesis | Decision line |
|---|---|---|
| H47 | A pass of mixed draws runs as emitted: each draw its pipeline, its pass type (the ISP words where it changes), its own texture | the three samples and the corner right in the first target |
| H48 | A copy of the end-of-tile program with another address sends the pass's tiles there | the four reads right 540 rows down; the first target through its own copy right as well |
| H49 | A line read by a pass the timeline has reached is read fresh when later passes write it again (the open userspace driver reuses its transient memory so). 5b lost a draw when a pass rewrote, past the old `TERMINATE`, the stream the waited pass before had read — the ring's wrap is that case, so this cycle decides between the two | 48 ring passes, every draw's tint right (`stale=0`), 47 of them on the lines the pass before read (`reused=47`); refuted (a draw lost or a tint of an earlier pass), the frame's rings need a cache invalidation before reuse |
| H50 | The rings take nothing of gpud's heap | `heap_bytes=0` |

**Cycle 21 — red, the cause named** (`board--2026-10-16T15-01-02`, `cycle21-gpu-lines.txt`,
`cycle21-board-test.txt`; the operator's eight confirmations; `[FAIL] board-visible` on the
rung's FAIL form): `gpud: FAIL gpu mixed (mixed pass at row 540: emit Space)`. The first target's
pass ran and read right — **H47 confirmed** (three pipelines, the pass type changing at the
second draw, the colour block through its own descriptor, the rounded layer's corner at the
clear), and the first target went through a copy of the end-of-tile program (H48's first half).
The second pass found no room: the rings kept their head where the first pass ended, five of
the vertex ring's seven lines in, and a block never wraps — the empty ring refused a five-line
block its tail could not hold. Fixed on the host: an emptied ring starts again from its first
line (`an_emptied_ring_holds_a_block_its_tail_would_not`), which also makes every ring pass write
the lines the pass before read — the hardest case for H49. Every other line as in cycle 20.
Measured beside it — what the desktop asks for (`gpud: dc present mix (…)`, eleven windows):
3–9 layers a present covering about one screen (2.1–2.4 M pixels: the desktop's band and the
windows), 1–7 of them glass but only 70–300 K glass pixels, 1–4 with a shadow, 2–9 copies of
0.3–1.2 M pixels, a blur for every glass layer over the glass's pixels — the blur cache pays for
itself, the frame's cost is the layers.

**Results — the board, cycle 22** (`board--2026-10-16T15-10-07`, `cycle22-gpu-lines.txt`,
`cycle22-board-test.txt`; `[PASS] board-visible`, 63 rungs, the operator's eight confirmations):

`gpud: gpu mixed ok (draws=3 targets=2 reads=8/8 mixed_us=14994 ring_passes=48 ring_reads=96/96
reused=49 stale=0 heap_bytes=0)`; every other line as in cycle 20.

- **H47 confirmed** (cycle 21 already): one pass, three pipelines, the pass type changing at the
  second draw, each draw its own texture.
- **H48 confirmed.** The same pass 540 rows down, through the second copy of the end-of-tile
  program, read right — a pass names its target by the program its pixel event kicks.
- **H49 confirmed.** 49 passes wrote the very lines the pass before had read once it was reached,
  and all 96 tints read right: the GPU keeps no data line of a finished pass. 5b's lost draw
  narrows to what it rewrote besides the stream — per-draw program copies; programs stay placed
  once and never rewritten (H31), data lines are reused once their pass is reached.
- **H50 confirmed.** The rings took nothing of gpud's heap.
- Measured, not decided here: the two mixed passes took 15.0 ms — about 7.5 ms a pass of three
  draws over a 1156x256 area, where the pipeline gate's per-pixel costs predict about 1.3 ms. P2's
  first cycle splits it (geometry and fragment stage per pass, the pass type change, the first
  pass into a target).

**Verdict.** A pass can now be a frame's pass: mixed pipelines and textures, any target of the
plane's stride, its memory reused as soon as the timeline says the pass that read it is done.


## 5c-3 P2a/P2b — memory for many contexts, and several contexts (host 2026-10-16; board cycle 23)

**Question.** The frame wants windowd's own context beside gpud's, and every production GPU
stack gives each client its own GPU address space and takes everything back when the client
goes (the operator, 2026-10-16: "production grade, as Fuchsia would do it"). What stood in the
way, read from the code (ledger "P2 — measured first"): a buffer was a region of gpud's address
space (64; the render bundle alone took 36), the firmware heap's map a table of 128 ranges, its
backing pool and the page tables a bump allocator, the free lists' parameter pages CPU-mapped,
no cleanup command at all, one context whose destruction dropped its client's every buffer. The
open kernel driver: a VM context (catalogue + firmware memory context, the catalogue register
set left to the firmware) under each client's contexts; the dataset and the free lists their own
objects; queues cleaned up first, then the render-target data, then the free lists (a busy free
list retried after the firmware's messages), the memory context last.

**Instrument.** P2a on the host: a binding is a range of a buffer (`Mapping`, driverkit's
`Binding.offset`), a buffer is destroyed by its client once nothing binds it, the firmware heap
is a page bitmap, the page tables hand their slot storage back (`TableStorage`), the cleanup
command encodes kind and address — the mock and the encoders tested, the rejects named
(`test_reject_mapping_past_the_buffer`, `test_reject_destroy_bound_bo`,
`test_reject_free_of_a_page_not_held`). On the board: the scene loader packs a bundle by flag
class (one CPU-mapped buffer per class, the 8 MiB partial-render scratch the GPU's alone: three
regions where there were 36); the G1 rungs give their spaces back (`gpud: gpu mmu ok (… freed=)`,
`gpud: gpu fence ok (… cleanup_us= freed=)`); every render rung draws in the render rung's
context. P2b: `gpud: gpu contexts ok (rounds=2 contexts=2 samples= refused= both_us= cleanup_us=
teardown_us= pc_a= pc_b= backing= bos= quarantined= cleanup_busy= heap_delta= shown=)` — the
rungs' context torn down first, then twice: A (gpud's) and B (a second client) each its own
catalogue, the render bundle loaded into both at the same addresses, A's target a plane, B's a
buffer of B's, B's texture the checker with red and blue exchanged, both passes in flight, eight
samples, nine refusals (A's buffer and context to B, B's context to A, an address only B binds,
bound buffers not destroyed, a buffer not B's, a context not A's), both torn down; the second
round also refuses the first round's numbers. Carried from P1: the mixed passes' stages
(`gpud: gpu mixed ok (… geom_us=a/b frag_us=a/b …)`).

| # | Hypothesis | Decision line |
|---|---|---|
| H51 | The loader's packing draws as the per-buffer binding did: a binding of a range reaches the same pages | every render rung's line as in cycle 22 (fill, quad, render, library, quads, area, pipeline, mixed), their samples right; a fault or a wrong sample names a range |
| H52 | The firmware lets go of an idle common context, render-target data and free list at the first cleanup | `cleanup_busy=0`; `cleanup_us` of the fence rung about one kernel command's round trip (50–150 µs); a `busy` or `no return` FAIL line names the object |
| H53 | Pages come back exactly: the firmware pool's bitmap, the zeroed reuse, the flush before reuse | `freed=N/N` in the mmu and fence rungs; `backing=` the G1 baseline after the rungs' teardown and after each round; refuted, a page stays held or a recycled page faults the firmware |
| H54 | Two contexts with their own catalogues run in flight together, each through its own space | `pc_a != pc_b`, both passes reached, A's four samples in the plane and B's four (the exchanged checker) in its buffer; refuted forms: B's colours in A's plane (one space), A's colours in B's buffer, a firmware page fault (a range missing in B) |
| H55 | A slot serves again with nothing grown | round 2 refuses round 1's numbers; `heap_delta=0`; `bos=0 quarantined=0` |
| H56 | Two passes in flight overlap (one's geometry beside the other's fragment) | measured: `both_us` against twice one pass (`gpu render ok` us) |
| H57 | (from P1) The mixed pass's 7.5 ms sits in one stage | `geom_us` / `frag_us` of each mixed pass; the larger names the next lever |

**Cycle 23 — red, the cause in the kernel** (`board--2026-10-16T16-23-28`, `cycle23-gpu-lines.txt`,
`cycle23-board-test.txt`; `[FAIL] board-visible` on the rung's FAIL form; the operator: seven of
eight, the screenshot broken). Every render rung green on the new memory — **H51 confirmed**
(`fill ok (ns_per_px=0.9)`, `quad ok`, `render ok (us=6007)`, `library ok`, `quads ok`, `area ok`,
`pipeline ok`, `mixed ok`, the quad rung's context torn down between them); the G1 rungs gave
their spaces back — `mmu ok (… freed=957/957)`, `fence ok (… cleanup_us=111 freed=957/957)`: the
firmware let go of the transfer context at the first cleanup (H52's first half, H53 for the
ladder). Then `gpud: FAIL gpu contexts (round 0: b's target Exhausted)` beside
`[ERROR vm] VM-MAP-FAIL reason=pte va=0x57600000 mapped_to=0x57600000 err=Overlap` — the same
line at every screenshot afterwards (`screencapd: deny (reason=readback)`). Read in the kernel:
an unmap clears the leaves and leaves an emptied level-0 table linked under its level-1 entry
(`unmap_leaf`: "intermediate table pages are NOT reclaimed"); a later map of 2 MiB or more gets a
superpage there and `map_2m` refuses the valid table entry as `Overlap`. gpud never gave a buffer
back before P2; now the rungs' packed buffers go, and the next 8 MiB map in gpud — B's target, the
frame readback — lands in the span such a buffer held. The fix (operator: give the tables back, as
Zircon does): the unmap's clear unlinks every table it emptied and retires it with its region,
the unmap's last phase frees it after the TLB shootdown (CLEAR-SHOOTDOWN-FORGET for the tables);
a superpage that still meets a table is mapped as 4 KiB leaves into it
(`mm/page_table/reclaim.rs`, `vm_ops`); `KSELFTEST: vm unmap ok` now also proves both. Also
measured, **H57 decided**: the mixed passes' time is the geometry stage's — `geom_us=11195/3998
frag_us=0/0`: the geometry word was reached after 11.2 ms (the first pass, into the plane) and
4.0 ms (the second, 540 rows down), and the fragment word within the same slice of a millisecond.
Three draws cost milliseconds before a pixel is shaded: the next lever is the geometry stage of a
mixed pass (vertex programs' first runs, the state loads, or a partial render on a short free
list — the next cycle's line names it).

**Results — the board, cycle 24** (`board--2026-10-16T17-04-44`, `cycle24-gpu-lines.txt`,
`cycle24-board-test.txt`; `[PASS] board-visible`, 64 rungs, the operator's eight confirmations,
the screenshot saved again — `screencapd: saved (kind=window …)`, no `VM-MAP-FAIL`):

`gpud: gpu contexts ok (rounds=2 contexts=2 samples=16/16 refused=20/20 both_us=11854/11848
cleanup_us=1656/1487 teardown_us=1282 pc_a=0x6443000 pc_b=0x6445000 backing=957/957 bos=0
quarantined=0 cleanup_busy=0 heap_delta=0 shown=true)`; `mmu ok (… freed=957/957)`, `fence ok
(rt_us=83 … cleanup_us=122 freed=957/957)`; every render rung as in cycle 23.

- **H51 confirmed** (cycle 23 already): a range of a packed buffer reaches the pages a buffer of
  its own did — every render rung's samples right on three regions where there were 36.
- **H52 confirmed.** The firmware let go of every object at the first cleanup — the transfer
  queues, both render contexts' common contexts, four render-target data, four free lists in two
  rounds, the rungs' context before them (`cleanup_busy=0`); a cleanup is one kernel command's
  round trip (122 µs), a whole render context with its transfer queue and space 0.7–0.8 ms
  (`cleanup_us=1656/1487` for two).
- **H53 confirmed.** The firmware pool back to the G1 baseline after the rungs' teardown and after
  each round (`backing=957/957`), the buffers all destroyed (`bos=0`), nothing quarantined.
- **H54 confirmed.** Two catalogues (`pc_a=0x6443000`, `pc_b=0x6445000`), both passes in flight,
  A's four samples in the plane and B's four — the checker with red and blue exchanged — in B's
  own buffer: the same addresses reached each context's own memory, read and written. Each was
  refused the other's buffers, contexts and addresses by name (`refused=20/20`: nine a round and
  the first round's two numbers in the second).
- **H55 confirmed.** Round 2 on the same slots: the first round's numbers refused, gpud's heap
  unmoved (`heap_delta=0`) — a context costs gpud's heap nothing after its slot's first use.
- **H56 refuted.** Two passes in flight took 11.85 ms, twice one pass (`render ok us=6008`): the
  firmware ran the two contexts' passes one after the other — no overlap to count on; the frame
  stays one context's passes.
- **H57 (cycle 23) holds:** the mixed passes' time is the geometry stage's (`geom_us=19443/6836
  frag_us=0/1`, 26.3 ms this boot against 15.2 ms in cycle 23 — the stage varies by boot); the
  geometry stage of a mixed pass is the frame's next lever (P4's first cycle names the cause:
  partial renders on a short free list, the vertex programs' first runs, the state loads).
- **The kernel fix proven twice:** QEMU `KSELFTEST: vm unmap ok` (smp1, SMP=2, visible lanes) with
  `reclaimed` and `fallback`, and the board — B's 8 MiB target and every frame readback mapped in
  gpud after its buffers came and went.

**Verdict.** The device holds several contexts the way the production stacks do: each client its
own GPU address space and its own firmware objects, refused everything of another's, torn down
by the firmware's cleanup with every page, buffer and table back — and the kernel gives an
address space's page tables back with them. Next: P2c (a fault stays in its context), then P3
(windowd's context over IPC).

## 5c-3 P2c — a fault stays in its context (TASK-0215, RFC-0101 §6)

**Question.** When one context's work faults, does the device keep the fault in that context —
the context lost alone, the firmware running on, every other context drawing, the lost one
given back whole?

**Measured first** (the 5a logs, the code). The firmware reports a reset of a context's work in
its message buffer (`CONTEXT_RESET_NOTIFICATION`: the context's number, a reason, the data master,
flags, and after a page fault the faulting context's page catalogue and the address). The board
sent three forms: cycle 3 a write fault in the geometry stage reported as an *innocent* lockup
(`reason=2 dm=2 flags=0x1 pc=0x5c43000 fault=0xdbf0000000`), cycles 5–7 a fragment-stage fault as a
guilty one (`reason=1 dm=3 flags=0x1 pc=0x6db2001`) — the reason does not tell the culprit, the
page-fault flag and the catalogue do, and the catalogue's address carries a low bit of the
firmware's own. In cycle 3 the reset was followed by a free-list reconstruction request (message
104) that nothing answered: the geometry's update had run (`geom_word=1`), the fragment queue never
moved (`frag_word=0`) and the pass timed out — a reset context's queue waits for the host's
answer. Until P2c gpud named every such message as a FAIL line and answered none; every context
used the same two free-list ids.

**What P2c builds.** A reset is held against the context it blames (`fwif::ContextReset::blame`,
host-tested against the board's three forms): after a page fault the context whose catalogue is
the named page, otherwise the context it numbers unless the reason calls it innocent. That
context is lost: its points count as reached, its submits, binds, resets and setups are refused
`ContextLost`, its memory stays its client's until the client destroys it. A reconstruction
request is answered as the open kernel driver answers it — each list rebuilt as made (its entries
written again, its stack, its counts; `Freelist::rebuilt`, host-tested) and the render-target data
on it put in the recovery's state — then the ids it named sent back; a growth request is answered
with no pages. The free lists carry ids unique on the device (the context's number above the
list's bit). The device serves the messages wherever it waits and between the firmware's busy
answers to a cleanup; each event is a line formatted on the stack (a client faulting again and
again takes nothing of gpud's heap).

**The rung** (`render_containment.rs`, after the contexts rung). Two contexts of two clients
load the render bundle at the same addresses — A's target a plane, F's a buffer of F's own — and
each draws once. F then unbinds the buffer its passes write their region headers into (cycle 3's
fault, made on purpose), and F's next pass is submitted with A's beside it. Every pass's samples
are marked before it runs, so a picture read is that pass's own. Then F is refused by name, A
draws once more, F is torn down, and a fresh context of a third client draws. Gate:
`gpud: gpu containment ok (contexts=3 samples=20/20 refused=4/4 lost=1 pc_f= fault= headers=
fault_us= f_words=g/f of g/f f_teardown_us= c_slot_of_f= resets= innocent= reconstructions=
rebuilt= unknown_lists=0 grows= unattributed=0 teardown_us= backing= bos=0 quarantined=0
cleanup_busy=)`; `gpud: gpu context lost (` names each loss as the firmware reported it.

| # | Hypothesis | Decision line |
|---|---|---|
| H58 | The reset after F's write fault names F's catalogue and nothing names A | `gpud: gpu context lost (… pc=)` masked equals `pc_f=`; `unattributed=0`; A not lost (the rung's FAIL form names either) |
| H59 | The firmware asks for F's free lists after the reset and runs on once answered: F's queues finish their updates | `reconstructions≥1 rebuilt=` two per request, `unknown_lists=0`; `f_words` equal to the points after `of`; words short of them — the firmware holds a reset context's queue, the teardown decides |
| H60 | The firmware lets go of a lost context's objects at its cleanup as of a live one's | F torn down before C is made: `quarantined=0`, `f_teardown_us` near the contexts rung's 0.7–0.8 ms, `cleanup_busy` unmoved; the final `backing=` the baseline |
| H61 | A's pass in flight beside F's fault lands right — the firmware never started it before its recovery, or replays work its reset cut off | A's four samples beside the fault right; a reset naming A counted `innocent=`; a timeout names A's point |
| H62 | A context made after the fault draws in the lost context's slot | C's four samples right, `c_slot_of_f=true` |
| H63 | (measured) what a client's fault costs the device: the firmware's recovery | `fault_us` from F's faulting submit until both points were reached — against the frame's 8.3 ms at 120 Hz |

**Results — the board, cycle 25** (`board--2026-10-17T15-48-51`, `cycle25-gpu-lines.txt`,
`cycle25-board-test.txt`; `[PASS] board-visible`, 65 rungs, the operator's seven confirmations,
the screenshot saved again, no `VM-MAP-FAIL`) — green on the package's first cycle:

`gpud: gpu context lost (ctx=0x301 reason=2 dm=2 job=2 flags=0x1 pc=0x5c45000 fault=0xdbf00032c0)`,
`gpud: gpu free lists rebuilt (n=2 unwritten=0 unknown=0 answered=true hwr=1 first_id=0x602)`,
`gpud: gpu containment ok (contexts=3 samples=20/20 refused=4/4 lost=1 pc_f=0x5c45000
fault=0xdbf00032c0 headers=0xdbf0000000+0x7000 fault_us=6759 f_words=2/4 of 2/4 f_teardown_us=1450
c_slot_of_f=true resets=1 innocent=0 reconstructions=1 rebuilt=2 unknown_lists=0 grows=0
unattributed=0 teardown_us=2722 backing=957/957 bos=0 quarantined=0 cleanup_busy=0)`; the contexts
rung as in cycle 24 (`both_us=11861/11846 cleanup_us=1458/1500 teardown_us=1287`), `render ok
us=6007`.

- **H58 confirmed.** The reset came in cycle 3's form — an innocent lockup's reason in the geometry
  stage with the page-fault flag (`reason=2 dm=2 flags=0x1`) — with F's catalogue (`pc=0x5c45000`
  = `pc_f`) and an address inside F's region headers (`0xdbf00032c0` in `0xdbf0000000+0x7000`): the
  provoked fault, held against F by its catalogue where the reason alone would have called it
  innocent. Nothing named A (`unattributed=0 innocent=0`, A not lost).
- **H59 confirmed.** One reconstruction request, for F's two lists (`first_id=0x602`: F's number
  `0x301` above the list's bit), both rebuilt and answered (`hwr=1`); F's queues then finished their
  updates (`f_words=2/4 of 2/4`), where cycle 3's, unanswered, stood at `frag_word=0` until the pass
  timed out. The firmware asked for no growth (`grows=0`).
- **H60 confirmed** (its time named the wrong span). The firmware let go of the lost context's
  objects at the first cleanup (`cleanup_busy=0`, `quarantined=0`), the pool back at the baseline
  (`backing=957/957`, `bos=0`). `f_teardown_us=1450` times the context with its scene and its 8 MiB
  target; the 0.7–0.8 ms the hypothesis named were the contexts rung's context alone
  (`cleanup_us=1458/1500` for two this boot). In the same rung A and C went in 2722 µs together, A
  without a target of its own: a lost context goes as fast as a live one.
- **H61 confirmed.** A's four samples beside the fault right and no reset named A (`innocent=0`):
  A's pass ran whole beside F's fault — the two contexts' passes run one after the other (H56).
- **H62 confirmed.** C, made after F's teardown, drew in F's slot (`c_slot_of_f=true`), its four
  samples right in its own buffer.
- **H63 measured.** The faulting pass and A's whole pass beside it took 6.76 ms (`fault_us=6759`)
  against 6.01 ms for the same scene's pass alone (`render ok us=6007`, this boot): F's fault with
  the firmware's recovery added 0.75 ms to the work beside it — A's whole pass and the fault fit in
  one 120 Hz frame (8.3 ms).
- **The gate's red form** was measured before P2c existed: 5a cycle 3 (`cycle3-gpu-lines.txt`) made
  the same write fault, answered nothing, and the faulting pass timed out — the rung's FAIL
  `the passes of the fault: point … not reached`.

**Verdict.** A client's fault stays in its context: the firmware's report is held against the
context whose page catalogue it names, that context is refused by name until its client destroys
it, the firmware's request for its free lists is answered so the device runs on, and the other
contexts draw beside it and after it — at 0.75 ms to the work in flight. Next: P3 (windowd's
context over IPC).

## 5c-3 P3 — windowd's context (TASK-0215, RFC-0101 §1, §7, §7b)

**Question.** Can windowd open its own GPU context at the highest priority and hand gpud its
pipeline library over IPC — on the board and on QEMU — so that gpud holds no GPU code of its own
and a restarted windowd gets a fresh context?

**Measured first** (the code read through). windowd and gpud share one endpoint pair init mints,
windowd the only sender; the wire is `nexus-display-proto` (ADR-0038), ops 1–15, a reply told
apart by its length. gpud discarded the sender's id. A service's id is the kernel's hash of its
name — a restarted windowd keeps it — and nothing tells gpud that windowd went (init keeps a send
right on gpud's endpoint, so no end-of-file reaches it; windowd is not respawned today): the
tree's pattern is that the newest attach wins. gpud compiled the library in twice — the board
rungs' and QEMU's, whose GL scanout and splash draw with it from windowd's attach on, so
windowd's library can arrive before it.

**What P3 builds.** Op 16 answers the GPU's code target, its feature bits and the context bound;
op 17 opens the sender's context with its library moved in as a VMO and answers the context and
the entries taken and refused (`nexus_display_proto::gpu_context`, sixteen-byte answers with the
op in their second byte). gpud serves windowd alone (the kernel's id), checks the VMO before it
maps it read-only, closes it whatever the answer, and replaces a context the client holds whole.
`GfxDevice::take_library` places a library on every device (the mock places it as the board
does). On the board windowd's context is made as the render rungs' is — the render bundle bound
with its target unbound, the render objects set up, the library placed; on QEMU the virgl
pipelines are made from windowd's library and the draw, gradient and composite readbacks run on
them. windowd carries one library per code target and picks it by the target op 16 names; gpud's
code embeds none (a host test fails on one, the board rungs' fixture excepted).

**The gate.** Board: `gpud: gpu library ok (client=windowd ctx= prio=0 bytes= entries= refused=0
code_bytes= setup_bytes= open_us= replaced= heap_bytes=)` (REQUIRED), windowd's `windowd: gpu
context ok (ctx= entries= bytes= target= features=)`; the rungs' line now `gpud: gpu library ok
(client=gpud …`. QEMU, every GL lane (`scripts/qemu-test.sh`): `windowd: gpu context ok (` and
`gpud: virgl library ok (client=windowd …)`, the virgl readbacks after it.

| # | Hypothesis | Decision line |
|---|---|---|
| H64 | windowd's library reaches gpud whole and is placed as the rungs' fixture is | `client=windowd … entries=7 refused=0 code_bytes=2808 setup_bytes=852` — the fixture line's numbers, which the mock predicts (`a_context_takes_its_library_as_the_board_places_it`); windowd's `target=0x24001d003400b6 features=0x1` |
| H65 | windowd's context costs its start about the render rung's bind | `open_us` near `render ok … bind_us=6467` (the bundle bound and the render objects set up, no plane adopted) |
| H66 | an open costs gpud's heap the library's table alone | `heap_bytes` near the rung's `table_bytes=7039`: the context's slot storage is reused after the rungs |
| H67 | windowd's context fits beside the display | no `VM-MAP-FAIL`; the desktop and the screenshot as in cycle 25 (the operator's confirmations) |
| H68 | QEMU's virgl path draws on windowd's library | `gpud: virgl library ok (client=windowd … fresh=true draw_ok=true)`, then `virgl draw ok`, `virgl gradient ok`, `layer composite ok`; the visible lane green (decided in QEMU before the board) |

**Results — QEMU** (2026-10-17, `visible--2026-10-17T17-05-26`, `headless`, `display-gpu`: each
`SELFTEST: Completed (markers verified)`). The visible lane's probe stops before the library
(`gpud: virgl ready`, `gl draw unproven at probe …`); windowd's open brings it — `virgl shader
ok`, `virgl draw ok`, `virgl gradient ok`, `layer composite ok`, `gpud: virgl library ok
(client=windowd ctx=1 bytes=36880 entries=15 fresh=true draw_ok=true)`, `windowd: gpu context ok
(ctx=0x1 entries=15 bytes=36880 target=0x5447534900000001 features=0x0)` — and the desktop
composites live on windowd's pipelines (`gpud: layer composite live`). The 2D lanes name the
other answer: `windowd: gpu context none (the display's GPU runs no code)`. **H68 confirmed.**

**Results — the board, cycle 26** (`board--2026-10-17T21-48-46`, `cycle26-gpu-lines.txt` — gpud's
GPU lines and windowd's context line, `cycle26-board-test.txt`; `[PASS] board-visible`, 66 rungs,
the operator's seven confirmations, no `VM-MAP-FAIL`) — green on the package's first cycle:

`gpud: gpu library ok (client=windowd ctx=0x600 prio=0 bytes=4748 entries=7 refused=0
code_bytes=2808 setup_bytes=852 open_us=9971 replaced=false heap_bytes=9023)`, `windowd: gpu
context ok (ctx=0x600 entries=7 bytes=4748 target=0x24001d003400b6 features=0x1)`; the rungs'
fixture `gpud: gpu library ok (client=gpud bytes=4748 entries=7 refused=0 code_bytes=2808
setup_bytes=852 table_bytes=7039 …)`, `render ok … bind_us=6572`.

- **H64 confirmed.** windowd's library reached gpud whole and was placed as the fixture is: the
  same seven entries, nothing refused, the same 2808 code and 852 setup bytes the mock predicted.
  windowd picked it by the target op 16 named (`target=0x24001d003400b6`), with the feature bit
  the layers need (`features=0x1`).
- **H65 named the wrong span** (as H60's time did). `open_us=9971` times the whole open — the
  bundle parsed, the context created, the bundle bound with its target unbound, the render objects
  set up, the library placed; the rung's `bind_us=6572` (this boot) times the binding and the
  setup alone. The open costs windowd's start 10 ms, once; how the 3.4 ms beyond the binding split
  among the parse, the context and the library this cycle did not measure.
  **Corrected 2026-10-18 (TASK-0330 S2b, the console's measurement README):** with no console
  write holding the kernel lock in the bring-up, the open took 6119 and 6098 µs in two boots
  against the rung's `bind_us=6392` — the 3.4 ms were the bring-up's lock waits, not the open's
  steps, and H65 holds as predicted (the open binds no target; the rung binds its plane).
- **H66 refuted in its number.** `heap_bytes=9023` against the table's `table_bytes=7039`: an open
  costs gpud's heap 1984 bytes beside the table. By the code, the open parses the render bundle
  again and keeps the bound bundle's buffer list, both per open; this cycle did not split the
  1984 bytes. gpud's heap never frees, so each open costs it 9 KB for good — once a boot today
  (windowd is not respawned).
- **H67 confirmed.** No `VM-MAP-FAIL`; the desktop, the pointer, typed input and the screenshot
  confirmed by the operator as in cycle 25.

**Observed outside P3 — a teardown held up while the console printed a crash.** The containment
rung's lost context took 130 ms to tear down this boot (`f_teardown_us=129833`; cycle 25: 1450) on
a path P3 did not change. Between the rung's loss line and its verdict the log holds the
supervision probe's second crash (`init: restart svc=fault-probe`): 1540 bytes of `[USER-PF]`
lines, 133.7 ms on the 115 200-baud console in a proof boot, which prints the dump whole. In cycle
25 the probe's dumps fell before and after the rung. The 128 ms the teardown lost match the dump's
console time; what held gpud meanwhile is not measured (the kernel's lock accounting shows no
single wait above 29 ms this boot). Carried: measured before P4 puts the desktop's frames through
gpud, where such a hold would show as a stall. **Measured and fixed 2026-10-18 (TASK-0330 S2b):**
the report waited for the serial line — every bring-up filled the console's 8 KiB queue, and a
write into it waited with its hart's interrupts off. The serial line now reads the console ring
itself: a crash report holds its hart 3 ms (cycle 27, two boots). The containment teardown took
1504 and 1501 µs there — no report fell into the rung in either boot; one now could add about
3 ms.

**Verdict.** windowd opens its own context at the highest priority and hands gpud its library
over IPC, on the board and on QEMU; gpud serves windowd alone, replaces a context windowd holds
whole, and holds no GPU code of its own (the board rungs' fixture excepted). P3 complete; P4 next —
the desktop drawn on the GPU in windowd's context.

## 5c-3 P4a — one plan for both paths, and the mixed pass taken apart (TASK-0215)

**Question.** P4 draws the desktop on the board's GPU in windowd's context. Two things come
first: the frame's model — today the QEMU path's backend alone knows how a frame is composed —
and the cost of a pass of mixed draws, which P1 and P2 left unnamed.

**Measured first** (the code read through; board cycles 21–24):

- **The board presents on the CPU.** A ring slot taken, the retained plane copied into it whole
  (about 3 ms), windowd's commands run by the CPU executor, the damage cleaned, the flip at the
  blank: 100–200 ms a present. No GPU work on the present path; no switch between paths.
- **The QEMU path composes the frame itself** from the `CompositeLayer` commands alone: the
  wallpaper, then per layer back to front its glass (the destination so far snapshotted, blurred
  at half resolution, cached by a key folded over every layer below), its shadow, its content
  from the atlas; scroll and transform overrides applied first. That model — the retained set,
  the overrides, the walk, the glass cache — lived in gpud's virgl backend only.
- **One memory holds everything a frame touches**: the wallpaper plane, the retained plane, the
  three ring slots and the atlas (windowd's copies of the clients' surfaces, straight alpha) are
  rows of one contiguous DMA buffer (stride 7680). A context can take it the way the rungs take a
  ring slot (`adopt_plane`), as targets and as textures.
- **The open cost: a pass of mixed draws.** One draw a pass: the geometry word after 0.22–0.28
  ms (submit included), the fragment by the program's ns a pixel. Three draws of three pipelines
  over 1156x256 (P1): 7.5 ms a pass, all of it before the geometry word — 11.2 and 4.0 ms
  (cycle 23), 19.4 and 6.8 ms (cycle 24), the first pass of the rung the slower. The stamp starts
  before the submit, so the CPU's share was never apart; P1's straight draw samples a block of
  the plane — the display's memory, outside the GPU's cache.

**What P4a builds.** `nexus_gfx::frame` (host, 14 tests): the retained set (a present without
layers keeps it; layers replace it with its scroll overrides; transform overrides stay), the
overrides applied (moved, faded, scaled about the centre), the walk back to front with the glass
cache's read, write or off against the destination-so-far key, and the atlas's upload once per
content generation — allocation-free, the plan the frame's own storage. The QEMU path's virgl
backend draws that plan: its own walk, overrides and cache (`blur_cache.rs`) are gone. On the
board, a rung after the mixed one (`gpud: gpu passes (…)`) takes the mixed pass apart: six passes
into its target and area, each stamped at the submit's return, the geometry word and the
fragment word — `a` one opaque draw (the checker), `b` three opaque draws, `c` three pipelines,
`d` one straight draw over the block in the plane, `e` the mixed pass, warm, `f` the mixed pass
after 200 ms without GPU work; each of `a`–`e` run twice, the second reported.

| # | Hypothesis | Decision line |
|---|---|---|
| H69 | The CPU's submit is not where the milliseconds go | every case's submit (`x=s/…`) below 1 000 µs; refuted, `e`'s submit carries the excess — the lever is the host's writes and cache maintenance |
| H70 | Draws and pipeline changes cost the geometry stage little | `b`'s and `c`'s geometry within 500 µs of `a`'s; refuted, the state a draw changes (its pipeline's setup, its texture's state load) is the lever |
| H71 | A texture in the display's memory costs its fetches | `d`'s fragment at least five times the straight program's 4.87 ns a pixel over its 32 768 pixels (160 µs); refuted, the display's memory serves textures as well as the GPU's own |
| H72 | The GPU's idle costs the pass after it | `f`'s whole pass at least 3 ms above `e`'s; refuted, the first pass of P1's rung paid for something else |
| H73 | No partial render: a mixed pass's interrupts are one pass's | `irqs=` of `b`, `c` and `e` equal `a`'s; more, the geometry outgrew its parameter memory and the fragment stage ran twice |

**Results — QEMU** (2026-10-18, `visible--2026-10-18T15-27-18`, `headless`, `display-gpu`: each
`SELFTEST: Completed (markers verified)`): `gpud: layer composite live`, `gpud: rt backdrop dst
ok` — the layers and the glass drawn from the plan; every FAIL line as in the run before.

**Results — the board, cycle 28** (`board--2026-10-18T15-56-13`, `cycle28-gpu-lines.txt`,
`cycle28-board-test.txt`; `[PASS] board-visible`, 67 rungs, the operator's seven confirmations,
the screenshot's selection dragged):

`gpud: gpu passes (a=16/212/406 b=15/216/603 c=16/214/1174 d=19/211/353 e=16/215/1148
f=29/212/1153 irqs=4/4/4/4/4/3 idle_ms=200)`; `gpud: gpu mixed ok (… mixed_us=2771
geom_us=229/230 frag_us=1153/1158 …)`.

- **H69 confirmed.** The submit took 15–29 µs in every case.
- **H70 confirmed.** The geometry took 211–216 µs after the submit whatever the pass draws: one
  draw, three, three pipelines.
- **H71 refuted.** A texture in the display's memory costs nothing extra: `d`'s fragment took
  353 µs — the straight program's 160 µs over its 32 768 pixels and the fixed part every pass has
  (`a`: 406 µs for 166 µs of pixels).
- **H72 refuted.** 200 ms without GPU work cost nothing: `f` 15 µs above `e`.
- **H73 confirmed.** Four interrupts a pass in `a`–`e` (three in `f`): no partial render.
- **The mixed pass's milliseconds are gone — since cycle 25.** The mixed rung's own line read
  26 282 µs for its two passes in cycle 24 (geometry 19 443 and 6 836) and 2 790 µs in cycle 25
  (geometry 239 and 228), 2 761–2 790 µs in every cycle since. Cycle 25 was P2c's: the device
  answers the firmware's messages in every wait since, and the free lists' ids became unique on
  the device. No growth request was ever asked (`grows=0`, no `free list not grown` line); which
  of P2c's changes removed the wait was not measured before it vanished.
- **What a pass costs now:** about 0.45 ms fixed (the submit's 0.02, the geometry's 0.21, the
  fragment's own 0.19–0.27) and each draw's pixels at its program's rate — `c`: 166 + 160 + 581 µs
  of pixels and 267 µs fixed, 1 174 µs. From that line P4b's full-screen frame (the desktop's
  layers, their shadows, the glass read from its cache) is estimated at 15–20 ms against the CPU
  path's 100–200 ms.

**Verdict.** The frame has one model — the QEMU path draws the planner's steps — and a pass of
mixed draws costs its pixels and a fixed half millisecond: the board's executor builds on the
per-pixel model. P4a complete; P4b next.

## 5c-3 P4b — the desktop on the board's GPU (TASK-0215, RFC-0101 §8d)

**Question.** Does windowd's desktop draw on the board's GPU — in windowd's own context, from
the plan the QEMU path draws — at a fraction of the CPU path's cost, with every glass level
windowd asks for, and does the screenshot still see it?

**Measured first** (the code read through; host 2026-10-18; cycle 28's log):

- **The CPU path's cost, the baseline.** Cycle 28's windows of 32 presents: 104 393, 116 194
  and 138 586 µs on average with 4.3–5.9 layers, 2.3–3.9 of them glass, the whole screen
  damaged (`gpud: dc present us (…)`); the layers' blending takes 82–96 ms of it, the blurs
  9–32 ms.
- **windowd asks for four blur radii**: 8 (subtle), 20 (card), 30 (the default), 40 (panel,
  overlay). The board's library held 20 alone. Radius 30 did not compile: a run of constants
  that did not fit the pipeline's block of 64 dwords left the block half placed; placed whole or
  not at all, the vertical pass's constants still outgrew 64. The shared block can hold 128 (the
  DMA's size field holds 12 bits, the library's record a dword count in a byte, a source names a
  shared register in 11 bits).
- **The half-resolution chain against the full-resolution gaussian** (host, two backdrops, the
  largest and the mean deviation in levels): radius 8 5/0.44 and 7/0.58, 20 2/0.19 and 3/0.28,
  30 3/0.24 and 4/0.32, 40 2/0.21 and 4/0.36.
- **A pass cannot read its own target** (no load: a pass clears its area): the QEMU path's
  destination snapshot under a glass becomes, on the board, the scene below the glass drawn
  again into a scratch over the horizontal taps' reach.
- **The render dataset is the display's 1920x1080**: a pass draws inside it alone, so the glass
  cache (half-resolution rows, one per glass) holds 1024 rows.
- **One memory holds what a frame touches** (P4a): the wallpaper plane, the retained plane, the
  ring and the atlas, rows of one contiguous buffer the CPU writes and the device does not snoop.
  windowd raises the atlas's content generation with every write (the QEMU path uploads by it);
  it writes the wallpaper plane at boot and with a new theme, then says so (`OP_REVEAL`,
  `OP_WALLPAPER_DIRTY`); a capture's freeze copies the frame into the retained plane, its thaw
  says `OP_WALLPAPER_DIRTY`.

**What P4b builds.**

- **The compiler and the library**: a constant block is placed whole or not at all (the
  broadcasts stay), the shared block holds 128 dwords; the library gains the blur's two passes
  at 8, 30 and 40 (ids 8–13: 13 entries, 9 072 code bytes, 2 704 setup bytes; the library's PDS
  lines 6 KiB). A board rung samples each radius's chain back (`gpud: gpu blur radii ok (`);
  another names five 8x8 rectangles of a strip the CPU wrote 1, 2, 3, 4 and 8 texels past an
  aligned column and reads which texel each fetched (`gpud: gpu texel offsets ok (`).
- **`nexus_gfx::frame::board`** (host, 9 tests): the plan lowered for the board — per glass
  written, the scene below it into the scratch, the horizontal pass into the half-resolution
  scratch, the vertical pass into its cache rows; then the slot's pass: the base, per layer its
  glass drawn back, its shadow, its content (a rounded layer as its three bands and four corner
  squares, the rounded program for the corners alone). `RetainedLayers::covers`: a present the
  plan draws whole (its layers, the damage blits, the pointer); any other command is the CPU's.
- **gpud** (`backend/gpu/desktop.rs`, `backend/dc/gpu_frame.rs`): at windowd's first present
  after its context opened, the context takes the layout read-only and the ring's slots as
  targets, three buffers of its own for the scratches and the cache, one for the passes' memory;
  a present the plan covers is drawn into the free slot, its passes waited, the flip queued at
  the blank. The caches: the base plane cleaned when it is new or the CPU rewrote it, the layers'
  atlas rows cleaned once per content generation, a slot the CPU wrote dropped from the caches
  before the GPU draws it, the front slot before a readback reads what the GPU drew. A freeze
  makes the retained plane the base until the thaw. A refusal is said once
  (`gpud: FAIL gpu desktop (`) and the CPU composes from then on.
  Lines: `gpud: gpu desktop ok (frame_us= passes= draws= layers= glass_writes= glass_reads=
  glass_off= clean_us=)` once, `gpud: gpu frame us (n=32 avg= max= clean= passes= draws=
  glass_w= glass_r= glass_off=)` per 32 frames.

| # | Hypothesis | Decision line |
|---|---|---|
| H74 | The board's chain at 8, 30 and 40 reads its radius's gaussian as 20's does | `gpud: gpu blur radii ok (` — every read inside the glass within three levels of the host's largest deviation for its radius |
| H75 | windowd's desktop draws on the GPU from the first frame after its context opened | `gpud: gpu desktop ok (` and no `gpud: FAIL gpu desktop (`; the operator sees the desktop, its glass, shadows and windows as on the CPU path |
| H76 | A desktop frame costs a fifth of the CPU's present or less | every `gpud: gpu frame us (` window's `avg=` at or below 20 000 µs (P4a's estimate 15–20 ms; the CPU path 104–139 ms); above: the line's passes, draws and clean name the lever |
| H77 | The caches cost the GPU frame little | `clean=` of every window at or below 2 000 µs |
| H78 | The glass cache carries an idle desktop | windows without the operator's input read their glass (`glass_r=`) at least four times as often as they write it (`glass_w=`) |
| H79 | The screenshot sees the GPU's frame | the operator's capture with its selection dragged shows the desktop; the freeze holds it under the shell's overlay and the thaw returns the live desktop |
| H80 | A texture's base at any 4-byte step is fetched from there (every rung before sampled at 128-byte steps; the frame names atlas bands at any column, a glass's cache rows one texel in) | `gpud: gpu texel offsets ok (k=1/2/3/4/8 read=1/2/3/4/8)`; refuted, the reads name the alignment the unit applied — the board's layer variants then take the atlas mapping (the rectangle in the push constants, the base aligned) |

**Results — the board, cycle 29** (`board--2026-10-18T22-40-30`, `cycle29-gpu-lines.txt`): the
desktop stayed on the CPU — three FAIL lines, all before a GPU frame was drawn.

- `gpud: FAIL gpu blur radii (radius 40 emit Space)` and `gpud: FAIL gpu texel offsets (emit
  Space)`: both rungs placed their passes' memory in the cost rung's tails after the pipeline
  gate, never reusing it; the chains at 20, 8 and 30 filled them. Every pass is waited, so each
  chain and the texel rung now start from the tails again. H74 and H80 are open.
- `gpud: FAIL gpu desktop (open: a binding; the display composes on the cpu)`: a context's page
  tables held 64 tables (128 MiB of addresses) in 8 directories (a GiB each). Counted on the host
  from the render bundle: windowd's context holds 9 tables in 4 directories before the desktop;
  the layout takes 52, the ring's slots bound a second time 12 more — 73, refused at the second
  binding; the scratches, the cache and the pass memory, each in a GiB of its own, would have
  needed 9 directories. Now the layout is bound once, in three ranges by access (the planes and
  the atlas read-only, the slots read-write), and everything in one GiB: 64 tables beside the
  bundle's 9 in 5 directories; a context holds 128 tables (256 MiB). A refusal names its step and
  the device's word; the first frame's line carries the open's cost (`open_us=`).
- The CPU path meanwhile: windows of 94–224 ms a present, as in cycle 28.
- The pipeline gate's rates on this build (ns a pixel): opaque 1.30, premultiplied 4.04, straight
  4.87, rounded 8.84, shadow 10.77, blur horizontal 39.19 and vertical 24.57 (half-resolution
  pixels), the glass drawn back 8.85 — in the order of the programs' instruction-group budgets
  (3, 11, 15, 34, 43). For H76: the shell's full-screen layer alone is 2.07 million straight
  pixels, about 10 ms.

**Results — the board, cycle 30** (`board--2026-10-19T08-27-38`, `cycle30-gpu-lines.txt`):

- **H74 confirmed**: `gpud: gpu blur radii ok (r8=4/17.12/12.69 r30=1/54.62/35.80
  r40=1/78.81/43.70 tolerance=10/7/7)` — the reads 4, 1 and 1 levels from their gaussian; a
  half-resolution pixel of the horizontal pass costs 17.1, 54.6 and 78.8 ns, of the vertical
  12.7, 35.8 and 43.7 (radius 20: 40.7 and 23.7).
- **H80 confirmed**: `gpud: gpu texel offsets ok (k=1/2/3/4/8 read=1/2/3/4/8)` — a texture named
  at any 4-byte step is fetched from there.
- The desktop opened in 20 ms (`open_us=20326`) and drew windowd's first 32 frames on the GPU —
  `frame_us=6333`, the window `avg=5578 max=10126 clean=32`, one pass of the wallpaper alone:
  the presents before windowd's first layer. Every present with a glass layer went to the CPU
  (its windows 101–231 ms): the layer encoder emits the CPU's recipe beside each glass layer
  (the backdrop restored, `BlurBackdrop`, the cache's `BlitAbsolute`), and `covers` refused
  those commands — the stats' `blur=` column, 7–49 ms a present, is them. The QEMU path draws its
  layer set and leaves the recipe; `covers` now takes the recipe as the plan's to supersede (a
  fill or a shadow of its own still goes to the CPU).
- The operator: a little better, not smooth yet; the windows rounded, their blur not — the CPU's
  recipe blurs the layer's rectangle, so its square corners show beside the rounded content (the
  GPU's glass is drawn back by the rounded program). H75–H79 open.

**Results — the board, cycle 31** (`board--2026-10-19T08-46-06`, `cycle31-gpu-lines.txt`): every
present on the GPU — 28 windows of 32 frames, no FAIL line, the ladder's 70 rungs.

- **H75 confirmed.** `gpud: gpu desktop ok (frame_us=6077 … open_us=17499)`; the operator: the
  desktop much faster, the windows' corners clean, the blur right.
- **H76 refuted.** Windows whose glass was read from its cache (`passes=` 1.1–1.3): 13.3–18.1 ms
  a frame on average. Windows that wrote glass (`passes=` 2–8.5): 24–88 ms, the slowest frame
  186 ms. The CPU path's windows were 101–231 ms. A glass written costs three passes — the scene
  below it again at full resolution over the taps' reach, the horizontal blur at about 40 ns and
  the vertical at about 24 ns a half-resolution pixel — about 18 ms for a window-sized glass. And
  every content write anywhere rewrites every glass above a layer: the key folds the layers'
  content generation, which windowd raises for any atlas write (one global counter, the QEMU
  path's upload rule).
- **H77 confirmed.** `clean=` 29–704 µs a frame in 27 windows, 1 685 in one.
- **H78 confirmed for an idle desktop**: the windows without input read their glass 7–47 times
  as often as they wrote it (93/3, 94/2, 30/2, 28/4); under input reads and writes are alike.
- **H79**: the operator dragged the screenshot's selection and saved it — the drag lags the
  pointer (the frame's cost while the shell's overlay rewrites its glass).
- The operator's notes for the next packages: scrolling not smooth yet, windows slow to open,
  typed input late — the frames that write glass, and gpud waiting for every pass.

**The board test** (`cycle31-board-test.txt`): cycle 31's capture holds the ladder's 70 rungs
and the operator's confirmations of the desktop, typed input, the pointer, the screenshot
(selection dragged, saved — **H79 confirmed**), the search with the clipboard and the modal
alert; not `tile`. The same image booted again for the tiling check (cycle 32): drag-to-edge and
the app menu tile, the keyboard chords do not — no `windowd: wm chord (` line in any board boot
since 2026-10-16 15:01, nor a refusal, with no commit on the input path since (TASK-0066 F8);
in that boot no key reached inputd at all (`inputd: live keyboard route on` missing). Both are
outside this package's path and recorded there; the operator decided to carry on with the GPU.

**Verdict.** windowd's desktop draws on the board's GPU in its own context, from the plan the
QEMU path draws, with every glass level and the screenshot: a frame 13–18 ms with its glass
cached, against the CPU path's 101–231 ms. What is slow is named: a glass written costs three
passes and is rewritten whenever any content changes (the key folds a global generation), and
gpud waits for every pass. P4b complete; P5 — what changed, only — next.

## 5c-3 P5 — what changed, only (TASK-0215, RFC-0101 §8: damage per plane)

**Question.** Cycle 31 named what is slow: a frame redraws the whole screen into its slot, a
glass is blurred again (three passes, about 18 ms window-sized) whenever any content anywhere
changed — the key folded windowd's one global content generation — and every glass above a
changed layer with it. Does a frame that draws only what changed, and blurs a glass only when
something within its reach changed, bring an idle desktop's frame under the pass's fixed cost and
a typing or scrolling frame under the window's?

**Measured first** (cycle 31's log, the code read through):

- **windowd knows its damage already.** A client's present names its rectangles
  (`OP_SURFACE_PRESENT`), windowd turns them into screen damage per rectangle, a window move
  queues its old and new frame, and windowd's own CPU glass cache is invalidated by exactly that
  (`invalidate_blur_over`). gpud got it only as the present's `BlitSurface` restores, which the
  glass recipe's own restores are indistinguishable from; the present's header carries the op
  and the sequence alone. RFC-0101 §8 promises damage per plane on the wire.
- **A per-band content generation would not do**: the shell is one screen-sized layer, so a
  hover anywhere on it would be whole-screen damage again. The gain is windowd's rectangles.
- **Cycle 31's windows**: 5 of 28 had no input (`passes=` 1.1–1.3, 13–18 ms, 2–4 glass writes
  each — the pointer's own presents rewrote glass); the others 24–88 ms with 9–81 glass writes.
  The screenshot's drag: windowd presented a glass of the whole screen each pointer move
  (`dc present mix (… glass_px=2137120 …)`) — the capture tool's scrim is a screen-sized glass
  layer; its three passes cost about 90 ms a frame.
- **A pass's cost** (P4a): 0.45 ms fixed and its pixels at the program's rate — the slot's pass
  over the whole screen draws the shell's 2.07 million straight pixels (10 ms) plus the windows.

**What P5 builds.** `Command::Damage { rect }` in the present (one per screen rectangle, inside
the display, never empty — `test_reject_damage_empty_or_wrapping`); windowd sends one per damage
rectangle beside its restore blit. The planner (`nexus_gfx::frame`, 322 host tests): a frame's
damage is the present's rectangles, every layer drawn otherwise than in the last plan (where it
was and where it is, its shadow's halo included), every layer gone, and every glass blurred
again; a glass is blurred again when its key changed (the layers' geometry below it — the
content generation left the key) or the damage touches its reach (66 px); a present that brings
layers and names no damage is damage everywhere. `damage::History` keeps the last eight frames'
damage; the board's executor records each slot's frame and limits the slot's pass to what the
slot lacks (the frames since, everything when the CPU drew it since, the base changed or the
slot's age is unknown) and this frame's damage — a frame that lacks nothing runs no pass. The
QEMU path draws the whole frame as before and shares the glass rule. Lines: `gpud: gpu desktop
ok (… area_px=)`, `gpud: gpu frame us (… area_px=)`.

| # | Hypothesis | Decision line |
|---|---|---|
| H81 | An idle desktop's frame costs a pass's fixed share | windows without the operator's input: `avg=` at or below 3 000 µs and `area_px=` at or below 5 % of the screen (103 680); refuted, the pointer's presents carry damage the planner cannot keep small |
| H82 | Typing in a window rewrites no glass of another window | windows under typing: `glass_w=` per frame at most the glasses over or beside the typed window (1 on this desktop); more, the damage reaches further than the planner says |
| H83 | A typing or scrolling frame costs the window, not the screen | those windows' `avg=` at or below 10 000 µs and `area_px=` at or below the window's rectangle with its halo (about 1.0 million for a 1156x768 window) |
| H84 | The screenshot's drag is the scrim | the capture drag's windows keep `glass_w=` 1 a frame and `avg=` above 20 000 µs: the screen-sized scrim glass is the next lever (the shell's, not the planner's) |
| H85 | No stale pixel | the operator sees no remnant after moving, tiling, closing a window, typing, scrolling or the thaw; the screenshot shows the desktop |

**Results — the board, cycle 33** (`board--2026-10-19T10-09-15`, `cycle33-gpu-lines.txt`): every
present on the GPU, the operator: no remnant anywhere, scrolling and typing smoother, the desktop
much better — not yet smooth; tearing while scrolling (a horizontal line, old above, new below);
the screenshot's selection lags.

- **H85 confirmed**: no stale pixel after moving, closing, typing, scrolling, the thaw.
- **H81 refuted, and the floor named by its shape**: the idle windows draw 1 024 pixels a frame
  (`area_px=1024`, the pointer's rectangle; `passes=1.0 draws=5 glass_w=0`) and still cost
  12.6 ms (`avg=12643–12695`): a frame has a fixed cost of about 12 ms that is not GPU work —
  a pass is 0.45 ms (P4a). Where it goes is unmeasured; the next cycle splits the frame into the
  plan, the passes' emit, their submit, the waits and the retire (`split=`).
- **H82 refuted**: typing rewrote two glasses a frame (`glass_w=64` per 32, `passes=7.0`): the
  typed window's own glass — its content damage lies within its own reach — and the glass
  beside it. The damage carries no origin; the planner cannot tell a change below a glass from
  the glass's own. Now `Command::Damage { rect, origin }`: windowd names the topmost window the
  rectangle lies within (its `layer_id`) or the desktop (0); a glass blurs again only for an
  origin below it (the desktop, a window below it in the walk, an unknown one), never for its
  own content, a window above it or the pointer (`ORIGIN_ABOVE`).
- **H83 refuted with it**: typing windows 49–54 ms with `area_px=` 297–557 thousand.
- **H84 confirmed**: the capture drag's windows `avg=` 99–182 ms, `area_px=` 1.3–2.1 million,
  `glass_w=` 60–116: the scrim is a screen-sized glass blurred again at every move (the shell's
  overlay, outside the planner's lever).
- The glass cache ran out under many glasses (`glass_off=` 9–32 in three windows): a glass
  without rows is drawn unblurred on the board — a visible difference between frames whenever
  the set reorders. Carried: the cache packs by rows only; two columns of 960 half-resolution
  texels would double it (the next package).
- **The tear**: windowd's `front=0` in every window — the GPU never draws into the slot the ring
  calls scanned. The flip itself: `flip_in_blank` writes the plane's address, then the
  controller's config-ready latch and its start word (`dc::flip` ends with `flush`) — the
  register documentation names the latch "for the next frame". If the controller takes the
  plane at the frame boundary after the write, the slot the ring frees at the blank is scanned
  for one more frame while the next GPU frame is drawn into it: old above the GPU's write
  position, new below. Measured next cycle: whether the latch is still set right after the
  write (`pending=`) and the line the write landed on (`gpud: dc flips (n= pending= crossed=
  line_after_max=)`, every 64 flips).
- **Scrolling** on the board is the app's re-render: windowd sends `OP_SET_LAYER_SCROLL` alone
  and queues no present; the board's display answered "not retained" and nothing drew — the
  app re-rendered on the scroll position instead. Now the loop records the override in the
  GPU's retained set and, once the burst drained, draws one frame of it (`recompose`): the
  layers that moved, nothing else.

| # | Hypothesis | Decision line |
|---|---|---|
| H86 | The controller takes a flipped plane at the next frame boundary, so the slot the ring frees at the blank is still scanned for a frame — the tear | `gpud: dc flips (… pending=N …)` with N near n: confirmed, the ring frees a plane one blank later; N = 0 and `crossed=` 0: the write takes effect at once and the tear is elsewhere |
| H87 | A wheel step draws the window's rows alone and the fling stays on the GPU | scroll windows: `passes=1.0`, `glass_w=0`, `area_px=` at or below the window's rectangle, `avg=` at or below the floor + the window's pixels (about 17 ms); the operator: scrolling smooth to the eye |
| H88 | The 12 ms floor is the wait for the pass, not the CPU's emit | `split=` of the idle windows: `wait` above 10 000 µs and the others below 1 000; refuted, the named share is the lever |
| H89 | Typing rewrites no glass | typing windows: `glass_w=0` (the typed window's glass kept; a glass beside it within 66 px rewrites — reported if so) |

**Results — the board, cycle 34** (`board--2026-10-19T10-46-29`, `cycle34-gpu-lines.txt`; the
operator: not much better — the GPU packages first, the feel later):

- **H86 confirmed**: `gpud: dc flips (n=576 pending=90 crossed=0 line_after_max=0)` — one flip
  in six or seven leaves the config-ready latch set after the write: the controller takes the
  plane at a later frame boundary while the ring freed the old slot at once, and the next frame
  was drawn into a plane still on screen. The ring now counts such a flip as not landed: it
  stays queued and lands at the next blank (`ring.passed()`), the old slot stays scanned. The
  line after the write reads 0 in the blank (no witness of the write's place).
- **H88 unmeasured**: `split=` printed zeros — the frame's split was summed into the window
  nowhere (the accumulation was edited against text the formatter had rewrapped); fixed.
- **H89 refuted, and why**: typing windows still `glass_w=65` per 32 frames. An app window's
  material glass regions and its live-resize title overlay carried `layer_id` 0 ("untagged"),
  so the damage's origin (the window's `slot + 1`) matched no layer and counted as below. Every
  slice of a window now carries the window's id (what the transform override wanted too).
- **H87 open**: the scroll fast path reached the GPU's set, but every window still wrote glass;
  judged next cycle with H89.
- The frame windows: 10–134 ms; the idle ones gone from this log (the operator scrolled and
  typed throughout).

**Results — the board, cycle 35** (`board--2026-10-19T11-09-31`, `cycle35-gpu-lines.txt`,
`cycle35-board-test.txt`: the ladder's 70 rungs, the operator's confirmations of the desktop,
typed input, the pointer, the screenshot with its selection dragged, the search with the
clipboard and the modal alert; `tile` not — the chords, TASK-0066 F8; the operator: good enough
for now, the GPU packages first):

- **H88 confirmed**: the wait for the passes is 96.9 % of the frame over 16 windows (`split=`:
  plan 3–8 µs, emit 0.3–3.6 ms, submit 0.06–0.45 ms, wait 17–135 ms, retire 5–18 µs). A frame's
  cost is its passes' pixels at the programs' rates — the blur at 40.7 and 23.7 ns a
  half-resolution pixel is the lever: a window-sized glass costs about 20 ms to write (its scene
  below at full resolution 4 ms, the horizontal pass 10 ms, the vertical 5 ms), far above the
  pass's fixed 0.45 ms.
- **H86's fix on the board**: `pending=59` of 576 flips kept queued until the next blank; no
  tear reported in this boot (the operator did not name one; not asked separately).
- **H89 still refuted**: `glass_w=` 31–34 per 32 frames in the steady windows — one glass is
  blurred again every frame, its identity unmeasured (the shell's top bar strip above the
  windows is the first suspect: its reach of 66 px touches any window whose top lies within
  it). Carried.
- **H87 open**: the scroll override reached the GPU's set; which windows were the wheel's is
  not told apart in the line. Carried with H89.
- Windows 18–138 ms; glass written 0.7–3.7 times a frame.

**Verdict.** The frame draws what changed: the present's damage with its origin, the layers
that moved, the glass blurred again only for a change below it, a slot brought up to date by
the frames it missed, a flip counted landed only when the controller took the plane. What
remains is named and measured, not guessed: the blur programs' cost a pixel (the lever for the
next package), one glass still rewritten every frame (its identity the first measurement of
that package), the glass cache's rows (two columns), the capture scrim as a screen-sized glass.
P5 complete.

## 5c-3 P6 — the glass at quarter resolution, and the gate scene (TASK-0215; 5c-4)

**Question.** P5 named the frame's cost: the passes' pixels at the programs' rates, the blur at
40.7 and 23.7 ns a half-resolution pixel (a window-sized glass about 20 ms to write). RFC-0101
§8d asks for the blur at quarter size, and the stock stack's bar is `glass-quarter` (23.3 ms,
`../2026-10-10-gpu-stock-baseline/`). Does the chain at quarter resolution bring a glass's write
near the stock's, and our frame of the gate scene below the bar?

**Measured first** (host 2026-10-19): the chain at quarter resolution against the
full-resolution gaussian (`tools/nxgpu/tests/blur.rs`, the checker backdrop, largest/mean
deviation in levels): radius 20 7/0.46 and 10/0.46 (plain, 140 % saturation), 30 7/0.43 and
10/0.51, 40 7/0.63 and 10/0.81; the half-resolution chain stays for radius 8 (5/0.44, 7/0.58 —
at quarter the kernel would be a pixel wide). Taps: radius 20 eleven fetches across and seven
down (21 and 11 at half); the programs 49/50, 65/60, 89/70 groups (89/70, 129/100, 169/120 at
half). The library: 13 entries, 10 012 bytes (13 812).

**What P6 builds.** `blur.wgsl` takes a `scale` override (2 or 4): a tap steps `scale` texels,
the weights the full-resolution gaussian's at those distances; `nexus_gfx::frame::blur_scale`
(half below 20, quarter from 20 on) drives the planner's cache rows, the board lowering, the QEMU
path and the board rung; the glass cache holds four times the glasses it did. 5c-4's gate: at
windowd's first frame the executor draws the stock's scene — the wallpaper and a screen-sized
glass at radius 40 — twice into the slot before windowd's own frame: cold (the glass written) and
warm (read from its cache): `gpud: gpu gate scene (cold_us= warm_us= passes= bar=23300)`, `ok`
when cold is below the bar; and the operator's `board-visual: gpu-desktop`. The frame line
names the glasses written (`glass_idx=`, a mask of layer indices) — the one blurred again every
frame in cycle 35 gets its identity.

| # | Hypothesis | Decision line |
|---|---|---|
| H90 | The quarter chain reads its gaussian on the board as on the host | `gpud: gpu pipeline ok (… blur_dev=` at most 13 and `gpu blur radii ok (r8=…/r30=…/r40=…)` within tolerance 10/13/13 |
| H91 | A quarter pass costs a quarter of the pixels at about the same rate | the pipeline gate's `blur_h_ns_px=` and `blur_v_ns_px=` (per reduced pixel) within 25 % of the half chain's 40.7/23.7 at the same taps a pixel, i.e. about 21/14 ns at eleven and seven fetches |
| H92 | Our frame of the gate scene is below the stock's | `gpud: gpu gate scene (cold_us=` below 23 300; `warm_us=` below 8 300 (a hover frame's bar, 5d's) |
| H93 | A typing frame rewrites no glass once the shell's strip is told apart | `glass_idx=` names one index across the typing windows; its layer is the shell's top-bar strip (2b) when the window's top lies within 66 px of it, else the finding is new |
| H94 | The frames are faster to the eye | typing windows `avg=` at or below 25 000 µs (49–54 before); the operator: smoother |

**Results — the board, cycle 36** (`board--2026-10-19T11-45-48`, `cycle36-gpu-lines.txt`): the
desktop never came — gpud died at windowd's first present.

- `init: service exit name=gpud reason=fault`, the kernel's report before it: `[USER-PF] STORE
  @ sepc=0x115e4a stval=0x1fff6c50 … sp=0x1fff6b50` — a store below the stack's mapped bottom
  (`0x1fff7000`: eight pages under the top). The executor (the frame planner's 5.8 KB, the
  pipelines' 2.8 KB, the rings, the per-layer bookkeeping — some 10 KB) lived inside `BoardGpu`,
  a local of the service loop, on that stack beside the loop's 8 KB receive buffer and the
  present path's frames; the gate scene's one extra call (`draw` → `gate` → `frame_of`) took the
  last bytes. Fixed: the executor is boxed (one allocation per context open; gpud's heap never
  frees — a windowd restart leaves 10 KB), and gpud's stack is 16 pages (the manifest's
  `stack_pages`).
- Before the fault, on the quarter chain: `gpu pipeline ok (… blur_h_ns_px=41.75
  blur_v_ns_px=12.36 … blur_dev=4 …)` and `gpu blur radii ok (r8=4/17.05/12.04 r30=1/53.93/9.11
  r40=1/75.36/20.18 tolerance=10/13/13)` — **H90 confirmed** (4, 1, 1 levels). **H91 refuted for
  the horizontal pass**: its cost a reduced pixel stayed at the half chain's (41.8 vs 40.7 ns at
  radius 20 — eleven fetches instead of twenty-one), while the vertical pass halved (12.4 vs
  23.7; 9.1 vs 35.8 at 30, 20.2 vs 43.7 at 40). The horizontal pass's time is not its fetches:
  with four times fewer pixels its share of a glass is still a quarter — its rate is the
  lever to measure next (the fetches' spread across the full-resolution backdrop, four texels
  apart, against the vertical pass's one-row steps in a 480-wide result).

**Results — the board, cycle 37** (`board--2026-10-19T11-54-18`, `cycle37-gpu-lines.txt`; the
operator: everything in order, the executor boxed and gpud's stack doubled):

- **H90 confirmed again**: `blur_dev=4`, the radii 4/1/1 levels.
- **The gate scene red**: `gpud: FAIL gpu gate scene (cold_us=34734 above the bar 23300;
  warm_us=34718 passes=4/4)`. The warm frame cost the cold frame's: the display's `base_dirty`
  (set for windowd's first present) was read for both gate frames, the base cleaned twice, the
  wallpaper generation bumped twice, the glass's key changed and the glass blurred again. The
  gate now cleans the base for the cold frame alone and draws three warm frames (one pass over a
  pointer-sized change: the frame's floor), and names the last pass's wait, the wait's
  iterations and the interrupts it saw (`pass_wait_us= iters= irqs=`).
- **The cold frame's 34.7 ms against the bar**: four passes — the base into the scratch (1920x1080
  opaque, 2.7 ms), the horizontal pass (482x292 at 43 ns, 6.1 ms), the vertical (482x272 at
  18 ns, 2.4 ms), the slot's (the base 2.7, the glass drawn back 2.7, the glass layer's own
  content — a screen-sized straight layer at 4.9 ns — 10 ms): about 27 ms of pixels, and the
  rest the passes' waits beyond their pixels. The stock's picture has no screen-sized straight
  content over its glass; ours draws the glass layer's own texture because a layer always has
  one — the warm frame's floor and the per-pass wait are what the next cycle names.
- **H91**: the vertical pass 18.2 ns a quarter pixel (23.7 at half), the horizontal 43.4 (40.7):
  the horizontal pass costs the same a pixel with half the fetches — its cost is not its
  fetches (carried: the rate of a pass over a wide, sparse source).
- **H93 open**: `glass_idx=0x6` through the typing windows — layers 1 and 2 blurred again every
  frame; which they are is named next cycle (`gpud: gpu glass written (…)`).
- **H94 refuted**: the typing windows 44–49 ms (49–54 before) — the glass writes decide, not
  the blur's rate.
- The per-pass wait from the windows' `split=`: 5–8 ms a pass whatever the pass (passes 2.4:
  19 ms of wait; 7.0: 48 ms) — the rungs' passes took 0.4 ms. The next cycle's warm frame (one
  pass, one pixel) measures the pass's floor directly, with its wait iterations and
  interrupts: a pass that waits 5 ms for a pixel is the firmware's or the device's doing, not
  the frame's.

**Results — the board, cycle 38** (`board--2026-10-19T12-05-58`, `cycle38-gpu-lines.txt`):

- **The floor, measured**: `gpud: FAIL gpu gate scene (cold_us=35679 … warm_us=12365/12337/12318
  passes=4/1 pass_wait_us=12175 iters=15 irqs=4)` — a warm frame is one pass over one pixel and
  waits **12.2 ms** for it, in fifteen one-millisecond wait slices with four interrupts; three
  warm frames alike. The rungs' passes took 0.4 ms (P4a) through the same wait on the same
  words. Not the pixels, not the CPU (`split=` named them), not the glass: the pass's completion
  reaches the word 12 ms after the submit in windowd's context. The next cycle traces the last
  warm pass iteration by iteration — the time, the interrupts seen, the geometry and fragment
  words (`gpud: gpu pass trace (…)`) — to tell a late device from a late word.
- **H93 answered**: `gpud: gpu glass written (i=1 rect=916,484,88x88 id=0 blur=20 …)`, `(i=3
  rect=837,626,246x44 id=0 blur=40 …)`, `(i=4..6 rect=1740|1796|1852,1020,44x44 id=0 blur=40
  …)` — the desktop's own glass regions: the dock's three buttons, a pill, an 88x88 field. The
  desktop surface changes within their rectangles (the shell repaints them), the damage's origin
  is the desktop (0) and counts as below every glass — their own content, blurred again every
  frame. Now a desktop change within a desktop glass region's rectangle is that region's own
  (`a_desktop_glass_region_keeps_its_blur_when_its_own_pixels_change`).
- **The capture hang, again**: dragging the screenshot's selection hung the input — the same
  signature as cycle 27 on the CPU path: windowd's last line `raise id=desktop`, then `dbg:
  hidrawd inputd send fail backpressure`, `hidrawd: chain I2 wire send FAIL (inputd route)` —
  inputd's queue backs up because windowd stops draining; gpud's lines go on (the flips, the
  frame windows). Outside the GPU path (windowd's capture drag: a send that never returns);
  carried to the screenshot track.
- `glass_w` through the typing windows 63–72 per 32 frames, `glass_idx=0x6|0x16|0x36|0x7e`.

**Results — the board, cycle 39** (`board--2026-10-19T12-26-10`, `cycle39-gpu-lines.txt`; the
operator: everything runs):

- **The floor named**: `gpud: gpu pass trace (iters=14 irqs=3 us_at:irqs:words[g=1,f=2]= 5:0:0
  85:1:0 500:2:1 1542:2:1 … 11381:2:1 12231:3:3)` — the one-pixel pass's geometry word is
  reached 0.5 ms after the submit (the second interrupt); the fragment word 12.2 ms after it,
  with the third interrupt, and nothing in between for eleven slices. The device is not
  drawing for those 11.7 ms: the firmware starts the pass's fragment job that long after its
  geometry job in windowd's context, where the rungs' fragment jobs followed within 0.2 ms
  (P4a). A latency, not a throughput: the lever is to keep the queue full — a frame's passes
  submitted one after the other and waited for once (the firmware runs a context's passes in
  order; a pass's fragment stage reads only what earlier passes wrote). Built: the executor
  pipelines a frame's passes (the last one waits; a pass that finds no line in the rings waits
  for the one before and tries again), and the gate draws the cold scene both ways
  (`pipelined_us=`).
- `gpud: FAIL gpu gate scene (cold_us=33835 … warm_us=12352/12559/12708 passes=4/1
  pass_wait_us=12232 iters=14 irqs=3)` — the warm frames 12.4–12.7 ms, all of it the one pass's
  latency.
- The dock's regions still blurred again every frame (`glass written` i=1, 3, 4, 5, 6 — the
  planner's own-content rule was built after this image). The windows 17–67 ms; one with
  `passes=1.6`: 17.6 ms, the floor plus little.

| # | Hypothesis | Decision line |
|---|---|---|
| H95 | The fragment job's 12 ms is a latency the queue hides | `pipelined_us=` of the gate's cold frame below half its `cold_us=` (33.8 ms sequential); and the typing windows' `avg=` below 25 000 µs |
| H96 | The desktop's own glass regions keep their blur | `glass_idx=` without bits 1 and 3–6 through the typing windows; `glass_w=` at most 32 per window (one glass a frame at most) |
| H97 | Our frame of the gate scene reaches the bar through the queue | `gpud: gpu gate scene ok (… pipelined_us=` below 23 300 |

**Results — the board, cycle 40** (`board--2026-10-19T12-36-10`, `cycle40-gpu-lines.txt`; the
operator: it ran, felt slower, the glass's corners square — the CPU path):

- **A pipelined frame locks the GPU up.** `gpud: FAIL gpu context reset (no context of the
  device: ctx=0x600 reason=1 dm=3 job=20 flags=0x1 pc=0x73d3001 fault=0x0)` during the gate's
  pipelined cold frame (reason 1: a guilty lockup, the fragment data master, the page-fault
  flag with no fault address), then `gpud: gpu context lost (ctx=0x600 reason=1 dm=3 job=80
  …)` in the desktop's frames and `FAIL gpu desktop (frame: the submit ContextLost …)` — the CPU
  composed from then on. The cause, read from the code: a context's two render-target data
  alternate per pass (`render.rs`, as the open driver does), so at most two passes may be in
  flight; the executor queued a whole frame. **H95 and H97 are not decided**: the gate's
  `pipelined_us=14537` was reached through the reset (a lost context's points count as
  reached). Now a frame keeps two passes in flight — pass p waits for pass p − 2 — and the gate
  measures that.
- **Two attribution faults in the reset's handling**: the catalogue the firmware named carried
  its valid bit (`0x73d3001` against the context's `0x73d3000`), and a catalogue that matched
  no context was not followed by the context the message numbered (`ctx=0x600`). Both fixed:
  the catalogue's page compared, the numbered context the fallback.
- The warm frames 12.4–12.7 ms as in cycle 39 (`pass_wait_us=13261 iters=19 irqs=8`).

| # | Hypothesis | Decision line |
|---|---|---|
| H98 | Two passes in flight run without a lockup and hide the fragment's latency | no `gpud: FAIL gpu context reset (` nor `gpu context lost (ctx=0x600`; the gate's `pipelined_us=` below `cold_us=` by at least a third, and valid (no reset before it) |

**Results — the board, cycle 41** (`board--2026-10-19T12-56-01`, `cycle41-gpu-lines.txt`; the
operator: everything runs, feels quite good — the GPU path through the whole boot, no reset in
windowd's context):

- **H98 half**: no lockup with two passes in flight; but the queue hides nothing —
  `gpud: FAIL gpu gate scene (cold_us=35644 pipelined_us=33051 … warm_us=12364/12323/12783
  pass_wait_us=14846 iters=17 irqs=3)`. The fragment job's 12 ms follows its geometry whatever
  is queued behind it: a cost per fragment job, not a latency the queue overlaps. On this
  firmware a pass costs about 8–12 ms however small; a frame costs its passes. The typing
  windows: `avg=42418–42845 passes=6.9 glass_w=63` — two glasses written a frame still.
- **Still the desktop's regions**: `glass written` i=1, 3, 4, 5, 6 as in cycle 38 — the
  own-content rule did not catch them: the shell's damage rectangles must be larger than the
  regions (measured next: `gpud: gpu glass damage (rects=…)` beside the glasses named). And a
  planner fault found by reading: a window's glass slices share one id, and a window's own
  change counted as below its second slice (the first slice's index is lower) — fixed, tested.
- The rest of a typing frame: 7 passes × about 6 ms. The structural lever on this firmware is
  the number of passes a frame runs (the glass chains of a frame drawn in one scratch pass, one
  horizontal and one vertical pass, whatever their count), not their pixels — the next package.

**Results — the board, cycle 42** (`board--2026-10-19T13-10-06`, `cycle42-gpu-lines.txt`):

- **The dock's regions, explained**: `gpud: gpu glass damage (full=false rects= 0,0,1920x1080@0)`
  — the present that blurred them again carried one damage rectangle: the whole screen, from the
  desktop. windowd turns a desktop surface present that declares no rectangles into whole-screen
  damage (`queue_full_frame_damage`), and the shell presents without rectangles — on every
  keystroke too (the typing windows' `glass_idx=0x6`, two glasses a frame, both touched by a
  screen-sized desktop change). The planner's rules are right; the input is coarse: the
  shell's tree knows what it repainted and must say so (the next package's first item — in
  the app-host, where the rectangles are known, not in windowd).
- Frames: 14–84 ms; the passes still 6–7 a typing frame, the wait 97 % of them.

**Verdict (P6, and 5c-4's gate).** The glass runs at quarter resolution on both paths within
ten levels of the full-resolution gaussian (H90 ✅), a frame keeps two passes in flight without
a lockup, a reset is held against the context the firmware names or numbers, and the frame
planner blurs a glass again only for a change below it that is not the layer's own. The gate
scene is **red with its cause named**: our cold frame of the stock's `glass-quarter` picture
costs 33–36 ms against the stock's 23.3 ms, and the cost is its four passes at 8–12 ms each —
a one-pixel pass waits 12 ms for its fragment job, which the firmware starts that long after
the pass's geometry job in windowd's context (the rungs' followed within 0.2 ms); two passes in
flight hide nothing of it. Three levers, each measured here and none of them the planner's:
the fragment job's start in windowd's context (a firmware-side investigation: what the rungs'
context had that windowd's has not — the first item of the next package), the number of passes
a frame runs (every glass chain of a frame in one scratch, one horizontal and one vertical
pass), and the shell's damage rectangles (the app-host presents whole surfaces and full-width
row bands, so a desktop change touches every glass in its rows). 5c-3 P6 complete; 5c-4's bar
carried with these three.

## 5d — the CPU composite deleted on the board (TASK-0215; layer 5 complete)

**Question.** Since cycle 31 every present on the board has been the GPU's; the CPU composite
(`DcDisplay::compose`: the retained plane copied into the slot, the commands run by the CPU
executor, the slot cleaned) was reached only when the GPU refused — cycle 40's reset, which the
operator saw as square blur corners and a slower desktop. With that path deleted, does the
board's desktop run through a boot with every present the GPU's and nothing refused; and where
does a hover frame — a pointer-sized change over the cached glass — stand against the stock
stack's 8.3 ms?

**Measured first** (cycle 42's log, `board--2026-10-19T13-10-06`): the present windows' stage
times are zero (`gpud: dc present us (… blur=0 layer=0 shadow=0 blit=0 fill=0 …)` — no CPU
command ran); windowd's context opens before its first present (`gpu library ok
(client=windowd` at line 824, `dc first present` at 947); no `FAIL gpu desktop` since cycle 31.
The hover frame exists as the gate scene's warm frame (cycles 38–42: `warm_us=12277/12350/12664`
and the like), its cost the one pass's fragment start (P6).

**What 5d builds.** `execute_on` hands the present to the GPU or refuses it by name —
`gpud: FAIL dc present (windowd's GPU context is not open)` / `(the GPU did not draw the
frame)`, once, the present's status an error, the ring's slot given back; `compose`, the
retained→slot copy and the CPU target are deleted; the pointer's shape cache is its own module
(`backend/cursor_shapes.rs`), the CPU executor and its raster family the 2D path's alone. The
gate scene line carries the hover frame: `hover_us=` (the best warm frame), `hover_bar=8300`;
`ok` only when the scene and the hover frame are both below their bars, the FAIL form naming
which is above.

| # | Hypothesis | Decision line |
|---|---|---|
| H99 | The board's desktop runs through a boot with every present the GPU's: nothing refused | no `gpud: FAIL dc present (` in the boot; `gpud: gpu desktop ok (`; the operator's desktop, typed input, scrolling, screenshot |
| H100 | The hover frame is the warm frame of cycles 38–42: 12.3 ms, above the 8.3 ms bar for the fragment start | `hover_us=` within 12 000–13 000 in `gpud: FAIL gpu gate scene (… hover_bar=8300: …)` |
| H101 | The present windows read as before — the GPU's frames, no CPU stage time | `gpud: dc present us (… blur=0 layer=0 shadow=0 blit=0 fill=0 other=0 …)`, `avg=` in the range of cycles 41–42 |

### Results (cycle 43, `board--2026-10-19T16-23-56`; `cycle43-gpu-lines.txt`, `cycle43-board-test.txt`)

- **Nothing refused**: no `gpud: FAIL dc present (` in the boot; `gpud: gpu desktop ok
  (frame_us=6168 passes=1 … clean_us=905 open_us=20388)`, `windowd: desktop revealed (seq=23)`,
  the glasses written once per window as in cycle 42. The operator: the desktop, typed input,
  the pointer, a dialog, the clipboard and a screenshot confirmed (`tile` not: TASK-0066 F8).
  H99 ✅.
- **The hover frame**: `gpud: FAIL gpu gate scene (cold_us=35676 pipelined_us=33529 bar=23300
  hover_us=12367 hover_bar=8300: the scene above its bar and the hover frame above its bar;
  warm_us=12367/12457/12655 passes=4/1 pass_wait_us=14850 iters=16 …)` — 12.4 ms, the one
  pass's fragment start as in cycles 38–42. H100 ✅ (the bar carried to layer 8).
- **The present windows**: fifteen `gpud: dc present us (…)` lines, every stage time zero
  (`blur=0 layer=0 shadow=0 blit=0 fill=0 other=0`), `avg=` 21.6–85.6 ms, `clean=` 0–1 µs — the
  GPU's frames alone, as in cycles 41–42 (the averages the frames' glass writes; the shell's
  damage rectangles, layer 8). H101 ✅.
- **Carried, not 5d's**: six `gpud: chain G3 exec FAIL` / `chain reason: resource exhausted` /
  `present scanout damage FAIL` runs with `windowd: gpud present nack status=0x02` and a retry
  — the ring's three planes all taken during a present burst (one scanned, one flip queued,
  one waiting for the blank) and windowd's back-pressure answering; in every cycle since 33
  (4–7 a boot), none before. The ring's size and the compositor's pacing against the blank are
  the frame clock's item (RFC-0101 §8, layer 8's closing).
- Ladder: 70 rungs present, 13 tolerated reds, `board-visual: tile` missing (as cycles 31–42).

**Verdict (5d; layer 5).** The board's display composes nothing on the CPU: every present of
the boot is the GPU's or would be refused by name, and none was. The hover frame stands at
12.4 ms against the stock's 8.3 ms for the cause P6 measured; the bar is carried to Block 4
layer 8 with the gate scene's, the passes a frame runs, the shell's damage rectangles, the
planes for what moves and windowd's CPU scene. Layer 5 complete.
