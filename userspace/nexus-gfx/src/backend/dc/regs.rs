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
/// Background colour words the splash writes (G, A).
pub const CMPS_BG_G: u32 = 0x10;
pub const CMPS_BG_A: u32 = 0x14;
pub const CMPS_BG_VALUE: u32 = 0x00ff_0000;
pub const CMPS_BG_ALPHA: u32 = 0xff;
/// Layer 7 enable word: 7 = enable + the RDMA layer id the splash uses.
pub const CMPS_LAYER_EN: u32 = 0x38;
pub const CMPS_LAYER_EN_VALUE: u32 = 7;
/// Layer rectangle: left/top (0), right ((h−1) << 16), bottom (v−1), alpha word.
pub const CMPS_LAYER_LT: u32 = 0x48;
pub const CMPS_LAYER_RIGHT: u32 = 0x4c;
pub const CMPS_LAYER_BOTTOM: u32 = 0x50;
pub const CMPS_LAYER_ALPHA: u32 = 0x54;
pub const CMPS_LAYER_ALPHA_VALUE: u32 = 0x00ff_0000;

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
