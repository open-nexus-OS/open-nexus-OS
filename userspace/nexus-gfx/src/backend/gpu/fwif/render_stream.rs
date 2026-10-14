// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The render commands back to streams (TASK-0215 5c): the inverse of the decoders in
//! [`super::render`], so a device can decode the open driver's stream, change a register (the
//! render-area clip, `ISP_CTL`, the control stream's base) and hand the stream on in the same
//! definition order. Only what a stream carries is encoded: the flags come from the submit, the
//! render-target data's address from the dataset. Part of [`super`] (`fwif`); the captured
//! streams round-trip byte for byte (`tests/gpu_scene_goldens.rs`).

use alloc::vec::Vec;

use super::render::{FragCmd, GeomCmd, RenderFeatures};

struct Writer {
    b: Vec<u8>,
}

impl Writer {
    fn new(len: usize) -> Self {
        let mut b = Vec::with_capacity(len);
        b.extend_from_slice(&(len as u32).to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        Self { b }
    }
    fn u32(&mut self, v: u32) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
}

impl GeomCmd {
    /// The stream `from_stream` would decode back to these registers.
    pub fn to_stream(&self, feats: RenderFeatures) -> Vec<u8> {
        let r = &self.regs;
        let mut w = Writer::new(feats.geom_stream_len());
        w.u64(r.vdm_ctrl_stream_base);
        w.u64(r.tpu_border_colour_table);
        if feats.vdm_drawindirect {
            w.u64(r.vdm_draw_indirect0);
            w.u32(r.vdm_draw_indirect1);
        }
        w.u32(r.ppp_ctrl);
        w.u32(r.te_psg);
        w.u32(r.vdm_context_resume_task0_size);
        if feats.vdm_object_level_lls {
            w.u32(r.vdm_context_resume_task3_size);
        }
        w.u32(r.view_idx);
        if feats.tessellation {
            w.u32(r.pds_coeff_free_prog);
        }
        debug_assert_eq!(w.b.len(), feats.geom_stream_len());
        w.b
    }
}

impl FragCmd {
    /// The stream `from_stream` would decode back to these registers.
    pub fn to_stream(&self, feats: RenderFeatures) -> Vec<u8> {
        let r = &self.regs;
        let mut w = Writer::new(feats.frag_stream_len());
        for v in [
            r.isp_scissor_base,
            r.isp_dbias_base,
            r.isp_oclqry_base,
            r.isp_zlsctl,
            r.isp_zload_store_base,
            r.isp_stencil_load_store_base,
        ] {
            w.u64(v);
        }
        if feats.requires_fb_cdc_zls_setup {
            w.u64(r.fb_cdc_zls);
        }
        for v in r.pbe_word {
            w.u64(v);
        }
        w.u64(r.tpu_border_colour_table);
        for v in r.pds_bgnd.iter().chain(&r.pds_pr_bgnd) {
            w.u64(*v);
        }
        for v in r.usc_clear_register {
            w.u32(v);
        }
        w.u32(r.usc_pixel_output_ctrl);
        w.u32(r.isp_bgobjdepth);
        w.u32(r.isp_bgobjvals);
        w.u32(r.isp_aa);
        if feats.s7_top_infrastructure {
            w.u32(r.isp_xtp_pipe_enable);
        }
        w.u32(r.isp_ctl);
        w.u32(r.event_pixel_pds_info);
        if feats.cluster_grouping {
            w.u32(r.pixel_phantom);
        }
        w.u32(r.view_idx);
        w.u32(r.event_pixel_pds_data);
        if feats.gpu_multicore_support {
            w.u32(r.isp_oclqry_stride);
        }
        if feats.zls_subtile {
            w.u32(r.isp_zls_pixels);
        }
        if feats.isp_zls_d24_s8_packing_ogl_mode {
            w.u32(r.rgx_cr_blackpearl_fix);
        }
        w.u32(self.zls_stride);
        w.u32(self.sls_stride);
        if feats.gpu_multicore_support {
            w.u32(self.execute_count);
        }
        debug_assert_eq!(w.b.len(), feats.frag_stream_len());
        w.b
    }
}
