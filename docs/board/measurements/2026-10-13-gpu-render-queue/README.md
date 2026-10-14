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
