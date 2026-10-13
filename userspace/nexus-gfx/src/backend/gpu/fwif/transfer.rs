// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The transfer job (TASK-0169B P1, RFC-0101 §8c): the firmware form of one transfer-queue
//! command — the open kernel driver's 192-byte `cmd_transfer` (a frame number, 17 32-bit and
//! 13 64-bit registers, flags) — and the decoder from the **stream** the open userspace driver
//! emits for it: an 8-byte header (length, zero padding), the 64-bit words first, the 32-bit
//! words after, two of them present only with a feature of the core (`isp_xtp_pipe_enable`
//! with `S7_TOP_INFRASTRUCTURE`, `frag_screen` with `GPU_MULTICORE_SUPPORT`). The decoder
//! refuses what the kernel refuses: a length that is not the feature-gated one, a non-zero
//! padding word, an extension stream. Part of [`super`] (`fwif`); the offsets are asserted
//! against the open driver's layout.

use core::mem::{offset_of, size_of};

use super::{put32, put64};
use crate::backend::gpu::fw_features::{DeviceInfoBlock, Feature};

/// The flags of the firmware command.
pub mod flags {
    /// Run on one core (the open driver sets it on every multicore-capable core).
    pub const SINGLE_CORE: u32 = 1 << 1;
}

/// Which feature-gated words a core's stream carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamFeatures {
    /// `S7_TOP_INFRASTRUCTURE`: the stream carries `isp_xtp_pipe_enable`.
    pub s7_top_infrastructure: bool,
    /// `GPU_MULTICORE_SUPPORT`: the stream carries `frag_screen`.
    pub gpu_multicore_support: bool,
}

impl StreamFeatures {
    /// From the firmware's device-info block — the device decides, never a constant.
    pub fn of(info: &DeviceInfoBlock<'_>) -> Self {
        Self {
            s7_top_infrastructure: info.has(Feature::S7TopInfrastructure),
            gpu_multicore_support: info.has(Feature::GpuMulticoreSupport),
        }
    }

    /// The board's core (BVNC 36.29.52.182): multicore support, no S7 top infrastructure.
    pub const BOARD: Self = Self { s7_top_infrastructure: false, gpu_multicore_support: true };

    /// Bytes of a stream for a core with these features: header, 13 64-bit words, 15 32-bit
    /// words, the gated ones.
    pub const fn stream_len(self) -> usize {
        8 + 13 * 8
            + 15 * 4
            + if self.s7_top_infrastructure { 4 } else { 0 }
            + if self.gpu_multicore_support { 4 } else { 0 }
    }
}

/// The transfer registers, the open kernel driver's order: the 32-bit words first (one pad to
/// the 64-bit boundary), the 64-bit words after.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransferRegs {
    pub isp_bgobjvals: u32,
    pub usc_pixel_output_ctrl: u32,
    pub usc_clear_register0: u32,
    pub usc_clear_register1: u32,
    pub usc_clear_register2: u32,
    pub usc_clear_register3: u32,
    pub isp_mtile_size: u32,
    pub isp_render_origin: u32,
    pub isp_ctl: u32,
    /// Only with `S7_TOP_INFRASTRUCTURE`.
    pub isp_xtp_pipe_enable: u32,
    pub isp_aa: u32,
    pub event_pixel_pds_info: u32,
    pub event_pixel_pds_code: u32,
    pub event_pixel_pds_data: u32,
    pub isp_render: u32,
    pub isp_rgn: u32,
    /// Only with `GPU_MULTICORE_SUPPORT`.
    pub frag_screen: u32,
    pub padding: u32,
    pub pds_bgnd0_base: u64,
    pub pds_bgnd1_base: u64,
    pub pds_bgnd3_sizeinfo: u64,
    pub isp_mtile_base: u64,
    /// Three render targets times three pixel-back-end words.
    pub pbe_wordx_mrty: [u64; 9],
}

impl TransferRegs {
    pub const SIZE: usize = 176;
}

/// The firmware command of one transfer job.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransferCmd {
    pub frame_num: u32,
    pub padding0: u32,
    pub regs: TransferRegs,
    pub flags: u32,
    pub padding1: u32,
}

/// Why a stream was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamError {
    /// Fewer bytes than the header.
    Short,
    /// The header's length is not the feature-gated one, or exceeds the bytes.
    Length { expected: usize, got: usize },
    /// The header's padding word is not zero.
    Padding(u32),
    /// Bytes after the main stream: an extension stream this device does not take.
    Extension(usize),
}

fn get32(b: &[u8], off: usize) -> u32 {
    let mut w = [0u8; 4];
    w.copy_from_slice(&b[off..off + 4]);
    u32::from_le_bytes(w)
}

fn get64(b: &[u8], off: usize) -> u64 {
    let mut w = [0u8; 8];
    w.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(w)
}

impl TransferCmd {
    pub const SIZE: usize = 192;

    /// Decodes the open driver's stream for a core with `feats`; `single_core` is the job's
    /// one flag. The stream must be exactly the feature-gated length with a zero padding word.
    pub fn from_stream(
        bytes: &[u8],
        feats: StreamFeatures,
        single_core: bool,
    ) -> Result<Self, StreamError> {
        if bytes.len() < 8 {
            return Err(StreamError::Short);
        }
        let main_len = get32(bytes, 0) as usize;
        let padding = get32(bytes, 4);
        if padding != 0 {
            return Err(StreamError::Padding(padding));
        }
        let expected = feats.stream_len();
        if main_len != expected || main_len > bytes.len() {
            return Err(StreamError::Length { expected, got: main_len });
        }
        if bytes.len() > main_len {
            return Err(StreamError::Extension(bytes.len() - main_len));
        }
        let mut off = 8;
        let mut next64 = || {
            let v = get64(bytes, off);
            off += 8;
            v
        };
        let mut regs = TransferRegs {
            pds_bgnd0_base: next64(),
            pds_bgnd1_base: next64(),
            pds_bgnd3_sizeinfo: next64(),
            isp_mtile_base: next64(),
            ..TransferRegs::default()
        };
        for w in regs.pbe_wordx_mrty.iter_mut() {
            *w = next64();
        }
        let mut next32 = || {
            let v = get32(bytes, off);
            off += 4;
            v
        };
        regs.isp_bgobjvals = next32();
        regs.usc_pixel_output_ctrl = next32();
        regs.usc_clear_register0 = next32();
        regs.usc_clear_register1 = next32();
        regs.usc_clear_register2 = next32();
        regs.usc_clear_register3 = next32();
        regs.isp_mtile_size = next32();
        regs.isp_render_origin = next32();
        regs.isp_ctl = next32();
        regs.isp_aa = next32();
        regs.event_pixel_pds_info = next32();
        regs.event_pixel_pds_code = next32();
        regs.event_pixel_pds_data = next32();
        regs.isp_render = next32();
        regs.isp_rgn = next32();
        if feats.s7_top_infrastructure {
            regs.isp_xtp_pipe_enable = next32();
        }
        if feats.gpu_multicore_support {
            regs.frag_screen = next32();
        }
        debug_assert_eq!(off, main_len);
        Ok(Self {
            frame_num: 0,
            padding0: 0,
            regs,
            flags: if single_core { flags::SINGLE_CORE } else { 0 },
            padding1: 0,
        })
    }

    /// The 192 bytes the firmware reads after the command header.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        put32(&mut b, offset_of!(TransferCmd, frame_num), self.frame_num);
        let r = offset_of!(TransferCmd, regs);
        let regs = &self.regs;
        let words32 = [
            (offset_of!(TransferRegs, isp_bgobjvals), regs.isp_bgobjvals),
            (offset_of!(TransferRegs, usc_pixel_output_ctrl), regs.usc_pixel_output_ctrl),
            (offset_of!(TransferRegs, usc_clear_register0), regs.usc_clear_register0),
            (offset_of!(TransferRegs, usc_clear_register1), regs.usc_clear_register1),
            (offset_of!(TransferRegs, usc_clear_register2), regs.usc_clear_register2),
            (offset_of!(TransferRegs, usc_clear_register3), regs.usc_clear_register3),
            (offset_of!(TransferRegs, isp_mtile_size), regs.isp_mtile_size),
            (offset_of!(TransferRegs, isp_render_origin), regs.isp_render_origin),
            (offset_of!(TransferRegs, isp_ctl), regs.isp_ctl),
            (offset_of!(TransferRegs, isp_xtp_pipe_enable), regs.isp_xtp_pipe_enable),
            (offset_of!(TransferRegs, isp_aa), regs.isp_aa),
            (offset_of!(TransferRegs, event_pixel_pds_info), regs.event_pixel_pds_info),
            (offset_of!(TransferRegs, event_pixel_pds_code), regs.event_pixel_pds_code),
            (offset_of!(TransferRegs, event_pixel_pds_data), regs.event_pixel_pds_data),
            (offset_of!(TransferRegs, isp_render), regs.isp_render),
            (offset_of!(TransferRegs, isp_rgn), regs.isp_rgn),
            (offset_of!(TransferRegs, frag_screen), regs.frag_screen),
        ];
        for (off, v) in words32 {
            put32(&mut b, r + off, v);
        }
        let words64 = [
            (offset_of!(TransferRegs, pds_bgnd0_base), regs.pds_bgnd0_base),
            (offset_of!(TransferRegs, pds_bgnd1_base), regs.pds_bgnd1_base),
            (offset_of!(TransferRegs, pds_bgnd3_sizeinfo), regs.pds_bgnd3_sizeinfo),
            (offset_of!(TransferRegs, isp_mtile_base), regs.isp_mtile_base),
        ];
        for (off, v) in words64 {
            put64(&mut b, r + off, v);
        }
        let pbe = r + offset_of!(TransferRegs, pbe_wordx_mrty);
        for (i, w) in regs.pbe_wordx_mrty.iter().enumerate() {
            put64(&mut b, pbe + i * 8, *w);
        }
        put32(&mut b, offset_of!(TransferCmd, flags), self.flags);
        b
    }
}

// The open kernel driver's layout, pinned.
const _: () = assert!(size_of::<TransferRegs>() == TransferRegs::SIZE);
const _: () = assert!(size_of::<TransferCmd>() == TransferCmd::SIZE);
const _: () = assert!(offset_of!(TransferCmd, regs) == 8);
const _: () = assert!(offset_of!(TransferRegs, frag_screen) == 64);
const _: () = assert!(offset_of!(TransferRegs, pds_bgnd0_base) == 72);
const _: () = assert!(offset_of!(TransferRegs, isp_mtile_base) == 96);
const _: () = assert!(offset_of!(TransferRegs, pbe_wordx_mrty) == 104);
const _: () = assert!(offset_of!(TransferCmd, flags) == 184);

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream in the open driver's order for the board's core.
    fn stream(regs: &TransferRegs) -> alloc::vec::Vec<u8> {
        let mut s = alloc::vec::Vec::new();
        let len = StreamFeatures::BOARD.stream_len() as u32;
        s.extend_from_slice(&len.to_le_bytes());
        s.extend_from_slice(&0u32.to_le_bytes());
        for w in
            [regs.pds_bgnd0_base, regs.pds_bgnd1_base, regs.pds_bgnd3_sizeinfo, regs.isp_mtile_base]
        {
            s.extend_from_slice(&w.to_le_bytes());
        }
        for w in regs.pbe_wordx_mrty {
            s.extend_from_slice(&w.to_le_bytes());
        }
        for w in [
            regs.isp_bgobjvals,
            regs.usc_pixel_output_ctrl,
            regs.usc_clear_register0,
            regs.usc_clear_register1,
            regs.usc_clear_register2,
            regs.usc_clear_register3,
            regs.isp_mtile_size,
            regs.isp_render_origin,
            regs.isp_ctl,
            regs.isp_aa,
            regs.event_pixel_pds_info,
            regs.event_pixel_pds_code,
            regs.event_pixel_pds_data,
            regs.isp_render,
            regs.isp_rgn,
            regs.frag_screen,
        ] {
            s.extend_from_slice(&w.to_le_bytes());
        }
        s
    }

    /// The captured clear (job 0 of 2026-10-13): the colour in the clear register, no
    /// background program.
    fn clear_regs() -> TransferRegs {
        TransferRegs {
            pbe_wordx_mrty: [0x0200_0efc_60a0_0000, 0x0000_077f_0000_0437, 0, 0, 0, 0, 0, 0, 0],
            usc_pixel_output_ctrl: 0x7f8,
            usc_clear_register0: 0xff29_1f1a,
            isp_mtile_size: 0x0078_0044,
            isp_ctl: 0x21000,
            event_pixel_pds_info: 0x202,
            event_pixel_pds_code: 0x2620,
            event_pixel_pds_data: 0x2600,
            isp_render: 0x3,
            frag_screen: 0x0437_077f,
            ..TransferRegs::default()
        }
    }

    #[test]
    fn stream_len_is_feature_gated() {
        assert_eq!(StreamFeatures::BOARD.stream_len(), 176);
        assert_eq!(StreamFeatures::default().stream_len(), 172);
        let both = StreamFeatures { s7_top_infrastructure: true, gpu_multicore_support: true };
        assert_eq!(both.stream_len(), 180);
    }

    #[test]
    fn decodes_the_captured_clear_in_the_open_drivers_order() {
        let regs = clear_regs();
        let cmd = TransferCmd::from_stream(&stream(&regs), StreamFeatures::BOARD, true).unwrap();
        assert_eq!(cmd.regs, regs);
        assert_eq!(cmd.flags, flags::SINGLE_CORE);
        assert_eq!(cmd.frame_num, 0);
        let b = cmd.to_bytes();
        assert_eq!(&b[16..20], &0xff29_1f1au32.to_le_bytes(), "clear colour at its offset");
        assert_eq!(&b[72..76], &0x0437_077fu32.to_le_bytes(), "frag_screen at its offset");
        assert_eq!(&b[112..120], &0x0200_0efc_60a0_0000u64.to_le_bytes(), "pbe word 0");
        assert_eq!(&b[184..188], &flags::SINGLE_CORE.to_le_bytes());
    }

    #[test]
    fn test_reject_stream_length_for_features() {
        let s = stream(&clear_regs());
        // The same bytes read for a core without multicore support: the length is wrong.
        let r = TransferCmd::from_stream(&s, StreamFeatures::default(), false);
        assert_eq!(r, Err(StreamError::Length { expected: 172, got: 176 }));
        // A header that claims more than the bytes carry.
        let mut long = s.clone();
        long[0..4].copy_from_slice(&200u32.to_le_bytes());
        assert_eq!(
            TransferCmd::from_stream(&long, StreamFeatures::BOARD, false),
            Err(StreamError::Length { expected: 176, got: 200 })
        );
        assert_eq!(
            TransferCmd::from_stream(&s[..4], StreamFeatures::BOARD, false),
            Err(StreamError::Short)
        );
    }

    #[test]
    fn test_reject_stream_padding_nonzero() {
        let mut s = stream(&clear_regs());
        s[4] = 1;
        assert_eq!(
            TransferCmd::from_stream(&s, StreamFeatures::BOARD, false),
            Err(StreamError::Padding(1))
        );
    }

    #[test]
    fn test_reject_extension_stream() {
        let mut s = stream(&clear_regs());
        s.extend_from_slice(&[0u8; 8]);
        assert_eq!(
            TransferCmd::from_stream(&s, StreamFeatures::BOARD, false),
            Err(StreamError::Extension(8))
        );
    }
}
