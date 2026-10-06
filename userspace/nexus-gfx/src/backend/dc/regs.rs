// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The display controller's register map as MEASURED on the reference board
//! (2026-09-29, `docs/board/measurements/2026-09-29-display-regs/README.md` §"block map" and
//! §"first-light sequence"): the block bases of the controller window (0xc0440000 + 0x2a000)
//! and, inside the blocks the first-light path touches, the word offsets it writes. The names
//! are ours; the vendor kernel's headers were read as a name reference only. A value here is a
//! fact from the live dump (`dpu-regs-on-annotated.txt`) or the boot loader's splash sequence;
//! nothing is inferred.
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `model.rs` goldens reproduce the measured words at these offsets

/// Length of the controller window the tree names (`reg = <0xc0440000 0x2a000>`).
pub const WINDOW_LEN: u32 = 0x2a000;

/// The pipeline this board's HDMI path uses (`pipeline-id = 2`, "online2"): its composer,
/// its output control and its control word are the third of each.
pub const PIPELINE: u32 = 2;

// ── Block bases ────────────────────────────────────────────────────────────────────────────

/// Per-pipeline control words (`ctl<n>_*`), 0x500; each pipeline's words are 0x28 apart
/// (measured: ctl0 timing intervals at 0x528, ctl1 at 0x558, ctl2 at 0x588).
pub const DPU_CTL_BASE: u32 = 0x500;
/// DMA top (arbitration, qos, dmac resets), 0xa00.
pub const DMA_TOP_BASE: u32 = 0xa00;
/// RDMA channel 0; channels are 0x100 apart (measured: stride at 0xab8, 0xbb8, 0xcb8).
pub const RDMA0_BASE: u32 = 0xa80;
pub const RDMA_STRIDE: u32 = 0x100;
/// Composer 0; composers are 0x300 apart (measured: composer 2 at 0x4c00).
pub const CMPS0_BASE: u32 = 0x4600;
pub const CMPS_STRIDE: u32 = 0x300;
/// Output control 0; output controls are 0x8800 apart (measured: output control 2 at
/// 0x18000 holds the 1920x1080@60 timing).
pub const OUTCTRL0_BASE: u32 = 0x7000;
pub const OUTCTRL_STRIDE: u32 = 0x8800;

/// Base of RDMA channel `i`.
pub const fn rdma_base(i: u32) -> u32 {
    RDMA0_BASE + i * RDMA_STRIDE
}
/// Base of composer `i`.
pub const fn cmps_base(i: u32) -> u32 {
    CMPS0_BASE + i * CMPS_STRIDE
}
/// Base of output control `i`.
pub const fn outctrl_base(i: u32) -> u32 {
    OUTCTRL0_BASE + i * OUTCTRL_STRIDE
}

// ── DMA top ────────────────────────────────────────────────────────────────────────────────

/// Read/write qos word; the splash writes 0x2223, the live dump holds 0x42223 (a cmdlist qos
/// nibble the splash never sets).
pub const DMA_TOP_QOS: u32 = DMA_TOP_BASE + 0x1c;
pub const DMA_TOP_QOS_VALUE: u32 = 0x2223;

// ── Output control (relative to `outctrl_base`) ────────────────────────────────────────────

/// (v << 16) | h — the input size.
pub const OUTCTRL_IN_SIZE: u32 = 0x00;
/// Frame timing enable: 0x20 (measured `frame_timing_en=1` at 0x18018).
pub const OUTCTRL_FRAME_TIMING: u32 = 0x18;
pub const OUTCTRL_FRAME_TIMING_ON: u32 = 0x20;
/// The splash writes 0x100 here before the timing (a mode word; the live dump agrees).
pub const OUTCTRL_MODE: u32 = 0x7c;
pub const OUTCTRL_MODE_VALUE: u32 = 0x100;
/// (hbp << 16) | hfp (measured 0x00940058 at 1080p60).
pub const OUTCTRL_H_PORCH: u32 = 0x80;
/// (vbp << 16) | vfp (measured 0x00260002).
pub const OUTCTRL_V_PORCH: u32 = 0x84;
/// (vsp << 28) | (vsw << 16) | (hsp << 12) | hsw (measured 0x1005102c).
pub const OUTCTRL_SYNC: u32 = 0x88;
/// (v << 16) | h — the active size (measured 0x04380780).
pub const OUTCTRL_ACTIVE: u32 = 0x8c;
/// Output pixel format: 2 = RGB888 (measured `user=2`; the splash's `pix_fmt_out`).
pub const OUTCTRL_FORMAT: u32 = 0x90;
pub const OUTCTRL_FORMAT_RGB888: u32 = 2;

// ── RDMA channel (relative to `rdma_base`) ─────────────────────────────────────────────────

/// Channel control: the splash writes 0x202040 (burst/outstanding/composer fields).
pub const RDMA_CTRL: u32 = 0x00;
pub const RDMA_CTRL_VALUE: u32 = 0x0020_2040;
/// Plane base address, low and high words (a BUS address).
pub const RDMA_ADDR_LO: u32 = 0x20;
pub const RDMA_ADDR_HI: u32 = 0x24;
/// Row stride in bytes (measured 0x1e00 = 7680).
pub const RDMA_STRIDE_BYTES: u32 = 0x38;
/// (v << 16) | h — the image size (measured 0x04380780).
pub const RDMA_SIZE: u32 = 0x3c;
/// Bounding box start (0) and end ((v−1) << 16 | (h−1), measured 0x0437077f).
pub const RDMA_BBOX_START: u32 = 0x40;
pub const RDMA_BBOX_END: u32 = 0x44;
/// Pixel format word (measured 8 with the stock kernel's XR24 plane; the splash writes 4 with
/// its own enum — the first board cycle of D4 pins which the hardware means; both are recorded).
pub const RDMA_FORMAT: u32 = 0x70;
pub const RDMA_FORMAT_XRGB8888: u32 = 8;

// ── Composer (relative to `cmps_base`) ─────────────────────────────────────────────────────

/// (h << 8) | enable (measured 0x00078001).
pub const CMPS_ENABLE_WIDTH: u32 = 0x00;
/// Output height (measured 0x438).
pub const CMPS_HEIGHT: u32 = 0x04;
/// The background colour: R, G and B are 12-bit fields at 0x08, 0x0c and 0x10, the alpha an
/// 8-bit field at 0x14 (the vendor header's names; the stock dump holds G = 255 at 0x0c and
/// A = 255 at 0x14). Only the alpha is written — an opaque black under the full-screen layer.
/// Corrected 2026-10-03: the boot loader's splash writes `0xff0000` to 0x10, outside the 12-bit
/// blue field; the board's read-back gate found it reading 0 (`gpud: dc readback differs (1 of
/// 28): 4c10=ff0000/0`), so the model no longer copies it.
pub const CMPS_BG_A: u32 = 0x14;
pub const CMPS_BG_ALPHA: u32 = 0xff;
/// The first layer's enable word (`m_nl0_en` + `m_nl0_layer_id`): bit 0 enables the layer, the
/// bits above name the RDMA channel it reads. Measured both ways: the boot loader's splash writes
/// 7 here (layer 0 reading RDMA3, the channel it feeds); the stock kernel's layer 7 holds 0x3 at
/// 0x4d18 (`m_nl7_layer_id=1`, its RDMA1). Corrected 2026-10-03: this was the constant 7 while
/// the plane was fed through RDMA1 — the composer waited on a channel nobody ran, and the
/// board's line counter stayed at 0 (`gpud: FAIL dc scanout (line counter stuck at 0)`).
pub const CMPS_LAYER_EN: u32 = 0x38;

/// The layer enable word for a plane read through RDMA channel `rdma`.
pub const fn cmps_layer_word(rdma: u32) -> u32 {
    1 | (rdma << 1)
}
/// Layer 0's rectangle and alpha words (the boot loader's splash layer): left/top, right/top,
/// bottom/blend, alpha. Kept for the splash's reading; the desktop runs on layer 7.
pub const CMPS_LAYER_LT: u32 = 0x48;
pub const CMPS_LAYER_RIGHT: u32 = 0x4c;
pub const CMPS_LAYER_BOTTOM: u32 = 0x50;
pub const CMPS_LAYER_ALPHA: u32 = 0x54;
/// The desktop's layer (TASK-0251 P2a step 3c): the stock kernel's desktop plane runs on
/// composer layer 7 (`0x4d18 = 0x3`, blend mode 1, `0x4d34 = 0x00ff0037`) and its pointer on
/// layer 6 — a lower index composites ABOVE a higher one (board cycle 13: the desktop on layer
/// 0 hid the pointer's layer 6). The boot loader's splash on layer 0 is switched off at the
/// bring-up.
pub const DESKTOP_LAYER: u32 = 7;
pub const DESKTOP_BLEND_MODE: u32 = 1;
pub const DESKTOP_ALPHA_WORD: u32 = 0x00ff_0037;
pub const SPLASH_LAYER: u32 = 0;

// ── Control words (absolute; the pipeline-2 words the splash writes) ───────────────────────

/// ctl2 enable word: (1 << rdma) | OUTCTL_EN (measured 0x00040002 with RDMA1; the splash
/// writes 0x40008 with RDMA3).
pub const CTL2_ENABLE: u32 = 0x560;
pub const CTL_OUTCTL_EN: u32 = 0x0004_0000;
/// ctl2 mode word: video mode + timing intervals (measured 0x00000f21; the splash 0x821).
pub const CTL2_MODE: u32 = 0x588;
pub const CTL2_MODE_VALUE: u32 = 0x821;
/// ctl2 config-ready: 1 latches the shadowed configuration for the next frame.
pub const CTL2_CFG_READY: u32 = 0x56c;
/// ctl2 software start (write-1-clear): begins scanning.
pub const CTL2_SW_START: u32 = 0x58c;

// ── Liveness (read by the driver, never written) ───────────────────────────────────────────

/// The controller's version word at the window's start (measured `0x03001030`: product 0x300,
/// major 0x10, minor 0x30) — the register map above is that version's; another is refused.
pub const TOP_VERSION: u32 = 0x0;
pub const KNOWN_VERSION: u32 = 0x0300_1030;
/// The output control's post-processing line counter (`postproc_ln_cnt`, relative to the
/// output control's base): it moves while the timing generator scans (measured 857 with the
/// picture on, 920 in the next capture).
pub const OUTCTRL_LINE_COUNT: u32 = 0xc8;
// ── The pointer layer (TASK-0251 P2a step 3c) ──────────────────────────────────────────────
// Measured on the stock system with its pointer on the monitor (2026-10-05,
// `docs/board/measurements/2026-10-05-cursor-layer/`): DRM plane-1 (ARGB8888 64×64, pitch 256)
// is RDMA channel 2 feeding composer layer 6; a move is the layer's rectangle.

/// The channel the pointer's buffer is read through and the composer layer that shows it.
pub const CURSOR_RDMA: u32 = 2;
pub const CURSOR_LAYER: u32 = 6;
/// The pointer buffer: 64×64 ARGB8888, 256 bytes per row (the stock framebuffer 137).
pub const CURSOR_SIZE: u32 = 64;
pub const CURSOR_STRIDE_BYTES: u32 = CURSOR_SIZE * 4;
pub const CURSOR_BYTES: usize = (CURSOR_STRIDE_BYTES * CURSOR_SIZE) as usize;
/// The stock kernel's channel control word (its desktop channel 1 and the pointer's channel 2
/// both read `0x001e203c`: outstanding 15, burst 15, `layer_cmpsr_id` 2 — the splash's
/// `0x202040` drives channel 1 for us, the pointer's channel takes the kernel's word).
pub const RDMA_CTRL_CURSOR: u32 = 0x001e_203c;
/// Pixel format 4: ARGB8888 (the desktop's XRGB8888 channel reads 8; the pointer's reads 4 —
/// measured at `0xcf0`).
pub const RDMA_FORMAT_ARGB8888: u32 = 4;
/// The channel's memory word after the format (`fbc_mem_size` in the dump's names): the
/// desktop's channel holds 0x570, the pointer's `0x10000008` — the 64×64 buffer's size with
/// bit 28 (measured; its fields are not named).
pub const RDMA_CURSOR_WORD_78: u32 = 0x78;
pub const RDMA_CURSOR_WORD_78_VALUE: u32 = 0x1000_0008;
/// The channel's composer y offset (`compsr_y_offset`, the vendor header's name): the row of
/// the composer the channel's first line lands on — measured `0x3cb` (971) on the pointer's
/// channel with the layer at top 971 (board cycle 12 showed nothing with it left at 0).
pub const RDMA_COMPOSER_Y: u32 = 0x04;
/// The composer's layer blocks are 0x20 apart from layer 0's enable word (layer 7's enable at
/// 0x118, measured `0x3`; layer 6's at 0xf8, measured `0x5` = channel 2).
pub const CMPS_LAYER_STRIDE: u32 = 0x20;
/// Layer `n`'s enable word, relative to the composer's base.
pub const fn cmps_layer_en(n: u32) -> u32 {
    CMPS_LAYER_EN + n * CMPS_LAYER_STRIDE
}
/// The rectangle words relative to a layer's enable word: left (`left << 8`, measured
/// `0x0006bd00` at 1725), right/top (`right << 16 | top`, measured `0x06fc03cb`), bottom with
/// the blend mode (`blend << 16 | bottom`, measured `0x0005040a`), the alpha word
/// (`layer_alpha << 16 | alpha_factor`, measured `0x00ff000a`; the desktop's layer 7 runs
/// `0x00ff0037` with blend mode 1).
pub const LAYER_LEFT: u32 = 0x10;
pub const LAYER_RIGHT_TOP: u32 = 0x14;
pub const LAYER_BOTTOM_BLEND: u32 = 0x18;
pub const LAYER_ALPHA: u32 = 0x1c;
pub const CURSOR_BLEND_MODE: u32 = 5;
pub const CURSOR_ALPHA_WORD: u32 = 0x00ff_000a;

/// Pipeline 2's raw interrupt word (`onl2_nml_*_int_raw`): bit 0 vsync, 1 eof, 2 cfg-eof,
/// 3 cfg-line (measured `0xd` with the picture on: vsync, cfg-eof and cfg-line raised). A raw
/// vsync is the second witness that the timing generator scans.
pub const INT_RAW_ONL2: u32 = 0x960;
pub const INT_RAW_VSYNC: u32 = 1 << 0;
