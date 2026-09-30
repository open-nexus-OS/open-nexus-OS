// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The gaussian backdrop blur on virgl (the two frame-ring planes aliased as a GL
//! texture, separable two-pass ping-pong, the result left in the texture or copied back into
//! the scanned-out VMO) — split out of `virgl3d.rs` by responsibility (module-size ratchet,
//! M-L 2026-09-30). The plane rows and the stride it addresses are `nexus_display_proto::layout`'s.
//! Compiled only for the virgl OS build; the 2D path never pulls in this code.
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the visible QEMU lanes (`gpud: virgl blur gpu on` + the parity markers)

#![cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]

use super::raster::blur_backdrop_separable_vmo;
use super::VirtioGpuBackend;
use crate::markers::{
    GPUD_VIRGL_BLUR_GPU_ON, GPUD_VIRGL_BLUR_PARITY_OFF, GPUD_VIRGL_BLUR_PARITY_OK,
};
use crate::protocol;
use nexus_display_proto::layout;
use nexus_gfx::backend::error::GfxError;

impl VirtioGpuBackend {
    /// Lazily create the GPU blur pipeline. The source/destination texture
    /// ALIASES the framebuffer VMO's display planes (rows 1600..3199), so the
    /// blur is zero-copy: TRANSFER_TO_HOST syncs the region into the GL
    /// texture, two shader passes blur it (H into a scratch RT, V back), and
    /// TRANSFER_FROM_HOST lands the result directly in the scanned-out VMO.
    /// Reuses the boot self-test's blend/DSA/rasterizer/vertex-elements and
    /// vertex shader (context-persistent objects).
    pub(crate) fn virgl_blur_init(&mut self) -> Result<(), GfxError> {
        use crate::protocol::{
            VirtioGpuCtxAttachResource, VirtioGpuResourceCreate3d, VirtioGpuSubmit3d,
            VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE, VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING,
            VIRTIO_GPU_CMD_RESOURCE_CREATE_3D, VIRTIO_GPU_CMD_SUBMIT_3D,
        };
        use crate::virgl::{
            Submit3d, PIPE_BIND_RENDER_TARGET, PIPE_BIND_SAMPLER_VIEW, PIPE_BIND_VERTEX_BUFFER,
            PIPE_BUFFER, PIPE_FORMAT_B8G8R8A8_UNORM, PIPE_FORMAT_R8_UNORM, PIPE_SHADER_FRAGMENT,
            PIPE_TEXTURE_2D,
        };
        // The scanout record names windowd's framebuffer VMO; the alias is its
        // runs over the two frame-ring planes, RFC-0098 C4.
        let scanout = self.scanout_resource.ok_or(GfxError::DeviceNotFound)?;
        let record = self.find_resource(scanout).ok_or(GfxError::DeviceNotFound)?;
        let (alias_off, alias_len) =
            (layout::row_offset_bytes(layout::DISPLAY_ROW), 2 * layout::PLANE_BYTES);

        // FBSRC: a texture two display heights tall aliasing the two frame-ring planes.
        let create_src = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: 0xF8,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_RENDER_TARGET | PIPE_BIND_SAMPLER_VIEW,
            width: layout::LAYOUT_MAX.0,
            height: 2 * layout::PLANE_ROWS,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create_src)?;
        let hdr = self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING);
        self.attach_backing_runs(hdr, 0xF8, record.dma_vmo, alias_off, alias_len)?;
        let ctx_attach = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: 0xF8,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach)?;

        // TMP: a display-sized scratch render target (host-side only, no backing).
        let create_tmp = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: 0xF9,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_RENDER_TARGET | PIPE_BIND_SAMPLER_VIEW,
            width: layout::LAYOUT_MAX.0,
            height: layout::PLANE_ROWS,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create_tmp)?;
        let ctx_attach_tmp = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: 0xF9,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach_tmp)?;

        // QUAD: exact −1..1 quad (two triangles) so rasterization covers the
        // viewport box exactly — no scissor needed.
        let create_quad = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: 0xFA,
            target: PIPE_BUFFER,
            format: PIPE_FORMAT_R8_UNORM,
            bind: PIPE_BIND_VERTEX_BUFFER,
            width: 96,
            height: 1,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create_quad)?;
        let ctx_attach_quad = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: 0xFA,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach_quad)?;

        let quad: [f32; 24] = [
            -1.0, -1.0, 0.0, 1.0, 1.0, -1.0, 0.0, 1.0, -1.0, 1.0, 0.0, 1.0, // tri 1
            1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0, -1.0, 1.0, 0.0, 1.0, // tri 2
        ];
        let mut qbytes = [0u8; 96];
        for (i, v) in quad.iter().enumerate() {
            qbytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }

        let mut s = Submit3d::new();
        s.emit_resource_inline_write(0xFA, &qbytes);
        s.emit_create_surface(0x30, 0xF8, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_surface(0x31, 0xF9, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_view(0x32, 0xF8, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_view(0x33, 0xF9, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_state_default(0x34);
        s.emit_create_shader(13, PIPE_SHADER_FRAGMENT, crate::virgl_blur_shaders::FS_BLUR);
        s.emit_create_shader(
            crate::gl_scanout::H_FS_BLUR_ROUND,
            PIPE_SHADER_FRAGMENT,
            crate::virgl_blur_shaders::FS_BLUR_ROUND,
        );
        // Alpha-"over" blend for the masked blur pass. Its own handle (not the
        // compositor's `H_BLEND_ALPHA`): the blur runs BEFORE the first layer
        // composite of a frame, so it cannot depend on `composite_init`.
        s.emit_create_blend_alpha(crate::gl_scanout::H_BLEND_BLUR);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)?;
        self.virgl_blur_ready = true;
        Ok(())
    }

    /// Two-pass separable gaussian blur on the GPU via virgl.
    ///
    /// `y` is the absolute framebuffer row (display offset already applied by
    /// the caller); the fb-alias texture covers rows 1600..3199. The CPU
    /// fallback in `blur_backdrop_separable_vmo` remains the parity reference —
    /// on the first GPU blur the result is compared against it (interior of
    /// the region, tolerance 2 LSB) and a parity marker is emitted.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn submit_virgl_blur(
        &mut self,
        fb: *mut u8,
        fb_len: usize,
        fb_w: usize,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        radius: u32,
        // When false, the blurred result is left in the GL texture (0xF8) only
        // and NOT copied back to the VMO — the caller composites over 0xF8
        // directly (glass layer), saving a transfer per frame. The one-shot
        // boot parity check forces a writeback regardless.
        writeback: bool,
    ) -> Result<(), GfxError> {
        use crate::protocol::{VirtioGpuSubmit3d, VIRTIO_GPU_CMD_SUBMIT_3D};
        use crate::virgl::{
            Submit3d, PIPE_PRIM_TRIANGLES, PIPE_SHADER_FRAGMENT, PIPE_SHADER_VERTEX,
        };
        if !self.virgl_capable || !self.virgl_draw_ok || self.virgl_ctx_id == 0 {
            return Err(GfxError::DeviceNotFound);
        }
        if radius == 0 || w == 0 || h == 0 || fb_w != layout::LAYOUT_MAX.0 as usize {
            return Err(GfxError::InvalidArgument);
        }
        // The alias texture covers display rows 1600..3199.
        if y < layout::DISPLAY_ROW
            || x.saturating_add(w) > layout::LAYOUT_MAX.0
            || (y - layout::DISPLAY_ROW).saturating_add(h) > 2 * layout::PLANE_ROWS
        {
            return Err(GfxError::InvalidArgument);
        }
        let y_rel = y - layout::DISPLAY_ROW;
        if !self.virgl_blur_ready {
            self.virgl_blur_init()?;
        }
        // First GPU-executed blur (init may have happened earlier via the GL
        // scanout bringup — the marker tracks first USE, not init).
        if !self.virgl_blur_first_done {
            self.virgl_blur_first_done = true;
            let _ = nexus_abi::debug_println(crate::markers::GPUD_VIRGL_BLUR_GPU_ON);
        }

        // One-shot parity: snapshot the region and CPU-blur it for comparison.
        let parity_buf: Option<(usize, usize)> =
            if !self.virgl_parity_done && (w as usize) * (h as usize) * 4 <= 1024 * 1024 {
                self.virgl_parity_done = true;
                match self.virgl_alloc_scratch((w as usize) * (h as usize) * 4) {
                    Ok(va) => {
                        // Tightly pack the region into the scratch and CPU-blur it.
                        for row in 0..h as usize {
                            let src_off = (y as usize + row) * fb_w * 4 + (x as usize) * 4;
                            if src_off + (w as usize) * 4 > fb_len {
                                break;
                            }
                            unsafe {
                                core::ptr::copy_nonoverlapping(
                                    fb.add(src_off),
                                    (va + row * (w as usize) * 4) as *mut u8,
                                    (w as usize) * 4,
                                );
                            }
                        }
                        let _ = blur_backdrop_separable_vmo(
                            va as *mut u8,
                            (w as usize) * (h as usize) * 4,
                            w as usize,
                            0,
                            0,
                            w,
                            h,
                            radius,
                            0,
                        );
                        Some((va, (w as usize) * 4))
                    }
                    Err(_) => None,
                }
            } else {
                None
            };

        // Sync the region from the guest VMO into the GL texture.
        self.virgl_transfer_to_host(0xF8, x, y_rel, w, h, layout::STRIDE_BYTES)?;

        let sigma = (radius as f32) / 2.0;
        let k = -1.0 / (2.0 * sigma * sigma * core::f32::consts::LN_2);
        let r = radius as f32;

        let mut s = Submit3d::new();
        // Rebind pipeline state explicitly — other passes (gl_scanout blit,
        // selftests) bind their own objects and virgl state is context-global.
        s.emit_bind_object(crate::virgl::VIRGL_OBJECT_BLEND, 0x20);
        s.emit_bind_object(crate::virgl::VIRGL_OBJECT_DSA, 0x21);
        s.emit_bind_object(crate::virgl::VIRGL_OBJECT_RASTERIZER, 0x22);
        s.emit_bind_object(crate::virgl::VIRGL_OBJECT_VERTEX_ELEMENTS, 0x23);
        // Pass 1: horizontal blur, FBSRC region → TMP at (0,0,w,h).
        s.emit_set_framebuffer_state(0, &[0x31]);
        s.emit_set_viewport_box(0.0, 0.0, w as f32, h as f32);
        s.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[0x32]);
        s.emit_bind_sampler_states(PIPE_SHADER_FRAGMENT, 0, &[0x34]);
        s.emit_set_constant_buffer(
            PIPE_SHADER_FRAGMENT,
            &[
                1.0 / layout::LAYOUT_MAX.0 as f32,
                1.0 / (2 * layout::PLANE_ROWS) as f32,
                r,
                k,
                1.0,
                0.0,
                x as f32,
                y_rel as f32,
            ],
        );
        s.emit_bind_shader(10, PIPE_SHADER_VERTEX);
        s.emit_bind_shader(13, PIPE_SHADER_FRAGMENT);
        s.emit_set_vertex_buffers(&[(16, 0, 0xFA)]);
        s.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
        // Pass 2: vertical blur, TMP (0,0,w,h) → FBSRC region.
        s.emit_set_framebuffer_state(0, &[0x30]);
        s.emit_set_viewport_box(x as f32, y_rel as f32, w as f32, h as f32);
        s.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[0x33]);
        s.emit_set_constant_buffer(
            PIPE_SHADER_FRAGMENT,
            &[
                1.0 / layout::LAYOUT_MAX.0 as f32,
                1.0 / layout::PLANE_ROWS as f32,
                r,
                k,
                0.0,
                1.0,
                -(x as f32),
                -(y_rel as f32),
            ],
        );
        s.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)?;

        // Land the blurred pixels back in the scanned-out guest VMO — unless the
        // caller will composite over 0xF8 directly (glass) and only the boot
        // parity check (which reads the VMO) needs it.
        if writeback || parity_buf.is_some() {
            self.virgl_transfer_from_host(0xF8, x, y_rel, w, h, layout::STRIDE_BYTES)?;
        }

        // Compare GPU result vs CPU reference over the interior.
        if let Some((ref_va, ref_stride)) = parity_buf {
            let inset = (radius + 1) as usize;
            let mut max_diff: u8 = 0;
            if (w as usize) > 2 * inset && (h as usize) > 2 * inset {
                for row in inset..(h as usize - inset) {
                    for col in inset..(w as usize - inset) {
                        let gpu_off = (y as usize + row) * fb_w * 4 + (x as usize + col) * 4;
                        let ref_off = row * ref_stride + col * 4;
                        for c in 0..3 {
                            let g = unsafe { fb.add(gpu_off + c).read_volatile() };
                            let r8 = unsafe { ((ref_va + ref_off + c) as *const u8).read() };
                            let d = g.abs_diff(r8);
                            if d > max_diff {
                                max_diff = d;
                            }
                        }
                    }
                }
                let _ = nexus_abi::debug_println(if max_diff <= 2 {
                    crate::markers::GPUD_VIRGL_BLUR_PARITY_OK
                } else {
                    crate::markers::GPUD_VIRGL_BLUR_PARITY_OFF
                });
            }
        }
        Ok(())
    }
}
