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
//! The frame reaches the virgl scanout like every other shell layer —
//! `try_composite_layer` over a pre-blurred backdrop (retained Plane 1 is
//! invisible on virgl, see [[black-screen-is-2d-3d-dual-not-host]]). The body
//! content is rasterized into the window's atlas surface by the caller; this
//! component owns only chrome, glass recipe, hit-testing, drag, blur cache.
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

/// A movable/closable glass window. The content is supplied by the caller (copied into
/// `atlas`, its base, and `store`, RFC-0109); this struct owns the frame, glass and drag.
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
    /// Set when the atlas surface needs re-rasterizing (filter/scroll/hover).
    pub(crate) surface_dirty: bool,
    /// Cached blurred backdrop validity (blur once per open/move).
    pub(crate) blur_valid: bool,
    /// The surface's BASE layer in the atlas — `Some` only while the window is *mounted*
    /// (shown). Acquired from the atlas allocator on show, released on hide, so a closed window
    /// costs zero atlas rows (the pool model).
    pub(crate) atlas: Option<AtlasSurface>,
    /// The surface's STORE in the atlas (RFC-0109: its bands and patches, the store's used
    /// width) — its own region, placed beside the base wherever it fits; `None` without one.
    pub(crate) store: Option<AtlasSurface>,
    /// Cached blurred backdrop behind the window — mounted alongside `atlas` where the CPU
    /// executor composes; a GPU blurs and caches the glass itself.
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
            surface_dirty: true,
            blur_valid: false,
            atlas: None,
            store: None,
            blur_cache: None,
        }
    }

    /// True while the window holds its content surface (is shown). The blur
    /// cache is OPTIONAL (TASK-0070 Phase 3: a fullscreen-sized blur cache may
    /// not fit the pool — the window then composites without backdrop).
    pub(crate) fn is_mounted(&self) -> bool {
        self.atlas.is_some()
    }

    /// Attach freshly-allocated atlas regions (on show/resize): the base, the store, the blur
    /// cache. Forces a re-render and invalidates the blur cache so the new rows are painted
    /// before composite.
    pub(crate) fn mount(
        &mut self,
        (atlas, store): (AtlasSurface, Option<AtlasSurface>),
        blur_cache: Option<AtlasSurface>,
    ) {
        self.atlas = Some(atlas);
        self.store = store;
        self.blur_cache = blur_cache;
        self.surface_dirty = true;
        self.blur_valid = false;
    }

    /// Detach the atlas regions (on hide/resize) so the caller can return them to the
    /// allocator: `[base, store, blur cache]` when mounted.
    pub(crate) fn unmount(&mut self) -> Option<[Option<AtlasSurface>; 3]> {
        self.blur_valid = false;
        let base = self.atlas.take()?;
        Some([Some(base), self.store.take(), self.blur_cache.take()])
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
    /// Phase 3). A size change dirties the content surface (the caller
    /// re-renders — and re-mounts pool surfaces — at the new size); any
    /// change invalidates the blur cache.
    pub(crate) fn set_frame(&mut self, x: i32, y: i32, w: u32, h: u32) {
        if w != self.w || h != self.h {
            self.w = w;
            self.h = h;
            self.surface_dirty = true;
        }
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
    /// take them before borrowing the command buffer's encoder. `None` when the
    /// window is unmounted (no surfaces → nothing to composite). The blurred
    /// backdrop is OPTIONAL (TASK-0070 Phase 3): a resized/fullscreen window
    /// whose blur cache is missing or too small composites without it (the
    /// translucent body sits straight on the wallpaper — allowed) instead of
    /// sampling past the cache into foreign atlas rows.
    pub(crate) fn glass_params(&self) -> Option<GlassCompositeParams> {
        let atlas = self.atlas?;
        // Never composite past the atlas band. The frame (`self.w`/`self.h`) can
        // transiently exceed the created surface during a resize/fullscreen
        // negotiation: the client owns its surface, so the band is re-allocated a
        // round-trip AFTER the frame grows. Reading `self.w × self.h` from a
        // smaller band would sample the rows of ADJACENT windows packed after it
        // in the atlas — which paints garbage over the whole scene. Clamp the
        // glass extent to what the band actually backs; the frame catches up
        // visually when the re-created surface lands.
        let w = self.w.min(atlas.width);
        let h = self.h.min(atlas.height);
        let blur = match self.blur_cache {
            Some(cache) if cache.width >= w && cache.height >= h => Some(GlassBlur {
                cache_row: cache.abs_row,
                cache_x: cache.x,
                valid: self.blur_valid,
            }),
            _ => None,
        };
        Some(GlassCompositeParams {
            atlas_row: atlas.abs_row,
            atlas_x: atlas.x,
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
        })
    }

    /// Composite the glass window onto the display: restore + blur the backdrop
    /// (once per open/move, cached thereafter), then composite the atlas content
    /// with rounded corners + drop shadow. The proven Search/Chat glass recipe,
    /// now shared. Returns true when the blur cache was (re)built this present so
    /// the caller can mark `blur_valid = true`.
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
        let _ = encoder.composite_layer_full(
            &Layer {
                src_row_abs: p.atlas_row,
                src_x: p.atlas_x,
                width: w,
                height: h,
                content_w: p.content_w,
                content_h: p.content_h,
                dst_x: p.x,
                dst_y: p.y,
                opacity: p.opacity as u32,
                corner_radius: p.radius,
                layer_id: p.layer_id,
                content_epoch: crate::atlas::atlas_content_epoch(),
                scroll_band_top_abs: 0,
                scroll_band_h: 0,
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
        );
        // `built_blur`: the cache was (re)built this present iff a cache exists
        // and was invalid (no cache → nothing was built).
        p.blur.map(|b| !b.valid).unwrap_or(false)
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

/// Copy snapshot of the values [`ShellWindow::composite_glass`] needs — taken
/// before the per-frame command-buffer encoder borrows the runtime.
#[derive(Clone, Copy)]
pub(crate) struct GlassCompositeParams {
    /// Atlas content surface row + column (`src_x` — non-zero when 2D-packed).
    pub(crate) atlas_row: u32,
    pub(crate) atlas_x: u32,
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
