// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the GL glass-blur cache — a glass layer re-blurs ONLY when its
//! backdrop actually changed. Pure key/packing state (host-tested) + the
//! virgl read/write emission.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `mod tests` below; visual via QEMU glass markers
//!
//! WHY: the RT build-up re-renders the whole frame every present, so every
//! glass layer paid its snapshot + two gaussian passes on every cursor move
//! and keystroke. The historical windowd-side `BackdropCache` never survived
//! into this path (its fields are dropped at the `Command` encoding), which
//! went unnoticed while the scene had one or two glass layers — the app-panel
//! work (RFC-0084) made it five to ten.
//!
//! MODEL: a glass layer's blurred backdrop is a pure function of everything
//! composited BELOW it. Walking the retained layer set front-to-back, we fold
//! a running FNV hash over each drawn layer's effective inputs (post
//! transform/scroll overrides, content epoch included); a glass layer's cache
//! key = that fold + its own rect/radii. Key unchanged ⇒ the blurred pixels
//! are bit-stable ⇒ one masked draw from the cache texture replaces the
//! snapshot and both gaussian passes. Key changed ⇒ blur as before, then copy
//! the result into the cache. Rows PACK in walk order into one texture; a
//! layer that no longer fits simply blurs live (correct, just not cached).

/// Cache texture height (display-wide). Two windows of pane glass fit; what
/// does not fit falls back to live blur, so the cap is a cost bound, not a
/// correctness bound.
#[cfg(any(test, all(feature = "virgl", feature = "os-lite", target_os = "none")))]
pub(crate) const BLUR_CACHE_TEX_H: u32 = 4096;

/// Slot capacity — matches the retained layer set (`MAX_PENDING_RT_LAYERS`,
/// which is cfg-gated to the virgl build; this mirror keeps the pure state
/// host-testable, and the const assert below pins the two together).
#[cfg(any(test, all(feature = "virgl", feature = "os-lite", target_os = "none")))]
const MAX_SLOTS: usize = 32;

#[cfg(any(test, all(feature = "virgl", feature = "os-lite", target_os = "none")))]
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
#[cfg(any(test, all(feature = "virgl", feature = "os-lite", target_os = "none")))]
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// One FNV-1a fold step.
#[cfg(any(test, all(feature = "virgl", feature = "os-lite", target_os = "none")))]
pub(crate) fn fold(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(FNV_PRIME)
}

/// Folds one drawn layer's effective inputs into the running
/// destination-so-far hash. EVERY composited layer folds — opaque content
/// changes what a glass layer above it blurs just as much as another glass
/// does.
#[allow(clippy::too_many_arguments)]
#[cfg(any(test, all(feature = "virgl", feature = "os-lite", target_os = "none")))]
pub(crate) fn fold_layer(
    h: u64,
    src_row_abs: u32,
    src_x: u32,
    width: u32,
    height: u32,
    dst_x: u32,
    dst_y: u32,
    opacity: u32,
    content_epoch: u32,
) -> u64 {
    let a = (u64::from(src_row_abs) << 32) | u64::from(src_x);
    let b = (u64::from(width) << 32) | u64::from(height);
    let c = (u64::from(dst_x) << 32) | u64::from(dst_y);
    let d = (u64::from(opacity) << 32) | u64::from(content_epoch);
    fold(fold(fold(fold(h, a), b), c), d)
}

/// Seed of a frame's walk: display geometry + the wallpaper generation (the
/// RT base every present starts from).
#[cfg(any(test, all(feature = "virgl", feature = "os-lite", target_os = "none")))]
pub(crate) fn seed(display_w: u32, display_h: u32, wallpaper_epoch: u32) -> u64 {
    fold(
        fold(FNV_OFFSET, (u64::from(display_w) << 32) | u64::from(display_h)),
        u64::from(wallpaper_epoch),
    )
}

/// What the composite loop should do for one glass layer this present.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg(any(test, all(feature = "virgl", feature = "os-lite", target_os = "none")))]
pub(crate) enum CacheStep {
    /// Backdrop unchanged: one masked draw from cache rows starting here.
    Read(u32),
    /// Backdrop changed: blur live, then copy the result to cache rows here.
    Write(u32),
    /// No cache rows left (or no cache texture): blur live, store nothing.
    Off,
}

/// Per-slot cache bookkeeping. Slots are the layer's INDEX in the retained
/// set — windowd encodes the scene back-to-front deterministically, so a
/// stable scene keeps stable slots and a changed scene changes keys anyway.
#[cfg(any(test, all(feature = "virgl", feature = "os-lite", target_os = "none")))]
pub(crate) struct BlurCacheState {
    keys: [u64; MAX_SLOTS],
    ys: [u32; MAX_SLOTS],
    valid: [bool; MAX_SLOTS],
    /// Bumped whenever the wallpaper texture is (re)uploaded.
    pub(crate) wallpaper_epoch: u32,
    /// The cache texture exists GPU-side (one-shot init succeeded).
    pub(crate) tex_ready: bool,
}

#[cfg(any(test, all(feature = "virgl", feature = "os-lite", target_os = "none")))]
impl BlurCacheState {
    pub(crate) const fn new() -> Self {
        Self {
            keys: [0; MAX_SLOTS],
            ys: [0; MAX_SLOTS],
            valid: [false; MAX_SLOTS],
            wallpaper_epoch: 0,
            tex_ready: false,
        }
    }

    /// Decide read/write/off for glass layer `slot` with key `key` and
    /// height `h`, given the packing cursor `cursor_y` (the caller advances
    /// it by `h` on Read/Write). A hit requires the SAME key at the SAME
    /// packed offset — a reshuffled set re-writes instead of sampling rows
    /// that now belong to another layer.
    pub(crate) fn step(&mut self, slot: usize, key: u64, cursor_y: u32, h: u32) -> CacheStep {
        if !self.tex_ready
            || slot >= self.keys.len()
            || cursor_y.saturating_add(h) > BLUR_CACHE_TEX_H
        {
            if let Some(v) = self.valid.get_mut(slot) {
                *v = false;
            }
            return CacheStep::Off;
        }
        let hit = self.valid[slot] && self.keys[slot] == key && self.ys[slot] == cursor_y;
        self.keys[slot] = key;
        self.ys[slot] = cursor_y;
        self.valid[slot] = true;
        if hit {
            CacheStep::Read(cursor_y)
        } else {
            CacheStep::Write(cursor_y)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> BlurCacheState {
        let mut s = BlurCacheState::new();
        s.tex_ready = true;
        s
    }

    #[test]
    fn second_present_with_the_same_key_reads() {
        // A fresh state starts on wallpaper generation 0 with no texture.
        assert_eq!(BlurCacheState::new().wallpaper_epoch, 0);
        let mut s = ready();
        assert_eq!(s.step(0, 42, 0, 500), CacheStep::Write(0));
        assert_eq!(s.step(0, 42, 0, 500), CacheStep::Read(0));
    }

    #[test]
    fn a_changed_backdrop_rewrites() {
        let mut s = ready();
        assert_eq!(s.step(0, 42, 0, 500), CacheStep::Write(0));
        // Something below moved: the fold differs, the cache re-writes.
        assert_eq!(s.step(0, 43, 0, 500), CacheStep::Write(0));
        assert_eq!(s.step(0, 43, 0, 500), CacheStep::Read(0));
    }

    #[test]
    fn a_moved_packing_offset_never_reads_foreign_rows() {
        let mut s = ready();
        assert_eq!(s.step(1, 7, 100, 300), CacheStep::Write(100));
        // A glass layer vanished below: same key, new offset ⇒ rewrite.
        assert_eq!(s.step(1, 7, 0, 300), CacheStep::Write(0));
    }

    #[test]
    fn overflow_and_missing_texture_fall_back_to_live_blur() {
        let mut s = ready();
        assert_eq!(s.step(0, 1, BLUR_CACHE_TEX_H - 10, 500), CacheStep::Off);
        // And the slot is not left claiming validity for stale rows.
        assert_eq!(s.step(0, 1, 0, 500), CacheStep::Write(0));
        let mut cold = BlurCacheState::new();
        assert_eq!(cold.step(0, 1, 0, 100), CacheStep::Off);
    }

    #[test]
    fn the_fold_separates_field_order() {
        // (src 1, x 2) vs (src 2, x 1) must not collide (a plain XOR would).
        let a = fold_layer(seed(1280, 800, 0), 1, 2, 3, 4, 5, 6, 255, 0);
        let b = fold_layer(seed(1280, 800, 0), 2, 1, 3, 4, 5, 6, 255, 0);
        assert_ne!(a, b);
        // A wallpaper re-upload changes every key.
        assert_ne!(seed(1280, 800, 0), seed(1280, 800, 1));
    }
}

/// [`fold_layer`] over a drawn [`PendingRtLayer`](super::PendingRtLayer)
/// (post transform/scroll resolution — `src_row_abs` is the effective row).
#[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
pub(crate) fn fold_drawn_layer(h: u64, l: &super::PendingRtLayer, src_row_abs: u32) -> u64 {
    fold_layer(
        h,
        src_row_abs,
        l.src_x,
        l.width,
        l.height,
        l.dst_x,
        l.dst_y,
        l.opacity,
        l.content_epoch,
    )
}

// ── virgl emission ──────────────────────────────────────────────────────────
#[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
mod gl {
    use super::{BlurCacheState, CacheStep, BLUR_CACHE_TEX_H};
    use crate::backend::VirtioGpuBackend;
    use crate::gl_scanout::{H_BLEND_BLUR, H_FS_BLUR_ROUND, H_SAMPLER, H_VS, QUAD_RES};
    use crate::protocol::{
        VirtioGpuCtxAttachResource, VirtioGpuResourceCreate3d, VirtioGpuSubmit3d,
        VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE, VIRTIO_GPU_CMD_RESOURCE_CREATE_3D,
        VIRTIO_GPU_CMD_SUBMIT_3D,
    };
    use crate::virgl::{
        Submit3d, PIPE_BIND_RENDER_TARGET, PIPE_BIND_SAMPLER_VIEW, PIPE_FORMAT_B8G8R8A8_UNORM,
        PIPE_PRIM_TRIANGLES, PIPE_SHADER_FRAGMENT, PIPE_SHADER_VERTEX, PIPE_TEXTURE_2D,
    };
    use nexus_gfx::backend::error::GfxError;

    /// The mirror stays pinned to the real retained-set capacity.
    const _: () = assert!(super::MAX_SLOTS == crate::backend::MAX_PENDING_RT_LAYERS);

    /// Cache texture: display-wide, [`BLUR_CACHE_TEX_H`] rows, GPU-only.
    const H_BLUR_CACHE_TEX: u32 = 0xE8;
    /// Sampler view of the cache texture.
    const H_SV_BLUR_CACHE: u32 = 0x4F;

    impl VirtioGpuBackend {
        /// One-shot: create the blur-cache texture + sampler view (the
        /// `backdrop_tex_init` pattern — ctrl commands outside a batch).
        pub(crate) fn blur_cache_tex_init(&mut self) -> Result<(), GfxError> {
            if self.blur_cache.tex_ready {
                return Ok(());
            }
            let create = VirtioGpuResourceCreate3d {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
                resource_id: H_BLUR_CACHE_TEX,
                target: PIPE_TEXTURE_2D,
                format: PIPE_FORMAT_B8G8R8A8_UNORM,
                bind: PIPE_BIND_RENDER_TARGET | PIPE_BIND_SAMPLER_VIEW,
                width: self.display_w,
                height: BLUR_CACHE_TEX_H,
                depth: 1,
                array_size: 1,
                last_level: 0,
                nr_samples: 0,
                flags: 0,
                _padding: 0,
            };
            self.ctrl_submit_struct(&create)?;
            let attach = VirtioGpuCtxAttachResource {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
                resource_id: H_BLUR_CACHE_TEX,
                _padding: 0,
            };
            self.ctrl_submit_struct(&attach)?;
            let mut s = Submit3d::new();
            s.emit_create_sampler_view(
                H_SV_BLUR_CACHE,
                H_BLUR_CACHE_TEX,
                PIPE_FORMAT_B8G8R8A8_UNORM,
            );
            let bb = s.as_bytes();
            let bh = VirtioGpuSubmit3d {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
                size: bb.len() as u32,
                _padding: 0,
            };
            self.ctrl_submit_header_tail(&bh, bb)?;
            self.blur_cache.tex_ready = true;
            Ok(())
        }

        /// One glass layer's blur step in the composite walk: no glass = a
        /// no-op; otherwise the cached read/write/live decision. Returns the
        /// advanced packing cursor.
        pub(crate) fn glass_blur_step(
            &mut self,
            slot: usize,
            below: u64,
            cache_y: u32,
            l: &crate::backend::PendingRtLayer,
        ) -> u32 {
            if l.backdrop_blur == 0 {
                return cache_y;
            }
            let used = self.glass_blur_cached(
                slot,
                below,
                cache_y,
                l.dst_x,
                l.dst_y,
                l.width,
                l.height,
                l.backdrop_blur,
                l.corner_radius,
            );
            if used {
                cache_y.saturating_add(l.height)
            } else {
                cache_y
            }
        }

        /// The cached version of `blur_rt_backdrop` for one glass layer at
        /// walk slot `slot` whose destination-so-far fold is `below`.
        /// `cursor_y` is the packing cursor (advanced by the caller on
        /// Read/Write).
        #[allow(clippy::too_many_arguments)]
        pub(crate) fn glass_blur_cached(
            &mut self,
            slot: usize,
            below: u64,
            cursor_y: u32,
            x: u32,
            y: u32,
            w: u32,
            h: u32,
            radius: u32,
            corner_radius: u32,
        ) -> bool {
            let key = super::fold(
                super::fold(below, (u64::from(x) << 32) | u64::from(y)),
                (u64::from(w) << 40)
                    | (u64::from(h) << 16)
                    | (u64::from(radius) << 8)
                    | u64::from(corner_radius),
            );
            match self.blur_cache.step(slot, key, cursor_y, h) {
                CacheStep::Read(cy) => {
                    let _ = self.draw_cached_backdrop(cy, x, y, w, h, corner_radius);
                    true
                }
                CacheStep::Write(cy) => {
                    let _ = self.blur_rt_backdrop(x, y, w, h, radius, corner_radius);
                    // Snapshot the fresh blur result (pre-content) into the
                    // cache rows: same x, packed row offset.
                    let mut sb = Submit3d::new();
                    sb.emit_resource_copy_region(
                        H_BLUR_CACHE_TEX,
                        x,
                        cy,
                        self.rt_back_res(),
                        x,
                        y,
                        w,
                        h,
                    );
                    let bb = sb.as_bytes();
                    let bh = VirtioGpuSubmit3d {
                        hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
                        size: bb.len() as u32,
                        _padding: 0,
                    };
                    let _ = self.ctrl_submit_header_tail(&bh, bb);
                    true
                }
                CacheStep::Off => {
                    let _ = self.blur_rt_backdrop(x, y, w, h, radius, corner_radius);
                    false
                }
            }
        }

        /// A cache HIT: one masked textured draw from the cache rows — the
        /// gaussian shader with radius 0 degenerates to a single centre tap,
        /// so `FS_BLUR_ROUND` doubles as "masked copy" and the blur edge
        /// keeps the exact SDF curve of the live path.
        fn draw_cached_backdrop(
            &mut self,
            cache_y: u32,
            x: u32,
            y: u32,
            w: u32,
            h: u32,
            corner_radius: u32,
        ) -> Result<(), GfxError> {
            let rr = corner_radius.min(w / 2).min(h / 2) as f32;
            let cx = x as f32 + w as f32 / 2.0;
            let cy = y as f32 + h as f32 / 2.0;
            let rounded = corner_radius > 0;
            let (fs, blend) = if rounded { (H_FS_BLUR_ROUND, H_BLEND_BLUR) } else { (13, 0x20) };
            // CONST[0] = (inv_w, inv_h, radius=0, k); CONST[1] dir=(0,0) and
            // origin shifts fragcoord rows into the packed cache rows.
            let consts = [
                1.0 / self.display_w as f32,
                1.0 / BLUR_CACHE_TEX_H as f32,
                0.0,
                -1.0,
                0.0,
                0.0,
                0.0,
                cache_y as f32 - y as f32,
                -cx,
                -cy,
                w as f32 / 2.0 - rr,
                h as f32 / 2.0 - rr,
                rr,
                0.0,
                0.0,
                0.0,
            ];
            let consts = if rounded { &consts[..] } else { &consts[..8] };
            let mut sb = Submit3d::new();
            sb.emit_bind_object(crate::virgl::VIRGL_OBJECT_BLEND, blend);
            sb.emit_bind_object(crate::virgl::VIRGL_OBJECT_DSA, 0x21);
            sb.emit_bind_object(crate::virgl::VIRGL_OBJECT_RASTERIZER, 0x22);
            sb.emit_bind_object(crate::virgl::VIRGL_OBJECT_VERTEX_ELEMENTS, 0x23);
            sb.emit_bind_sampler_states(PIPE_SHADER_FRAGMENT, 0, &[H_SAMPLER]);
            sb.emit_bind_shader(H_VS, PIPE_SHADER_VERTEX);
            sb.emit_bind_shader(fs, PIPE_SHADER_FRAGMENT);
            sb.emit_set_vertex_buffers(&[(16, 0, QUAD_RES)]);
            sb.emit_set_framebuffer_state(0, &[self.rt_back_surface()]);
            sb.emit_set_viewport_box(x as f32, y as f32, w as f32, h as f32);
            sb.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[H_SV_BLUR_CACHE]);
            sb.emit_set_constant_buffer(PIPE_SHADER_FRAGMENT, consts);
            sb.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
            let bb = sb.as_bytes();
            let bh = VirtioGpuSubmit3d {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
                size: bb.len() as u32,
                _padding: 0,
            };
            self.ctrl_submit_header_tail(&bh, bb)
        }
    }
}
