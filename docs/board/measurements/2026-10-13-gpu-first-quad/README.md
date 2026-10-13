<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# 2026-10-13 — the first quad: the open driver's transfer jobs captured on the host, replayed on the board (TASK-0169B, layer 4)

## Question

What exactly does the open userspace driver for this core submit to the kernel for a clear and
a filtered, scaled blit of a 256×256 texture into a linear 1920×1080 BGRA target — which buffers
at which GPU addresses, which command streams — and do those bytes, bound at the same addresses
by our device and pointed at a plane of the ring, draw the same picture on the board, with a
full-plane fill at or below the stock stack's 1.8 ns a pixel (`../2026-10-10-gpu-stock-baseline/`)?

## Instrument

**Host (the capture).** The open userspace driver built from source with its kernel-stand-in shim
(`-Dvulkan-drivers=imagination -Dtools=drm-shim`, in a volatile container because the driver's
build needs an OpenCL-C compiler for its internal shader library). The shim mocks the device from
the environment (`PVR_SHIM_DEVICE_BVNC=36.29.52.182`; the driver's device table carries this BVNC,
the kernel-side heaps match ours: general `0x80_0000_0000`, PDS `0xDA_0000_0000`, USC
`0xE0_0000_0000`). Our hook in the shim (`capture/pvr_dump_hook.h`) records every `VM_MAP` /
`VM_UNMAP`, assigns context handles, and on every `SUBMIT_JOBS` writes each job's command stream
(`jobN.stream`), its header (`jobN.txt`: type, context, flags, stream length, sync count) and
**every live bound buffer's bytes from the shim's memory file** as `jobN-bo-<va>.bin`. The
program (`capture/first_quad.c`) creates a linear 1920×1080 `B8G8R8A8_UNORM` target and a linear,
host-written 256×256 `R8G8B8A8_UNORM` texture (a 32-px checker of two colours under a diagonal
alpha ramp with a 4-px white frame; `texture.sha256` is its digest), clears the target to
(0.10, 0.12, 0.16, 1.0) and blits the whole texture, linearly filtered, to the rectangle
(704, 284)–(1216, 796). The driver's non-conformance override is required (its
`PVR_I_WANT_A_BROKEN_VULKAN_DRIVER=1`); its `info` debug output confirms the device (`BXE-2-32`,
BVNC 36.29.52.182, `core_count 1`, `num_phantoms 1`). The bundle the capture became:
`resources/gpu/scenes/first-quad.scene` (packer: `examples/scene_pack.rs`; provenance and digest
in `resources/gpu/scenes/README.md`).

**Board (the replay).** `gpud`'s lines after `gpu fence ok`: `gpud: gpu fill ok (us= px=1920x1080
ns_per_px=)` and `gpud: gpu quad ok (us= samples=4/4)`, their FAIL forms (sample index, expected,
read; or the FWCCB fault type and address), and the operator's eyes (`[PASS] board-visible`).

## Results — the capture (host, 2026-10-13)

Two jobs, both `type=3` (transfer), on context 1 (`type=2`, the transfer context), `flags=0x1`
(single core), stream length 176: 8 header bytes (`main_len=0xb0`, padding 0), 4 + 9 64-bit
words, then 16 32-bit words — the last, `frag_screen`, present because the core has
`GPU_MULTICORE_SUPPORT` (the firmware's device-info block says so too; `isp_xtp_pipe_enable` is
absent: no `S7_TOP_INFRASTRUCTURE`). The streams decoded by the open kernel driver's definition:

| Register | job 0 (clear) | job 1 (blit) |
|---|---|---|
| `pds_bgnd0_base` | 0 | `0x1e40_0004_1000` (the per-blit PDS data, the job-1-only buffer at `0xda00041000`) |
| `pds_bgnd1_base` | 0 | `0x2640_0000_0000` |
| `pds_bgnd3_sizeinfo` | 0 | `0x0100_0000_0001_0001` |
| `isp_mtile_base` | 0 | 0 |
| `pbe_wordx_mrty[0..2]` | `0x0200_0efc_60a0_0000`, `0x0000_077f_0000_0437`, 0 | `0x0200_0efc_60a0_b000`, `0x0047_04bf_0000_031b`, 0 |
| `isp_bgobjvals` | 0 | `0x200` |
| `usc_pixel_output_ctrl` | `0x7f8` | `0x7f8` |
| `usc_clear_register0` | `0xff291f1a` — the pixel program's output word in RGBA order: R 0x1a (0.10), G 0x1f (0.12), B 0x29 (0.16), A 0xff; the back end writes it to memory as B G R A = `29 1f 1a ff` | 0 |
| `usc_clear_register1..3` | 0 | 0 |
| `isp_mtile_size` | `0x0078_0044` | `0x0020_0021` |
| `isp_render_origin` | 0 | `0x002c_0011` |
| `isp_ctl` | `0x21000` | `0x21000` |
| `isp_aa` | 0 | 0 |
| `event_pixel_pds_info` | `0x202` | `0x202` |
| `event_pixel_pds_code` / `_data` | `0x2620` / `0x2600` | `0x26a0` / `0x2680` (offsets into the PDS heap: the end-of-tile program in the buffer at `0xda00001000`) |
| `isp_render` | `0x3` | `0x3` |
| `isp_rgn` | 0 | 0 |
| `frag_screen` | `0x0437_077f` | `0x0437_077f` |

Buffers bound at submit (28; `jobN.txt` lists them; `log.txt` the order of creation):

| GPU address | Size | Non-zero bytes | Role (from the creation order and the stream) |
|---|---|---|---|
| `0x8000000000` | 4 KiB | 148 | static data area, general heap |
| `0x8000001000`, `0x8000005000` | 16 KiB each | 0 | render-context scratch (unused by transfer) |
| `0x8000009000` | 256 KiB | 0 | the free list's pages (render only) |
| `0x8000049000` | 4 KiB | 0 | — |
| `0x800004a000` | 4 KiB | 15 | transfer context state words |
| `0x800004b000` | 128 KiB | 12 → 32 after job 1 | texture / sampler state words of the blit |
| `0x800006b000`–`0x800006f000` | 4 KiB each | 0 | context state / call stack of the three other contexts |
| `0x8000070000` | 4 KiB | 41 | — |
| `0x8000071000` | 256 KiB | 383 | the blit's shader-side constants |
| **`0x80000b1000`** | **8 294 400 B** | 0 | **the target** — a plane of the ring on the board (stride 7680 = ours) |
| **`0x800089a000`** | **256 KiB** | 262 141 | **the texture** (row pitch 1024) — generated on the board by the same formula |
| `0xda00000000` | 4 KiB | 4 | static data area, PDS heap |
| `0xda00001000` | 128 KiB | 998 → 1018 | PDS programs (end-of-tile; the blit's) |
| `0xda00021000` … `0xda0003c000` | 4–20 KiB | 0 | PDS areas of the other contexts |
| `0xda00041000` | 4 KiB | 4 | job 1 only: the blit's PDS background data |
| `0xe000000000` | 4 KiB | 5 | static data area, USC heap |
| `0xe000001000` | 128 KiB | 20 147 → 20 356 | USC code (the driver's shader library + the two jobs' pixel programs) |

Decisions the capture took: the target's row pitch is 7680 and its memory 8 294 400 bytes —
exactly one plane of the ring (`nexus_display_proto::layout`), so the target binding is the
plane's `vmo_runs` and no copy; the texture is procedural, so the scene bundle carries only the
driver's bytes (sparse non-zero runs, ~44 KiB) and the board generates the texture; the open
driver's transfer path has no source-over blend (its "blend" resolve averages samples), so the
blended quad is layer 5's render-queue work.

## Hypotheses the first board cycle decides

| # | Hypothesis | Decision line |
|---|---|---|
| H1 | The firmware takes a `TQ_3D` command in the transfer context like the null job | `gpu fill ok` + corner pixel = clear colour; FWCCB fault address inside a scene range = our binding, outside = the stream |
| H2 | The captured streams and binaries run unchanged on the core | the four sample pixels; wrong colour = PBE/texture state; hang = PDS/USC addresses |
| H3 | A full-plane fill ≤ 1.8 ns/px (3.7 ms) | `ns_per_px=` in `gpu fill ok` |
| H4 | Clean before the GPU writes, invalidate before the readback | samples show the GPU's pixels only with both |
| H5 | Firmware boot variance is CPU-side load | `boot_us` across the cycles |

## Results — the board (cycle 1, 2026-10-13, `build/logs/board--2026-10-13T16-04-35`)

Green on the first cycle; the lines (`cycle1-gpu-lines.txt`):

```text
gpud: gpu fw ok (boot_us=1085 health_us=82 irqs=1 fw_irqs=1 executed=1 …)
gpud: gpu mmu ok (map_us=61 unmap_us=9683 … table_pages=1)
gpud: gpu fence ok (rt_us=109 worst_us=1802 n=64 irqs=64 below the stock 640us)
gpud: gpu fill ok (us=1857 px=1920x1080 ns_per_px=0.8 at or below the stock 1.8 bind_us=4603)
gpud: dc reveal flip ok (bus=0x8fd2000 stride=7680 readback 3/3 cfg-ready cleared after 0 reads)
gpud: gpu quad ok (us=1323 samples=4/4 shown row=2160)
```

| What | Measured | Bar |
|---|---|---|
| Binding the bundle (29 buffers, 28 flushes, ~2400 pages) | 4 603 µs | — |
| The clear of 1920×1080 (kick → completion seen) | 1 857 µs = **0.8 ns/px** | stock 1.8 ns/px (3.7 ms) |
| The blit of the 512×512 quad (kick → completion) | 1 323 µs | — |
| Sample pixels after the flush | 4 of 4 within ±2 | 4 of 4 |
| The lane | `[PASS] board-visible: 57 rungs, 7 operator marker(s), no FAIL marker` | — |

| # | Decided | By |
|---|---|---|
| H1 | ✅ the firmware takes the `TQ_3D` command in G1's transfer context like the null job | `gpu fill ok`, no FWCCB fault, one interrupt per job |
| H2 | ✅ the open driver's streams and binaries for this BVNC run unchanged on the core | the four samples: background `29 1f 1a`, frame white, both checker colours |
| H3 | ✅ a full-plane fill at 0.8 ns/px, 2.3× below the stock 1.8 | `ns_per_px=0.8` |
| H4 | ◐ with clean before and flush after, the readback shows the GPU's pixels; the necessity of each half was not tested (no cycle without them) | samples 4/4 |
| H5 | ◐ the firmware's boot time was 1.1 ms this boot (10.9 ms in layer 3's last cycle): the variance is still unexplained; this cycle adds `unmap_us=9683` (50 µs in layer 3) — one firmware MMU-cache flush took 9.7 ms | `boot_us`, `unmap_us` |
| H6 | ◐ the operator saw the ladder's pictures (acked 7 markers); the quad's look under the controller's alpha handling was not described separately | operator |

**The gate shown red** (`cycle1-gate-red.txt`): the same capture with the `gpu quad ok` line
removed is refused by the lane — `rung missing: 'gpud: gpu quad ok (' (rung 29 of 57, last
present: 'gpud: gpu fill ok (')`. A device-side red (the FAIL form with a sample's expected and
read bytes, or a firmware fault) did not occur: the first cycle was green.

## Verdict

The open driver's output for this core **is** the hardware's language: two transfer jobs captured
on a host without the hardware, bound at their addresses by our device and kicked through the
same context as the null job, drew the clear and the textured, filtered, scaled quad into a plane
of the ring — pixel-exact at four samples, with a full-plane fill at 0.8 ns a pixel against the
stock stack's 1.8. RFC-0101 Phase 3's gate is met; `GfxDevice` exists on hardware. Handed on: the
firmware's time variance (boot 1.1–10.9 ms, one 9.7 ms flush) to layer 5's first cycle with the
firmware trace; the alpha question to the same cycle (a textured quad with alpha 0 at a known
pixel, the controller's word read back); the render queue and blending to layer 5.
