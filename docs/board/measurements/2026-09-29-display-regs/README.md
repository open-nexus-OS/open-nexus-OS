# 2026-09-29 — the display pipeline at 1920x1080@60, measured on the stock system (D0)

The measurement half of TASK-0250 P0 / TASK-0251 P0 / TASK-0245B P3 (display set): what the
display controller and the HDMI encoder look like while the stock system shows a still desktop
at the SoC's maximum mode, what the pipeline needs from the SoC glue, and how the buffers reach
the controller. Every number below is read from the board; the vendor kernel's register headers
and the boot loader's splash driver were read as a NAME reference only (never ported).

## Instrument

- Stock system on the microSD, root over adb; `busybox devmem` for single words; a 32-bit-load
  `mmap` reader (`/dev/mem`, the `dd` path faults on MMIO) for the windows; debugfs for the
  clock tree, the APMU regmap and the DRM state.
- Files: `dpu-regs-on.bin` (0xc0440000 + 0x2a000), `hdmi-regs-on.bin` (0xc0400500 + 0x200),
  `dpu-regs-on-annotated.txt` (every live word with its field name), `dpu-off.bin` /
  `hdmi-off.bin` / `apmu-off.txt` / `clk-off.txt` (layer blanked through fb0), `apmu-on.txt`,
  `clk-summary-on.txt`, `drm-state-1080p60.txt`, `dt-stock-display-nodes.txt`,
  `dt-phandles-drm-dmesg.txt`, `oops-pinctrl-dram-fb.txt`, `edid-hdmi-2026-09-29.bin` +
  `edid-decode.txt`, `dpu-reserved-2ff40000.bin` (the 768 KiB the stock driver reserves).

## Results

### The mode (DRM state, EDID)

| | |
|---|---|
| mode | `1920x1080` 60 Hz, pixel clock 148.500 MHz, h 1920 2008 2052 2200, v 1080 1082 1087 1125, +hsync +vsync (VIC 16) |
| hfp / hsync / hbp | 88 / 44 / 148 |
| vfp / vsync / vbp | 2 / 5 / 38 |
| framebuffer | one plane, `XR24` (XRGB8888), pitch 7680, 1920x1080, in CMA at CPU 0x8fb7c000 (bank 0) |
| EDID | 256 bytes, identical to 2026-09-22; preferred 2560x1440 (DTD 1), 1920x1080@60 = VIC 16 and DTD 2, 16:9 |

### The stock tree's display nodes (what the pipeline really needs)

| node | reg | clocks | resets | power domain | interrupts | other |
|---|---|---|---|---|---|---|
| `display-subsystem-hdmi` (`spacemit,saturn-hdmi`) | 0xc0440000 + 0x2a000 | — | — | — | — | `ports` → the online2 port; interconnect `dram_range@1` |
| `port@c0440000` (`spacemit,dpu-online2`, `ip = "spacemit-saturn"`, pipeline-id 2) | (parent's) | **`hmclk` only** (clock 0x98 of the syscon) | `hdmi_reset` (0x59) | **domain 7** | 139 `ONLINE_IRQ`, 138 `OFFLINE_IRQ` | `memory-region` → `dpu_reserved@2ff40000` (768 KiB, `shared-dma-pool`) |
| `hdmi@C0400500` (`spacemit,hdmi`) | 0xc0400500 + 0x200 | `hmclk` | `hdmi_reset` | domain 7 | 136 | `pinctrl-0` → `hdmi_0_grp`: pads 0x1ec, 0x1f0 (mux 1, 0xd040), 0x1f4, 0x1f8 (mux 1, 0xb040) |

- **The pipeline runs on `hmclk` alone.** `clk_summary` with the picture on: `hdmi_mclk`
  491.52 MHz enabled (pll1_2457p6_vco → pll1_d5 → pll1_d5_491p52), consumers `hdmi@C0400500`
  and `port@c0440000`; `dpu_pxclk`, `dpu_mclk`, `dpu_bit_clk`, `dpu_hclk`, `dpu_esc_clk` all
  **disabled** (they belong to the DSI pipeline). The vendor driver's HDMI path enables exactly
  `hmclk` and sets it to its default rate. → D3 trims our `dpu` node to `hmclk` + `hdmi_reset`.
- **No DDC I²C bus in the tree:** the HDMI block has its own DDC master (registers 0x0/0x4/0x8/0xc:
  address, data, command, status with a "done" bit 14 and a byte count in bits 4..8) and reads
  the EDID at 0x50 in 16-byte chunks; HPD is bit 12 of the PHY status register (0xc).
- **Reach:** `dram_range@1` (`spacemit-dram-bus`) `dma-ranges` = child 0x0 → parent 0x8000_0000
  (0x8000_0000 long) and child 0x8000_0000 → parent 0x1_0000_0000 (0x3_8000_0000 long). The
  stock framebuffer is at CPU 0x8fb7c000 = bus 0x0fb7c000 — **bank 0 is reachable; the scanout
  buffer does not have to live in bank 1** (the plan assumed it might).

### The controller's block map (read from the live words; names from the vendor headers)

| block | base in the window | live at 1080p60 |
|---|---|---|
| top (version) | 0x0 | 0x03001030 (product 0x300, major 0x10, minor 0x30) |
| control `DPU_CTL` | 0x500 | ctl2: `nml_rch_en` = 0b10 (RDMA1), `nml_outctl_en` = 1, `video_mod` = 1, timing intervals 8/15 |
| clock/reset gating `CRG` | 0x700 | every auto-gate bit set (0x1fff, …) |
| command list | 0x800 | channel base 0x2ff45000 (in `dpu_reserved`) — the stock driver replays register batches from memory |
| interrupts | 0x900 | online2 masks 0xf212 (eof, cfg_rdy_clr, underflow, …); raw vsync/cfg_eof/cfg_line pending |
| DMA top | 0xa00 | qos 0x42223 (online read qos 3), dmac0..3 reset released, outstanding 8/32 |
| RDMA0..11 | 0xa80 + i·0x100 | RDMA0 and RDMA1: composer 2, stride 7680, 1920x1080, bbox (0,0)-(1919,1079), `pixel_format` 8 (XR24), base = **IOVA** 0x1000_0000 / 0x9000_0000 (the display MMU is on) |
| display MMU | 0x1680 (top), 0x1780 (TBU) | `mmu_cg_en`, dmac read outstanding 8, TBU tables at 0x2ff48000 (in `dpu_reserved`) |
| layer processing | 0x2000 + i·0x300 | all zero |
| composers CMPS0..3 | 0x4600 + i·0x300 | CMPS2: enable, 1920x1080, bg G=255 A=255, layer 7 = RDMA layer_id 1, rect (0,0)-(1919,1079), blend 1, alpha 255 |
| scalers | 0x6000, 0x6200 | — |
| output control OUTCTRL0..3 | 0x7000 + i·0x8800 (OUTCTRL2 = **0x18000**) | in 1920x1080, `frame_timing_en`, dither on (5/6/5, bayer map), `hfp=88 hbp=148` (0x18080), `vfp=2 vbp=38` (0x18084), `hsync=44 hsp=1 vsync=5 vsp=1` (0x18088), `h_active=1920 v_active=1080` (0x1808c), format word 2 (0x18090), irq masks 0x252, line counters ticking |
| writeback | 0x29000 | live defaults |

Blanking the layer through `fb0/blank` changed 17 words (the RDMA arbitration debug word,
composer/output line counters, MMU statistics) and APMU 0x090 bit 23; the HDMI block and
`hmclk` stayed as they were — a layer blank, not a pipeline power-down.

### The first-light sequence (the boot loader's splash driver, cross-checked against the live words)

About thirty direct register writes and no command list, no display MMU (`TBU_Ctrl = 0`,
the RDMA base holds the **bus address** of a contiguous buffer — the vendor kernel has the same
"contiguous memory" path):

1. DMA top qos `0xa1c = 0x2223`.
2. OUTCTRL2: `0x18000` = (v << 16) | h; `0x18018 = 0x20` (frame timing on); `0x1807c = 0x100`;
   `0x18080 = (hbp << 16) | hfp`; `0x18084 = (vbp << 16) | vfp`;
   `0x18088 = (vsp << 28) | (vsync << 16) | (hsp << 12) | hsync`; `0x1808c = (v << 16) | h`;
   `0x18090` = output format (2 = RGB888).
3. RDMA3 (`0xd80`): ctrl `0x202040`; `0xda0/0xda4` = buffer bus address low/high;
   `0xdb8` = stride (h·4); `0xdbc` = (v << 16) | h; `0xdc0 = 0`; `0xdc4` = ((v−1) << 16) | (h−1);
   `0xdf0 = 4` (pixel format; the stock kernel uses 8 for XR24 — D2 pins the table).
4. CMPS2 (`0x4c00`): `(h << 8) | 1` (enable + width); `0x4c04` = v; bg `0x4c10 = 0xff0000`,
   `0x4c14 = 0xff`; layer `0x4c38 = 7`; rect `0x4c48 = 0`, `0x4c4c = (h−1) << 16`, `0x4c50 = v−1`;
   `0x4c54 = 0xff0000`.
5. DPU_CTL: `0x560 = 0x40008` (ctl2: RDMA3 + outctl); `0x588 = 0x821` (video mode, timing
   intervals); `0x56c = 1` (cfg ready); `0x58c = 1` (sw start, write-1-clear).
6. The buffer is written with the CPU and **flushed** (`flush_cache`) before the DMA reads it —
   the controller is not coherent with the CPU caches (D4's `cbo.clean` per damage rectangle).

HDMI encoder (0xc0400500), for a pixel clock `f` in MHz at 8 bpc (bit clock = f):
PLL words at `0xe8` (fraction, `0x20 << 24`), `0xec` (integer part, dividers), `0xf0`
(post-dividers), PHY bias at `0xe0` (base 0xAE5C410F with bias/resistor/phase fields), color
depth at `0x34` (`0xd | depth << 4`), PHY reset/enable `0xe4` = 0 then 3, then `0x28`. The live
words agree: `0xe0 = 0xae5c010f`, `0xe4 = 0x00010003`, `0xf0 = 0x0821`, `0x34 = 0x4d`,
`0xec = 0x509d453e`, `0xe8 = 0x203f0000`. HPD: `0xc` bit 12; DDC through 0x0/0x4/0x8/0xc.

### The power-down protocol (domain 7) — NOT measured yet

Unbinding the stock driver (`spacemit-drm-drv/unbind`) oopsed the vendor driver in
`drm_dev_unregister` (`oops-pinctrl-dram-fb.txt`); the system survived, the pipeline stayed up.
The remaining honest instrument is the HDMI cable: an unplug lets the driver take the encoder
down (HPD), a replug brings it back — APMU/clock/register diffs then show what the driver
touches for the encoder. Domain 7's own on/off bits stay a D3 experiment on our own chain
(read-back, capped poll, one write at a time), since the stock kernel never drops the domain
while it runs.

## Verdict (decisions for D2/D3/D4)

1. **`hmclk` alone + `hdmi_reset` + domain 7** — the `dpu` node's five DSI clocks and three
   resets in `config/board/bpi-f3/board.dts` are trimmed to the measured truth (D3).
2. **Direct MMIO, no command list, no display MMU** — the splash sequence is the model: one
   contiguous XR24 buffer whose **bus address** (CPU − 0x8000_0000 for bank 0) goes to the RDMA,
   composer 2 → output control 2 → HDMI. D2's register model is this table; D4 programs it.
3. **The buffer is non-coherent** — `cbo.clean` of the damage before every flush; the mode's
   frame time (16.7 ms) is the pacing, the ONLINE IRQ (139) the fence.
4. **EDID over the encoder's own DDC** (registers 0x0..0xc) — no I²C controller needed; HPD is a
   status bit. `pick_mode` = highest mode ≤ 1920x1080@60 with the monitor's aspect (16:9 here).
5. **Format table to pin in D2:** RDMA `pixel_format` 8 = XR24 (stock kernel at 1080p), the
   splash uses 4 with its own format enum — one host golden per value we emit.
