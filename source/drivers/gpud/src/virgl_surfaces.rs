// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The app surfaces on virgl (RFC-0114 §1, §6, ADR-0092): each object gpud made for an
//! app — eager, made for the virtio device — is one host texture resource over the object's runs
//! (`stride / 4` texels wide, `slots × rows` tall: slot `s` at row `s × rows`), with a sampler
//! view, made at allocation, gone when the object retires. A buffer layer's rows reach the host
//! texture when its content changed since the layer last drew (`LayerStep::upload`), and that
//! transfer is waited for — the host has copied them before the present is acked, so a slot
//! windowd hands back to the app is never read again. Then the layer is composed like any other.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the visible QEMU lanes (`gpud: surface alloc ok (`, the apps' windows on screen)
//! ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::command::buffer::RgbaColor;
use nexus_gfx::frame::FrameLayer;

use crate::backend::surface_backing::Backed;
use crate::backend::VirtioGpuBackend;
use crate::protocol::{
    VirtioGpuCtxAttachResource, VirtioGpuResourceCreate3d, VirtioGpuResourceDetachBacking,
    VirtioGpuResourceUnref, VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE,
    VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING, VIRTIO_GPU_CMD_RESOURCE_CREATE_3D,
    VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING, VIRTIO_GPU_CMD_RESOURCE_UNREF,
};
use crate::surfaces::Geometry;
use crate::virgl::{Submit3d, PIPE_BIND_SAMPLER_VIEW, PIPE_FORMAT_B8G8R8A8_UNORM, PIPE_TEXTURE_2D};
use crate::virgl_composite::Content;

/// The first resource id and object handle an app surface takes: clear of every fixed one
/// (resources 0xE0–0xFA, handles 0x20–0x62).
const FIRST_ID: u32 = 0x1000;

impl VirtioGpuBackend {
    /// Object `vmo` — source `buffer` of `geometry` — as a host texture with its sampler view.
    /// False: the device refused a step (what was made goes again).
    pub(crate) fn virgl_bind_surface(
        &mut self,
        buffer: u32,
        vmo: u32,
        geometry: &Geometry,
    ) -> bool {
        let id = FIRST_ID.saturating_add(buffer);
        let (width, height) =
            (geometry.stride / 4, u32::from(geometry.rows) * u32::from(geometry.slots));
        let create = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: id,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_SAMPLER_VIEW,
            width,
            height,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        if self.ctrl_submit_struct(&create).is_err() {
            return false;
        }
        let hdr = self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING);
        let attached = self.attach_backing_runs(hdr, id, vmo, 0, geometry.len()).is_ok()
            && self
                .ctrl_submit_struct(&VirtioGpuCtxAttachResource {
                    hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
                    resource_id: id,
                    _padding: 0,
                })
                .is_ok();
        let mut s = Submit3d::new();
        s.emit_create_sampler_view(id, id, PIPE_FORMAT_B8G8R8A8_UNORM);
        let viewed = attached && self.submit3d(&s).is_ok();
        let kept = viewed
            && self.cpu.surfaces.keep(Backed {
                buffer,
                geometry: *geometry,
                resource: id,
                view: id,
            });
        if !kept {
            self.virgl_release(id, viewed);
        }
        kept
    }

    /// Source `buffer`'s texture and view go (its object retired).
    pub(crate) fn virgl_unbind_surface(&mut self, buffer: u32) {
        if let Some(b) = self.cpu.surfaces.take(buffer) {
            self.virgl_release(b.resource, true);
        }
    }

    /// The texture `id` given back to the host: its view (when made), its backing, the resource.
    fn virgl_release(&mut self, id: u32, viewed: bool) {
        if viewed {
            let mut s = Submit3d::new();
            s.emit_destroy_object(id);
            let _ = self.submit3d(&s);
        }
        let _ = self.ctrl_submit_struct(&VirtioGpuResourceDetachBacking {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING),
            resource_id: id,
            _padding: 0,
        });
        let _ = self.ctrl_submit_struct(&VirtioGpuResourceUnref {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_UNREF),
            resource_id: id,
            _padding: 0,
        });
    }

    /// A buffer layer onto the scanout render target: its shadow, its rows transferred when they
    /// changed (waited — the host holds them before the present's ack), its content composed.
    pub(crate) fn composite_buffer_rt(
        &mut self,
        l: &FrameLayer,
        upload: bool,
    ) -> Result<(), GfxError> {
        if !self.virgl_capable || !self.virgl_draw_ok || !self.pipelines.ready {
            return Err(GfxError::DeviceNotFound);
        }
        let b = *self.cpu.surfaces.get(l.source).ok_or(GfxError::InvalidArgument)?;
        let g = b.geometry;
        if l.shadow_blur > 0 {
            self.submit_drop_shadow_rt(
                l.dst_x,
                l.dst_y,
                l.width,
                l.height,
                l.corner_radius,
                l.shadow_blur,
                0,
                l.shadow_offset_y,
                RgbaColor::new(0, 0, 0, l.shadow_alpha.min(255) as u8),
            )?;
        }
        let (src_w, src_h) =
            if l.content_w > 0 { (l.content_w, l.content_h) } else { (l.width, l.height) };
        let base = u32::from(l.slot) * u32::from(g.rows);
        if upload {
            // A scroll band's whole rows (a later slice samples another offset of them), else the
            // rows the layer shows.
            let (x, row, w, h) = if l.scroll_band_h > 0 {
                (0, l.scroll_band_top_abs, u32::from(g.width), l.scroll_band_h)
            } else {
                (l.src_x, l.src_row_abs, src_w, src_h)
            };
            self.ctrl_unbatched(|gpu| {
                gpu.virgl_transfer_to_host(b.resource, x, base + row, w, h, g.stride)
            })?;
        }
        let content = Content {
            view: b.view,
            tex: (g.stride / 4, u32::from(g.rows) * u32::from(g.slots)),
            rect: (l.src_x, base + l.src_row_abs, src_w, src_h),
        };
        let target = self.rt_back_surface();
        let frame = (l.dst_x, l.dst_y, l.width, l.height);
        self.layer_pass(
            self.pipelines.layer_atlas,
            target,
            content,
            frame,
            l.opacity,
            l.corner_radius,
        )
    }
}
