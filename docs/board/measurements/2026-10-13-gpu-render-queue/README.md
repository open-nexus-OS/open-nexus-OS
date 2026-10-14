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
