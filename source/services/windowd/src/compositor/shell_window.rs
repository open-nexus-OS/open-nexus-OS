// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │ ⚠ LEGACY — DO NOT EXTEND. This is a hand-rolled window frame living in    │
//! │ the COMPOSITOR. It is being RETIRED (RFC-0067 P3/P4, see                  │
//! │ docs/dev/ui/patterns/windowing/windows-as-widgets.md). A window is a      │
//! │ WIDGET: `userspace/ui/widgets/window` (`Window` + `frame` + `chrome`) →   │
//! │ a `LayoutNode` → the retained scene graph → nexus-gfx. Window chrome,     │
//! │ resize, sizing, theming, materials belong THERE, not here.                │
//! │ New window behaviour (resize, maximize, frosting, controls, …) goes into  │
//! │ the widget + `layout_to_scene`, NEVER into this file or windowd. Touch    │
//! │ this only to DELETE from it as the migration proceeds.                    │
//! └─────────────────────────────────────────────────────────────────────────┘
//!
//! CONTEXT: the reusable **glass window** component for the desktop shell. One
//! `ShellWindow` owns a movable/closable frame — rounded glass body, cached
//! blurred backdrop, a title bar with a close "x" and drag state — independent
//! of what lives inside it. What scrolls inside a window is the app's: its
//! surface's scroll layers (RFC-0109, ADR-0076).
//!
//! The frame reaches the scanout like every other shell layer —
//! `try_composite_layer` over a pre-blurred backdrop. The body samples the app's
//! own buffer (RFC-0114: the surface's buffer and its newest slot, `BodySource`);
//! this component owns only the frame, glass recipe, hit-testing, drag, blur cache.
//!
//! OWNERS: @ui
//! STATUS: In progress (unified-window refactor, W1)

use crate::atlas::AtlasSurface;
use crate::compositor::damage::DamageRect;
use crate::compositor::{
    DARK_GLASS_BLUR_RADIUS, DARK_GLASS_SATURATION_PERCENT, DISPLAY_ROW_OFFSET, RETAINED_ROW_OFFSET,
};
use nexus_gfx::{BackdropCache, Layer, LayerBackdrop, LayerShadow, RenderCommandEncoder};

/// Shadow-halo margin around the window when computing its damage rect, so the
/// soft drop shadow is restored from the retained plane on move/close.
const SHADOW_HALO_PAD: u32 = 24;

// ── Window chrome: P3.2 windows-as-widgets — the title bar renders FROM the
// (TASK-0337: windowd composes no title bar at all — the window kit draws the
// chrome inside the app's surface; the hand-rasterizer and the widget raster
// cache that lived here are gone.)

// Window hit-testing geometry lives in the host-tested `nexus-widget-window`
// crate (`frame`) so every `ShellWindow` (chat + search) shares one
// implementation; re-export so existing `shell_window::{Frame,WindowPress}` call
// sites keep working (RFC-0067 P3: window geometry is a widget concern).
pub(crate) use nexus_widget_window::{Frame, ResizeEdge, WindowPress};

// The per-region material-glass recipe lives in its own module (ratchet
// split); re-exported here so `scene.rs` keeps one import path.
pub(crate) use crate::compositor::material_glass::{composite_material_glass, MaterialLayerParams};

/// A movable/closable glass window. The content is the app's buffer (`BodySource`, RFC-0114);
/// this struct owns the frame, glass and drag.
pub(crate) struct ShellWindow {
    /// Title shown in the title bar (used by the renderer in W3; kept here so the
    /// component fully describes the window).
    #[allow(dead_code)]
    pub(crate) title: &'static str,
    /// Top-left on the display (display-space).
    pub(crate) x: i32,
    pub(crate) y: i32,
    /// Window size: the surface's (TASK-0337: the app draws its chrome inside it).
    pub(crate) w: u32,
    pub(crate) h: u32,
    /// Glass frame parameters applied by the composite.
    pub(crate) radius: u32,
    pub(crate) shadow_blur: u32,
    pub(crate) shadow_offset_y: i32,
    pub(crate) shadow_alpha: u32,
    /// Visible on screen.
    pub(crate) visible: bool,
    /// Active title-bar drag: cursor offset from the window origin at grab.
    pub(crate) drag: Option<(i32, i32)>,
    /// Floating frame remembered while fullscreen, restored on toggle-off.
    pub(crate) fullscreen_restore: Option<(i32, i32, u32, u32)>,
    /// Cached blurred backdrop validity (blur once per open/move).
    pub(crate) blur_valid: bool,
    /// Cached blurred backdrop behind the window — where the CPU executor composes; a GPU
    /// blurs and caches the glass itself.
    pub(crate) blur_cache: Option<AtlasSurface>,
}

impl ShellWindow {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        title: &'static str,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        radius: u32,
        shadow_blur: u32,
        shadow_offset_y: i32,
        shadow_alpha: u32,
    ) -> Self {
        Self {
            title,
            x,
            y,
            w,
            h,
            radius,
            shadow_blur,
            shadow_offset_y,
            shadow_alpha,
            visible: false,
            drag: None,
            fullscreen_restore: None,
            blur_valid: false,
            blur_cache: None,
        }
    }

    /// The blur cache (the CPU executor's; OPTIONAL — a large one may not fit the atlas and the
    /// window then composites without it), invalidated: the next present builds it.
    pub(crate) fn set_blur_cache(&mut self, blur_cache: Option<AtlasSurface>) {
        self.blur_cache = blur_cache;
        self.blur_valid = false;
    }

    /// The blur cache taken out (the window closes), for the caller to return to the atlas.
    pub(crate) fn take_blur_cache(&mut self) -> Option<AtlasSurface> {
        self.blur_valid = false;
        self.blur_cache.take()
    }

    /// The pure-geometry frame (host-tested hit-testing / clamp / damage live in
    /// [`Frame`] in `nexus-widget-window`; this component owns only the presentation).
    pub(crate) fn frame(&self) -> Frame {
        Frame {
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.h,
            // No compositor chrome (TASK-0337): the widget's title geometry is empty.
            title_h: 0,
            close_w: 0,
        }
    }

    /// Apply a new display-space frame (move + resize in one step, TASK-0070
    /// Phase 3); any change invalidates the blur cache. The content follows when the app
    /// re-creates its surface at the new size.
    pub(crate) fn set_frame(&mut self, x: i32, y: i32, w: u32, h: u32) {
        self.w = w;
        self.h = h;
        self.x = x;
        self.y = y;
        self.blur_valid = false;
    }

    /// Enter fullscreen: remember the floating frame and take the whole
    /// display (TRUE fullscreen — the content re-renders at display size via
    /// the Phase-3 resize machinery; the composite covers the chrome).
    pub(crate) fn enter_fullscreen(&mut self, y: i32, mode_w: u32, mode_h: u32) {
        if self.fullscreen_restore.is_none() {
            self.fullscreen_restore = Some((self.x, self.y, self.w, self.h));
        }
        self.end_drag();
        self.set_frame(0, y, mode_w, mode_h);
    }

    /// Leave fullscreen: return to the remembered floating frame.
    pub(crate) fn leave_fullscreen(&mut self) {
        if let Some((x, y, w, h)) = self.fullscreen_restore.take() {
            self.set_frame(x, y, w, h);
        }
    }

    /// Begin a title-bar drag at the press point.
    pub(crate) fn begin_drag(&mut self, cx: i32, cy: i32) {
        self.drag = Some((cx - self.x, cy - self.y));
    }

    pub(crate) fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub(crate) fn end_drag(&mut self) {
        self.drag = None;
    }

    /// Where an in-progress drag would put the window for the pointer at (`cx`, `cy`), clamped
    /// to the drag envelope — nothing moved (TASK-0336 P3: the move rides as a transform
    /// until the release).
    pub(crate) fn drag_target(
        &self,
        cx: i32,
        cy: i32,
        mode_w: u32,
        bounds: nexus_widget_window::DragBounds,
    ) -> Option<(i32, i32)> {
        let (gx, gy) = self.drag?;
        Some(self.frame().clamp_pos(cx - gx, cy - gy, mode_w, bounds))
    }

    /// Continue an in-progress drag, clamped to the status-bar drag envelope
    /// (`DragBounds`: the grab strip stays between the bars, the body is free).
    /// Returns the previous damage rect when it moved.
    #[rustfmt::skip]
    pub(crate) fn drag_to(
        &mut self, cx: i32, cy: i32, mode_w: u32, mode_h: u32,
        bounds: nexus_widget_window::DragBounds,
    ) -> Option<DamageRect> {
        let (gx, gy) = self.drag?;
        let old = self.damage_rect(mode_w, mode_h);
        let (nx, ny) = self.frame().clamp_pos(cx - gx, cy - gy, mode_w, bounds);
        if nx == self.x && ny == self.y {
            return None;
        }
        self.x = nx;
        self.y = ny;
        self.blur_valid = false; // backdrop under the window changed
        Some(old)
    }

    /// Damage rect of the window plus its shadow halo.
    pub(crate) fn damage_rect(&self, mode_w: u32, mode_h: u32) -> DamageRect {
        let (x, y, width, height) = self.frame().damage_bounds(SHADOW_HALO_PAD, mode_w, mode_h);
        DamageRect { x, y, width, height }
    }

    /// Snapshot the immutable values the glass composite needs, so the caller can
    /// take them before borrowing the command buffer's encoder: the body samples
    /// `body` (the app's buffer, its newest slot). The blurred backdrop is OPTIONAL
    /// (TASK-0070 Phase 3): a resized/fullscreen window whose blur cache is missing
    /// or too small composites without it (the translucent body sits straight on the
    /// wallpaper — allowed) instead of sampling past the cache.
    pub(crate) fn glass_params(&self, body: BodySource) -> GlassCompositeParams {
        // Never sample past the content. The frame (`self.w`/`self.h`) can transiently exceed
        // the created surface during a resize/fullscreen negotiation: the client owns its
        // surface and re-creates it a round-trip AFTER the frame grows. Clamp the extent to what
        // the content holds; the frame catches up when the re-created surface presents.
        let w = self.w.min(body.width);
        let h = self.h.min(body.height);
        let blur = match self.blur_cache {
            Some(cache) if cache.width >= w && cache.height >= h => Some(GlassBlur {
                cache_row: cache.abs_row,
                cache_x: cache.x,
                valid: self.blur_valid,
            }),
            _ => None,
        };
        GlassCompositeParams {
            body,
            blur,
            x: self.x.max(0) as u32,
            y: self.y.max(0) as u32,
            w,
            h,
            // Default: content fills the frame (`0`). The AppClient resize path
            // overrides these (frame `w`/`h`, band content) in `scene.rs`.
            content_w: 0,
            content_h: 0,
            radius: self.radius,
            layer_id: 0,
            opacity: 255,
            shadow_blur: self.shadow_blur,
            shadow_offset_y: self.shadow_offset_y,
            shadow_alpha: self.shadow_alpha,
            solid: false,
            gpu_glass: false,
            key: 0,
        }
    }

    /// Composite the glass window onto the display: restore + blur the backdrop
    /// (once per open/move, cached thereafter), then composite the atlas content
    /// with rounded corners + drop shadow. The proven Search/Chat glass recipe,
    /// now shared. Returns whether the window's body went into the frame (on the
    /// display and taken by the encoder) — what `windowd: windows drawn` counts.
    pub(crate) fn composite_glass(
        encoder: &mut RenderCommandEncoder<'_>,
        p: GlassCompositeParams,
        mode_w: u32,
        mode_h: u32,
    ) -> bool {
        if p.x >= mode_w || p.y >= mode_h {
            return false;
        }
        let w = p.w.min(mode_w.saturating_sub(p.x));
        let h = p.h.min(mode_h.saturating_sub(p.y));
        // Static glass window: no shadow halo, blur cached after first settle (layer SSOT).
        encoder
            .composite_layer_full(
                &Layer {
                    src_row_abs: 0,
                    src_x: 0,
                    width: w,
                    height: h,
                    content_w: p.content_w,
                    content_h: p.content_h,
                    dst_x: p.x,
                    dst_y: p.y,
                    opacity: p.opacity as u32,
                    corner_radius: p.radius,
                    layer_id: p.layer_id,
                    content_epoch: p.body.epoch,
                    scroll_band_top_abs: 0,
                    scroll_band_h: 0,
                    source: p.body.source,
                    slot: p.body.slot,
                    shadow: (p.shadow_alpha > 0).then_some(LayerShadow {
                        blur: p.shadow_blur,
                        offset_y: p.shadow_offset_y,
                        alpha: p.shadow_alpha,
                    }),
                    opaque: p.solid && p.content_w == 0 && p.opacity == 255,
                    solid: false,
                    key: p.key,
                    backdrop: if p.solid {
                        None
                    } else if p.content_w > 0 {
                        // Live resize: the frame grew past the content band. The blur
                        // cache is content-size and can't cover the frame, so blur
                        // LIVE at the frame rect (no cache) — the exposed area beyond
                        // the content is frosted glass ("glass frame grows"). Snaps
                        // back to the cached path when the client re-renders at size.
                        Some(LayerBackdrop {
                            blur_radius: DARK_GLASS_BLUR_RADIUS,
                            saturation_percent: DARK_GLASS_SATURATION_PERCENT,
                            restore_halo_pad: 0,
                            retained_src_y_offset: RETAINED_ROW_OFFSET,
                            cache: BackdropCache::None,
                        })
                    } else {
                        // The CPU executor's cache when the window has one; a GPU that composes
                        // blurs and caches the glass itself (its plan), so the glass is declared
                        // without one.
                        let cache = match p.blur {
                            Some(blur) => Some(glass_cache(blur)),
                            None => p.gpu_glass.then_some(BackdropCache::None),
                        };
                        cache.map(|cache| LayerBackdrop {
                            blur_radius: DARK_GLASS_BLUR_RADIUS,
                            saturation_percent: DARK_GLASS_SATURATION_PERCENT,
                            restore_halo_pad: 0,
                            retained_src_y_offset: RETAINED_ROW_OFFSET,
                            cache,
                        })
                    },
                },
                (mode_w, mode_h),
            )
            .is_ok()
    }
}

/// Map a window's blur-cache state to the layer SSOT's cache mode: build + write
/// the blurred backdrop on the first settled present, reuse it thereafter.
fn glass_cache(blur: GlassBlur) -> BackdropCache {
    if blur.valid {
        BackdropCache::Read {
            cache_x: blur.cache_x,
            cache_row_abs: blur.cache_row,
            display_row_offset: DISPLAY_ROW_OFFSET,
        }
    } else {
        BackdropCache::Write {
            cache_x: blur.cache_x,
            cache_row_abs: blur.cache_row,
            display_row_offset: DISPLAY_ROW_OFFSET,
        }
    }
}

/// The window's blurred-backdrop cache surface, when one exists AND fits the
/// current window size (see `glass_params`).
#[derive(Clone, Copy)]
pub(crate) struct GlassBlur {
    pub(crate) cache_row: u32,
    pub(crate) cache_x: u32,
    pub(crate) valid: bool,
}

/// What a window's body samples (RFC-0114 §6): the surface's buffer, the slot of its newest
/// present and that present's seq as the content's generation, and the content's size.
#[derive(Clone, Copy)]
pub(crate) struct BodySource {
    pub(crate) source: u32,
    pub(crate) slot: u8,
    pub(crate) epoch: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// Copy snapshot of the values [`ShellWindow::composite_glass`] needs — taken
/// before the per-frame command-buffer encoder borrows the runtime.
#[derive(Clone, Copy)]
pub(crate) struct GlassCompositeParams {
    /// What the body samples: the app's buffer, its newest slot.
    pub(crate) body: BodySource,
    /// Blurred-backdrop cache, when present and sized for the window.
    pub(crate) blur: Option<GlassBlur>,
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) w: u32,
    pub(crate) h: u32,
    /// Content sub-size (`0` = same as `w`/`h`). When the CONTENT band is smaller
    /// than the glass frame (`w`/`h`) — a live resize where the frame grows past
    /// the client's created surface — the backdrop blur fills `w`×`h` but the
    /// content is drawn at `content_w`×`content_h` in the top-left. The rest is
    /// blurred glass ("glass frame grows, content 1:1"). See `nexus_gfx::Layer`.
    pub(crate) content_w: u32,
    pub(crate) content_h: u32,
    pub(crate) radius: u32,
    /// Transform identity (Track C2): tags every slice of this window's
    /// composite so the present's `Command::LayerTransform` moves/fades/scales
    /// the whole window on the GPU. `slot + 1`; 0 = untagged.
    pub(crate) layer_id: u32,
    /// The window's own opacity (255 = opaque); the transition's fade is the
    /// present's `Command::LayerTransform`, multiplied on the GPU.
    pub(crate) opacity: u8,
    pub(crate) shadow_blur: u32,
    pub(crate) shadow_offset_y: i32,
    pub(crate) shadow_alpha: u32,
    /// Solid materials (TASK-0337): the window's page base is opaque — no backdrop is blurred,
    /// and the body occludes what lies below it.
    pub(crate) solid: bool,
    /// The glass is blurred by a GPU that composes (windowd's GPU context): declared without a
    /// CPU cache — the window holds no blur band.
    pub(crate) gpu_glass: bool,
    /// The body's identity across presents (RFC-0109: the planner's match).
    pub(crate) key: u32,
}
