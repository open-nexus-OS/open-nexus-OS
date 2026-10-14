// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The render jobs (TASK-0215 5a, RFC-0101 §8d): the firmware forms of a geometry command
//! (`GEOM`, 112 bytes) and a fragment command (`FRAG` / `FRAG_PR`, 480 bytes) — the open kernel
//! driver's structs with their offsets asserted — and the decoders from the streams the open
//! userspace driver emits for them, in the open kernel driver's definition order with the
//! feature-gated words a core's device-info block decides. The static render-context state
//! (the 23 context-switch registers, a 192-byte stream) is here too. A quirk extension stream
//! (BRN 47217, 49927) is refused by name: this core has neither.

use core::mem::{offset_of, size_of};

use super::transfer::StreamError;
use super::{put32, put64, Ufo};
use crate::backend::gpu::fw_features::{DeviceInfoBlock, Feature};

/// The geometry command's flags (the firmware's).
pub mod geom_flags {
    pub const FIRSTKICK: u32 = 1 << 0;
    pub const LASTKICK: u32 = 1 << 1;
    pub const SINGLE_CORE: u32 = 1 << 3;
    /// From the open driver's submit flags: FIRST (bit 0), LAST (bit 1), SINGLE_CORE (bit 2).
    pub const fn from_submit(flags: u32) -> u32 {
        (if flags & 1 != 0 { FIRSTKICK } else { 0 })
            | (if flags & 2 != 0 { LASTKICK } else { 0 })
            | (if flags & 4 != 0 { SINGLE_CORE } else { 0 })
    }
}

/// The fragment command's flags (the firmware's).
pub mod frag_flags {
    pub const SINGLE_CORE: u32 = 1 << 3;
    pub const GET_VIS_RESULTS: u32 = 1 << 5;
    pub const DEPTHBUFFER: u32 = 1 << 7;
    pub const STENCILBUFFER: u32 = 1 << 8;
    pub const DISABLE_PIXELMERGE: u32 = 1 << 15;
    pub const SCRATCHBUFFER: u32 = 1 << 19;
    pub const PREVENT_CDM_OVERLAP: u32 = 1 << 26;
    /// The open driver's submit flag that makes the command a `FRAG_PR`, not a flag.
    pub const SUBMIT_PARTIAL_RENDER: u32 = 1 << 6;
    /// From the open driver's submit flags (bits 0–7, PARTIAL_RENDER excluded).
    pub const fn from_submit(flags: u32) -> u32 {
        (if flags & (1 << 0) != 0 { SINGLE_CORE } else { 0 })
            | (if flags & (1 << 1) != 0 { DEPTHBUFFER } else { 0 })
            | (if flags & (1 << 2) != 0 { STENCILBUFFER } else { 0 })
            | (if flags & (1 << 3) != 0 { PREVENT_CDM_OVERLAP } else { 0 })
            | (if flags & (1 << 4) != 0 { SCRATCHBUFFER } else { 0 })
            | (if flags & (1 << 5) != 0 { GET_VIS_RESULTS } else { 0 })
            | (if flags & (1 << 7) != 0 { DISABLE_PIXELMERGE } else { 0 })
    }
}

/// Which feature-gated words a core's geometry and fragment streams carry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderFeatures {
    pub vdm_drawindirect: bool,
    pub vdm_object_level_lls: bool,
    pub tessellation: bool,
    /// Derived in the open kernel driver from quirks; false for this core — a mismatch is
    /// caught by the stream's length.
    pub requires_fb_cdc_zls_setup: bool,
    pub s7_top_infrastructure: bool,
    pub cluster_grouping: bool,
    pub gpu_multicore_support: bool,
    pub zls_subtile: bool,
    pub isp_zls_d24_s8_packing_ogl_mode: bool,
}

impl RenderFeatures {
    pub fn of(info: &DeviceInfoBlock<'_>) -> Self {
        Self {
            vdm_drawindirect: info.has(Feature::VdmDrawindirect),
            vdm_object_level_lls: info.has(Feature::VdmObjectLevelLls),
            tessellation: info.has(Feature::Tessellation),
            requires_fb_cdc_zls_setup: false,
            s7_top_infrastructure: info.has(Feature::S7TopInfrastructure),
            cluster_grouping: info.has(Feature::ClusterGrouping),
            gpu_multicore_support: info.has(Feature::GpuMulticoreSupport),
            zls_subtile: info.has(Feature::ZlsSubtile),
            isp_zls_d24_s8_packing_ogl_mode: info.has(Feature::IspZlsD24S8PackingOglMode),
        }
    }

    /// The board's core: multicore support, nothing else of the gated set.
    pub const BOARD: Self = Self {
        vdm_drawindirect: false,
        vdm_object_level_lls: false,
        tessellation: false,
        requires_fb_cdc_zls_setup: false,
        s7_top_infrastructure: false,
        cluster_grouping: false,
        gpu_multicore_support: true,
        zls_subtile: false,
        isp_zls_d24_s8_packing_ogl_mode: false,
    };

    pub const fn geom_stream_len(self) -> usize {
        8 + 2 * 8
            + if self.vdm_drawindirect { 12 } else { 0 }
            + 3 * 4
            + if self.vdm_object_level_lls { 4 } else { 0 }
            + 4
            + if self.tessellation { 4 } else { 0 }
    }

    pub const fn frag_stream_len(self) -> usize {
        8 + 6 * 8
            + if self.requires_fb_cdc_zls_setup { 8 } else { 0 }
            + 24 * 8
            + 8
            + 3 * 8
            + 3 * 8
            + 8 * 4
            + 4 * 4
            + if self.s7_top_infrastructure { 4 } else { 0 }
            + 2 * 4
            + if self.cluster_grouping { 4 } else { 0 }
            + 2 * 4
            + if self.gpu_multicore_support { 4 } else { 0 }
            + if self.zls_subtile { 4 } else { 0 }
            + if self.isp_zls_d24_s8_packing_ogl_mode { 4 } else { 0 }
            + 2 * 4
            + if self.gpu_multicore_support { 4 } else { 0 }
    }
}

/// What a geometry and a fragment command share: the frame and the render-target data.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GeomFragShared {
    pub frame_num: u32,
    /// The firmware address of the `HwrtData` the job renders into.
    pub hwrt_data_fw_addr: u32,
    pub pr_buffer_fw_addr: [u32; 2],
}

/// The geometry registers, the open kernel driver's order.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GeomRegs {
    pub vdm_ctrl_stream_base: u64,
    pub tpu_border_colour_table: u64,
    pub vdm_draw_indirect0: u64,
    pub vdm_draw_indirect1: u32,
    pub ppp_ctrl: u32,
    pub te_psg: u32,
    pub tpu: u32,
    pub vdm_context_resume_task0_size: u32,
    pub vdm_context_resume_task3_size: u32,
    pub pds_ctrl: u32,
    pub view_idx: u32,
    pub pds_coeff_free_prog: u32,
    pub padding: u32,
}

/// The geometry command (`GEOM`).
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GeomCmd {
    pub shared: GeomFragShared,
    pub regs: GeomRegs,
    pub flags: u32,
    /// The fragment queue's timeline word and the point before the paired fragment's — the
    /// firmware's partial-render handshake.
    pub partial_render_geom_frag_fence: Ufo,
    pub padding0: u32,
    pub dummy_rgnhdr_init_te_psgregion_addr: u64,
    pub brn61484_66333_live_rt: u32,
    pub padding1: u32,
}

/// The fragment registers, the open kernel driver's order.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FragRegs {
    pub usc_pixel_output_ctrl: u32,
    pub usc_clear_register: [u32; 8],
    pub isp_bgobjdepth: u32,
    pub isp_bgobjvals: u32,
    pub isp_aa: u32,
    pub isp_xtp_pipe_enable: u32,
    pub isp_ctl: u32,
    pub tpu: u32,
    pub event_pixel_pds_info: u32,
    pub pixel_phantom: u32,
    pub view_idx: u32,
    pub event_pixel_pds_data: u32,
    pub brn65101_event_pixel_pds_data: u32,
    pub isp_oclqry_stride: u32,
    pub isp_zls_pixels: u32,
    pub rgx_cr_blackpearl_fix: u32,
    pub isp_scissor_base: u64,
    pub isp_dbias_base: u64,
    pub isp_oclqry_base: u64,
    pub isp_zlsctl: u64,
    pub isp_zload_store_base: u64,
    pub isp_stencil_load_store_base: u64,
    pub fb_cdc_zls: u64,
    /// Eight render targets times three pixel-back-end words.
    pub pbe_word: [u64; 24],
    pub tpu_border_colour_table: u64,
    pub pds_bgnd: [u64; 3],
    pub pds_bgnd_brn65101: [u64; 3],
    pub pds_pr_bgnd: [u64; 3],
    pub isp_dummy_stencil_store_base: u64,
    pub isp_dummy_depth_store_base: u64,
    pub rgnhdr_single_rt_size: u32,
    pub rgnhdr_scratch_offset: u32,
}

/// The fragment command (`FRAG`, or `FRAG_PR` for the partial render).
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FragCmd {
    pub shared: GeomFragShared,
    pub regs: FragRegs,
    pub flags: u32,
    pub zls_stride: u32,
    pub sls_stride: u32,
    pub execute_count: u32,
}

/// The static render-context state: the geometry's context-switch registers, one core's set
/// (the firmware object holds two; the second stays zero on a one-core device).
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StaticRenderContextState {
    pub regs: [u64; 23],
}

impl StaticRenderContextState {
    /// One core's registers.
    pub const SIZE: usize = 23 * 8;
    /// The firmware object: two cores.
    pub const FW_SIZE: usize = 2 * Self::SIZE;
    pub const STREAM_LEN: usize = 8 + Self::SIZE;

    /// The 192-byte stream the open driver passes at context creation.
    pub fn from_stream(bytes: &[u8]) -> Result<Self, StreamError> {
        let mut r = Reader::new(bytes, Self::STREAM_LEN)?;
        let mut regs = [0u64; 23];
        for w in regs.iter_mut() {
            *w = r.u64();
        }
        r.finish()?;
        Ok(Self { regs })
    }

    /// The firmware object's bytes: this core's set, the second core zero.
    pub fn to_fw_bytes(&self) -> [u8; Self::FW_SIZE] {
        let mut b = [0u8; Self::FW_SIZE];
        for (i, w) in self.regs.iter().enumerate() {
            put64(&mut b, i * 8, *w);
        }
        b
    }
}

/// A stream reader with the open kernel driver's header check.
struct Reader<'a> {
    bytes: &'a [u8],
    off: usize,
    main_len: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], expected: usize) -> Result<Self, StreamError> {
        if bytes.len() < 8 {
            return Err(StreamError::Short);
        }
        let main_len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let padding = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if padding != 0 {
            return Err(StreamError::Padding(padding));
        }
        if main_len != expected || main_len > bytes.len() {
            return Err(StreamError::Length { expected, got: main_len });
        }
        if bytes.len() > main_len {
            return Err(StreamError::Extension(bytes.len() - main_len));
        }
        Ok(Self { bytes, off: 8, main_len })
    }
    fn u32(&mut self) -> u32 {
        let mut w = [0u8; 4];
        w.copy_from_slice(&self.bytes[self.off..self.off + 4]);
        self.off += 4;
        u32::from_le_bytes(w)
    }
    fn u64(&mut self) -> u64 {
        let mut w = [0u8; 8];
        w.copy_from_slice(&self.bytes[self.off..self.off + 8]);
        self.off += 8;
        u64::from_le_bytes(w)
    }
    fn finish(self) -> Result<(), StreamError> {
        debug_assert_eq!(self.off, self.main_len);
        Ok(())
    }
}

impl GeomCmd {
    pub const SIZE: usize = 112;

    /// Decodes the open driver's geometry stream; `submit_flags` are its submit flags, `hwrt`
    /// the render-target data's firmware address, `pr_fence` the partial-render handshake.
    pub fn from_stream(
        bytes: &[u8],
        feats: RenderFeatures,
        submit_flags: u32,
        hwrt: u32,
        pr_fence: Ufo,
    ) -> Result<Self, StreamError> {
        let mut r = Reader::new(bytes, feats.geom_stream_len())?;
        let mut regs = GeomRegs {
            vdm_ctrl_stream_base: r.u64(),
            tpu_border_colour_table: r.u64(),
            ..GeomRegs::default()
        };
        if feats.vdm_drawindirect {
            regs.vdm_draw_indirect0 = r.u64();
            regs.vdm_draw_indirect1 = r.u32();
        }
        regs.ppp_ctrl = r.u32();
        regs.te_psg = r.u32();
        regs.vdm_context_resume_task0_size = r.u32();
        if feats.vdm_object_level_lls {
            regs.vdm_context_resume_task3_size = r.u32();
        }
        regs.view_idx = r.u32();
        if feats.tessellation {
            regs.pds_coeff_free_prog = r.u32();
        }
        r.finish()?;
        Ok(Self {
            shared: GeomFragShared {
                frame_num: 0,
                hwrt_data_fw_addr: hwrt,
                pr_buffer_fw_addr: [0; 2],
            },
            regs,
            flags: geom_flags::from_submit(submit_flags),
            partial_render_geom_frag_fence: pr_fence,
            ..Self::default()
        })
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        put32(&mut b, 0, self.shared.frame_num);
        put32(&mut b, 4, self.shared.hwrt_data_fw_addr);
        put32(&mut b, 8, self.shared.pr_buffer_fw_addr[0]);
        put32(&mut b, 12, self.shared.pr_buffer_fw_addr[1]);
        let r = offset_of!(GeomCmd, regs);
        put64(
            &mut b,
            r + offset_of!(GeomRegs, vdm_ctrl_stream_base),
            self.regs.vdm_ctrl_stream_base,
        );
        put64(
            &mut b,
            r + offset_of!(GeomRegs, tpu_border_colour_table),
            self.regs.tpu_border_colour_table,
        );
        put64(&mut b, r + offset_of!(GeomRegs, vdm_draw_indirect0), self.regs.vdm_draw_indirect0);
        for (off, v) in [
            (offset_of!(GeomRegs, vdm_draw_indirect1), self.regs.vdm_draw_indirect1),
            (offset_of!(GeomRegs, ppp_ctrl), self.regs.ppp_ctrl),
            (offset_of!(GeomRegs, te_psg), self.regs.te_psg),
            (offset_of!(GeomRegs, tpu), self.regs.tpu),
            (
                offset_of!(GeomRegs, vdm_context_resume_task0_size),
                self.regs.vdm_context_resume_task0_size,
            ),
            (
                offset_of!(GeomRegs, vdm_context_resume_task3_size),
                self.regs.vdm_context_resume_task3_size,
            ),
            (offset_of!(GeomRegs, pds_ctrl), self.regs.pds_ctrl),
            (offset_of!(GeomRegs, view_idx), self.regs.view_idx),
            (offset_of!(GeomRegs, pds_coeff_free_prog), self.regs.pds_coeff_free_prog),
        ] {
            put32(&mut b, r + off, v);
        }
        put32(&mut b, offset_of!(GeomCmd, flags), self.flags);
        let f = offset_of!(GeomCmd, partial_render_geom_frag_fence);
        put32(&mut b, f, self.partial_render_geom_frag_fence.addr);
        put32(&mut b, f + 4, self.partial_render_geom_frag_fence.value);
        put64(
            &mut b,
            offset_of!(GeomCmd, dummy_rgnhdr_init_te_psgregion_addr),
            self.dummy_rgnhdr_init_te_psgregion_addr,
        );
        put32(&mut b, offset_of!(GeomCmd, brn61484_66333_live_rt), self.brn61484_66333_live_rt);
        b
    }
}

impl FragCmd {
    pub const SIZE: usize = 480;

    /// Decodes the open driver's fragment stream; the command type (`FRAG` or `FRAG_PR`) is the
    /// caller's from `frag_flags::SUBMIT_PARTIAL_RENDER`.
    pub fn from_stream(
        bytes: &[u8],
        feats: RenderFeatures,
        submit_flags: u32,
        hwrt: u32,
    ) -> Result<Self, StreamError> {
        let mut r = Reader::new(bytes, feats.frag_stream_len())?;
        let mut regs = FragRegs {
            isp_scissor_base: r.u64(),
            isp_dbias_base: r.u64(),
            isp_oclqry_base: r.u64(),
            isp_zlsctl: r.u64(),
            isp_zload_store_base: r.u64(),
            isp_stencil_load_store_base: r.u64(),
            ..FragRegs::default()
        };
        if feats.requires_fb_cdc_zls_setup {
            regs.fb_cdc_zls = r.u64();
        }
        for w in regs.pbe_word.iter_mut() {
            *w = r.u64();
        }
        regs.tpu_border_colour_table = r.u64();
        for w in regs.pds_bgnd.iter_mut() {
            *w = r.u64();
        }
        for w in regs.pds_pr_bgnd.iter_mut() {
            *w = r.u64();
        }
        for w in regs.usc_clear_register.iter_mut() {
            *w = r.u32();
        }
        regs.usc_pixel_output_ctrl = r.u32();
        regs.isp_bgobjdepth = r.u32();
        regs.isp_bgobjvals = r.u32();
        regs.isp_aa = r.u32();
        if feats.s7_top_infrastructure {
            regs.isp_xtp_pipe_enable = r.u32();
        }
        regs.isp_ctl = r.u32();
        regs.event_pixel_pds_info = r.u32();
        if feats.cluster_grouping {
            regs.pixel_phantom = r.u32();
        }
        regs.view_idx = r.u32();
        regs.event_pixel_pds_data = r.u32();
        if feats.gpu_multicore_support {
            regs.isp_oclqry_stride = r.u32();
        }
        if feats.zls_subtile {
            regs.isp_zls_pixels = r.u32();
        }
        if feats.isp_zls_d24_s8_packing_ogl_mode {
            regs.rgx_cr_blackpearl_fix = r.u32();
        }
        let zls_stride = r.u32();
        let sls_stride = r.u32();
        let execute_count = if feats.gpu_multicore_support { r.u32() } else { 0 };
        r.finish()?;
        Ok(Self {
            shared: GeomFragShared {
                frame_num: 0,
                hwrt_data_fw_addr: hwrt,
                pr_buffer_fw_addr: [0; 2],
            },
            regs,
            flags: frag_flags::from_submit(submit_flags),
            zls_stride,
            sls_stride,
            execute_count,
        })
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        put32(&mut b, 0, self.shared.frame_num);
        put32(&mut b, 4, self.shared.hwrt_data_fw_addr);
        put32(&mut b, 8, self.shared.pr_buffer_fw_addr[0]);
        put32(&mut b, 12, self.shared.pr_buffer_fw_addr[1]);
        let r = offset_of!(FragCmd, regs);
        let g = &self.regs;
        put32(&mut b, r + offset_of!(FragRegs, usc_pixel_output_ctrl), g.usc_pixel_output_ctrl);
        for (i, v) in g.usc_clear_register.iter().enumerate() {
            put32(&mut b, r + offset_of!(FragRegs, usc_clear_register) + i * 4, *v);
        }
        for (off, v) in [
            (offset_of!(FragRegs, isp_bgobjdepth), g.isp_bgobjdepth),
            (offset_of!(FragRegs, isp_bgobjvals), g.isp_bgobjvals),
            (offset_of!(FragRegs, isp_aa), g.isp_aa),
            (offset_of!(FragRegs, isp_xtp_pipe_enable), g.isp_xtp_pipe_enable),
            (offset_of!(FragRegs, isp_ctl), g.isp_ctl),
            (offset_of!(FragRegs, tpu), g.tpu),
            (offset_of!(FragRegs, event_pixel_pds_info), g.event_pixel_pds_info),
            (offset_of!(FragRegs, pixel_phantom), g.pixel_phantom),
            (offset_of!(FragRegs, view_idx), g.view_idx),
            (offset_of!(FragRegs, event_pixel_pds_data), g.event_pixel_pds_data),
            (offset_of!(FragRegs, brn65101_event_pixel_pds_data), g.brn65101_event_pixel_pds_data),
            (offset_of!(FragRegs, isp_oclqry_stride), g.isp_oclqry_stride),
            (offset_of!(FragRegs, isp_zls_pixels), g.isp_zls_pixels),
            (offset_of!(FragRegs, rgx_cr_blackpearl_fix), g.rgx_cr_blackpearl_fix),
            (offset_of!(FragRegs, rgnhdr_single_rt_size), g.rgnhdr_single_rt_size),
            (offset_of!(FragRegs, rgnhdr_scratch_offset), g.rgnhdr_scratch_offset),
        ] {
            put32(&mut b, r + off, v);
        }
        for (off, v) in [
            (offset_of!(FragRegs, isp_scissor_base), g.isp_scissor_base),
            (offset_of!(FragRegs, isp_dbias_base), g.isp_dbias_base),
            (offset_of!(FragRegs, isp_oclqry_base), g.isp_oclqry_base),
            (offset_of!(FragRegs, isp_zlsctl), g.isp_zlsctl),
            (offset_of!(FragRegs, isp_zload_store_base), g.isp_zload_store_base),
            (offset_of!(FragRegs, isp_stencil_load_store_base), g.isp_stencil_load_store_base),
            (offset_of!(FragRegs, fb_cdc_zls), g.fb_cdc_zls),
            (offset_of!(FragRegs, tpu_border_colour_table), g.tpu_border_colour_table),
            (offset_of!(FragRegs, isp_dummy_stencil_store_base), g.isp_dummy_stencil_store_base),
            (offset_of!(FragRegs, isp_dummy_depth_store_base), g.isp_dummy_depth_store_base),
        ] {
            put64(&mut b, r + off, v);
        }
        for (i, v) in g.pbe_word.iter().enumerate() {
            put64(&mut b, r + offset_of!(FragRegs, pbe_word) + i * 8, *v);
        }
        for (i, v) in g.pds_bgnd.iter().enumerate() {
            put64(&mut b, r + offset_of!(FragRegs, pds_bgnd) + i * 8, *v);
        }
        for (i, v) in g.pds_bgnd_brn65101.iter().enumerate() {
            put64(&mut b, r + offset_of!(FragRegs, pds_bgnd_brn65101) + i * 8, *v);
        }
        for (i, v) in g.pds_pr_bgnd.iter().enumerate() {
            put64(&mut b, r + offset_of!(FragRegs, pds_pr_bgnd) + i * 8, *v);
        }
        put32(&mut b, offset_of!(FragCmd, flags), self.flags);
        put32(&mut b, offset_of!(FragCmd, zls_stride), self.zls_stride);
        put32(&mut b, offset_of!(FragCmd, sls_stride), self.sls_stride);
        put32(&mut b, offset_of!(FragCmd, execute_count), self.execute_count);
        b
    }
}

// The open kernel driver's layouts, pinned.
const _: () = assert!(size_of::<GeomFragShared>() == 16);
const _: () = assert!(size_of::<GeomRegs>() == 64);
const _: () = assert!(size_of::<GeomCmd>() == GeomCmd::SIZE);
const _: () = assert!(offset_of!(GeomCmd, regs) == 16);
const _: () = assert!(offset_of!(GeomCmd, flags) == 80);
const _: () = assert!(offset_of!(GeomCmd, partial_render_geom_frag_fence) == 84);
const _: () = assert!(offset_of!(GeomCmd, dummy_rgnhdr_init_te_psgregion_addr) == 96);
const _: () = assert!(size_of::<FragRegs>() == 448);
const _: () = assert!(offset_of!(FragRegs, isp_scissor_base) == 96);
const _: () = assert!(offset_of!(FragRegs, pbe_word) == 152);
const _: () = assert!(offset_of!(FragRegs, rgnhdr_single_rt_size) == 440);
const _: () = assert!(size_of::<FragCmd>() == FragCmd::SIZE);
const _: () = assert!(offset_of!(FragCmd, flags) == 464);
const _: () = assert!(size_of::<StaticRenderContextState>() == StaticRenderContextState::SIZE);
