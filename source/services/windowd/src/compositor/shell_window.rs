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
//! blurred backdrop, a title bar with a close "x", drag state, and a scroll
//! offset — independent of what list lives inside it. The Search window is the
//! first instance; the Chat window migrates onto it next (W3), so both windows
//! share ONE appearance (the nicer Search look) and ONE scroll mechanism.
//!
//! The frame reaches the virgl scanout the same way every other shell layer
//! does — `try_composite_layer` over a pre-blurred backdrop (the retained
//! Plane 1 is invisible on virgl, see [[black-screen-is-2d-3d-dual-not-host]]).
//! The body content (filtered words / chat messages) is rasterized into the
//! window's atlas surface by the caller; this component owns only the chrome,
//! the glass recipe, hit-testing, drag, and the blur cache.
//!
//! OWNERS: @ui
//! STATUS: In progress (unified-window refactor, W1)

use crate::atlas::AtlasSurface;
use crate::compositor::{
    DARK_GLASS_BLUR_RADIUS, DARK_GLASS_SATURATION_PERCENT, DISPLAY_ROW_OFFSET, RETAINED_ROW_OFFSET,
};
use crate::error::WindowdError;
use crate::compositor::damage::DamageRect;
use nexus_gfx::{BackdropCache, Layer, LayerBackdrop, LayerShadow, RenderCommandEncoder};

/// Shadow-halo margin around the window when computing its damage rect, so the
/// soft drop shadow is restored from the retained plane on move/close.
const SHADOW_HALO_PAD: u32 = 24;

// ── Shared window chrome: title bar tint + title text + close "x" ──
// One title-bar renderer for every ShellWindow so the chat and search windows
// share the exact same look (the user's "same close x / same frame" goal). The
// glass body + corners + shadow come from the composite; this paints the atlas.
// Titles render with the 13px runtime face (TASK-0070 Phase 6).
const TITLE_FONT: crate::text::FontSize = crate::text::FontSize::Small;
/// Title-bar background tint (slightly lighter than the body for separation),
/// hover highlight behind the close zone, and the chrome text colour. Straight
/// alpha — gpud's layer blend composites these over the blurred backdrop.
// The title bar is OPAQUE (theme `surface_alt`, alpha 255): composited FIXED on
// top of the scrollable window body, it must fully occlude scrolled rows passing
// underneath — a translucent bar would let them bleed through. `HOVER_TINT[3]` is
// the SSOT for the button hover-cell alpha (the theme swaps only the RGB).
const HOVER_TINT: [u8; 4] = [120, 110, 100, 96];

/// Draw one window-local row of the shared title bar: the header tint across
/// the bar, the title on the left, and the right-aligned window controls
/// `[– □ ×]` (TASK-0070 Phase 2), the hovered one highlighted. Button zone
/// geometry comes from the SAME host-tested [`Frame`] math the hit-tester uses
/// (`Frame::button_local_x`), so hover/press and pixels can never disagree.
/// `local_y` is window-local; rows `>= title_h` are left untouched (the body).
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
/// Title-bar wash alpha over the blurred backdrop (frosted chrome, not a slab).
const TITLE_BAR_TINT_ALPHA: u8 = 150;

pub(crate) fn draw_title_bar_row(
    local_y: u32,
    row: &mut [u8],
    w: u32,
    title: &str,
    title_h: u32,
    close_w: u32,
    hover: Option<TitleButton>,
    radius: u32,
    tk: &crate::theme::ThemeTokens,
) -> Result<(), WindowdError> {
    if local_y >= title_h {
        return Ok(());
    }
    // Title bar is part of the frosted material — a TRANSLUCENT wash over the
    // blurred backdrop, not an opaque strip (the opaque `surface_alt` fill made
    // the whole bar read as a solid slab and hid the gaussian entirely). Text +
    // button glyphs use fg; the button hover cell uses accent (TASK-0072 P9).
    let hover_col = crate::theme::with_alpha(tk.accent, HOVER_TINT[3]);
    let glyph_tint = Some(crate::theme::rgb3(tk.fg));
    write_tint_span(row, 0, w, crate::theme::with_alpha(tk.surface_alt, TITLE_BAR_TINT_ALPHA));
    let text_top = title_h.saturating_sub(crate::text::line_height(TITLE_FONT)) / 2;
    crate::text::draw_text_row(
        row,
        local_y,
        text_top as i32,
        14,
        w,
        title.chars(),
        TITLE_FONT,
        tk.fg,
    );
    // Zone geometry SSOT: a zero-positioned frame gives the window-local x.
    let zones = Frame { x: 0, y: 0, w, h: title_h, title_h, close_w };
    for (button, icon, dim) in [
        (
            TitleButton::Minimize,
            crate::assets::MINIMIZE_ICON_BGRA,
            crate::assets::MINIMIZE_ICON_DIM,
        ),
        (
            TitleButton::Maximize,
            crate::assets::MAXIMIZE_ICON_BGRA,
            crate::assets::MAXIMIZE_ICON_DIM,
        ),
        (TitleButton::Close, crate::assets::CLOSE_ICON_BGRA, crate::assets::CLOSE_ICON_DIM),
    ] {
        let bx = zones.button_local_x(button);
        if hover == Some(button) {
            write_tint_span(row, bx, bx + close_w, hover_col);
        }
        // Real Lucide glyphs (monochrome, straight-alpha) recolored to the theme
        // fg and centred in each zone.
        let cy0 = title_h.saturating_sub(dim) / 2;
        if local_y >= cy0 && local_y < cy0 + dim {
            let cix = bx + close_w.saturating_sub(dim) / 2;
            crate::assets::blend_icon_row(row, cix, icon, dim, local_y - cy0, 255, glyph_tint);
        }
    }
    // Round the TOP corners at the surface level: clear (make transparent) the
    // pixels of the top-left/top-right `radius`×`radius` squares that fall OUTSIDE
    // the corner arc. The window body underneath is rounded by the composite SDF
    // with the same radius, so the cleared corners reveal it — giving rounded top
    // corners with a straight bottom edge (the title bar is composited square).
    round_top_corners(local_y, row, w, radius);
    Ok(())
}

/// Clear the out-of-arc pixels of the two TOP corners (transparent), so a
/// square-composited title bar still presents rounded top corners.
fn round_top_corners(local_y: u32, row: &mut [u8], w: u32, radius: u32) {
    if radius == 0 || local_y >= radius {
        return;
    }
    let rp = (row.len() / 4) as u32;
    let r = radius;
    // Vertical distance from the arc centre (at y = r), squared.
    let dy = r - 1 - local_y; // local_y in [0, r)
    let dy2 = dy as u32 * dy as u32;
    let r2 = r * r;
    // Top-left: arc centre at x = r. Clear x in [0, r) outside the arc.
    for x in 0..r.min(rp) {
        let dx = r - 1 - x;
        if dx as u32 * dx as u32 + dy2 > r2 {
            let idx = x as usize * 4;
            row[idx..idx + 4].copy_from_slice(&[0, 0, 0, 0]);
        }
    }
    // Top-right: arc centre at x = w - r. Clear x in [w - r, w) outside the arc.
    let start = w.saturating_sub(r);
    for x in start..w.min(rp) {
        let dx = x - start;
        if dx * dx + dy2 > r2 {
            let idx = x as usize * 4;
            row[idx..idx + 4].copy_from_slice(&[0, 0, 0, 0]);
        }
    }
}

/// Write one straight-alpha BGRA span (gpud's layer blend does the SRC_ALPHA
/// compositing over the blurred backdrop). Shared with the dock renderer.
pub(crate) fn write_tint_span(row: &mut [u8], x0: u32, x1: u32, c: [u8; 4]) {
    let rp = (row.len() / 4) as u32;
    for px in x0.min(rp)..x1.min(rp) {
        let idx = px as usize * 4;
        row[idx..idx + 4].copy_from_slice(&c);
    }
}

// Window hit-testing geometry lives in the host-tested `nexus-widget-window`
// crate (`frame`) so every `ShellWindow` (chat + search) shares one
// implementation; re-export so existing `shell_window::{Frame,WindowPress}` call
// sites keep working (RFC-0067 P3: window geometry is a widget concern).
pub(crate) use nexus_widget_window::{Frame, ResizeEdge, TitleButton, WindowPress};

/// A movable/closable glass window. The content list is supplied by the caller
/// (rendered into `atlas`); this struct owns the frame, glass, drag and scroll.
pub(crate) struct ShellWindow {
    /// Title shown in the title bar (used by the renderer in W3; kept here so the
    /// component fully describes the window).
    #[allow(dead_code)]
    pub(crate) title: &'static str,
    /// Top-left on the display (display-space).
    pub(crate) x: i32,
    pub(crate) y: i32,
    /// Window size. `h` is the full window height (title + body).
    pub(crate) w: u32,
    pub(crate) h: u32,
    /// Title bar height + close-button width (chrome geometry, reusable).
    pub(crate) title_h: u32,
    pub(crate) close_w: u32,
    /// Glass frame parameters applied by the composite.
    pub(crate) radius: u32,
    pub(crate) shadow_blur: u32,
    pub(crate) shadow_offset_y: i32,
    pub(crate) shadow_alpha: u32,
    /// Visible on screen.
    pub(crate) visible: bool,
    /// Active title-bar drag: cursor offset from the window origin at grab.
    pub(crate) drag: Option<(i32, i32)>,
    /// Hovered title-bar button `[– □ ×]` (re-renders the title bar on change).
    pub(crate) title_hover: Option<TitleButton>,
    /// Floating frame remembered while fullscreen, restored on toggle-off.
    pub(crate) fullscreen_restore: Option<(i32, i32, u32, u32)>,
    /// Scroll offset of the body list (rows, for now; GPU-offset in W2).
    pub(crate) scroll: u32,
    /// Set when the atlas surface needs re-rasterizing (filter/scroll/hover).
    pub(crate) surface_dirty: bool,
    /// Cached blurred backdrop validity (blur once per open/move).
    pub(crate) blur_valid: bool,
    /// Content surface (off-screen atlas) — `Some` only while the window is
    /// *mounted* (shown). Acquired from the atlas allocator on show, released on
    /// hide, so a closed window costs zero atlas rows (the pool model).
    pub(crate) atlas: Option<AtlasSurface>,
    /// Cached blurred backdrop behind the window — mounted alongside `atlas`.
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
        title_h: u32,
        close_w: u32,
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
            title_h,
            close_w,
            radius,
            shadow_blur,
            shadow_offset_y,
            shadow_alpha,
            visible: false,
            drag: None,
            title_hover: None,
            fullscreen_restore: None,
            scroll: 0,
            surface_dirty: true,
            blur_valid: false,
            atlas: None,
            blur_cache: None,
        }
    }

    /// True while the window holds its content surface (is shown). The blur
    /// cache is OPTIONAL (TASK-0070 Phase 3: a fullscreen-sized blur cache may
    /// not fit the pool — the window then composites without backdrop).
    pub(crate) fn is_mounted(&self) -> bool {
        self.atlas.is_some()
    }

    /// Attach freshly-allocated atlas surfaces (on show/resize). Forces a
    /// re-render and invalidates the blur cache so the new rows are painted
    /// before composite.
    pub(crate) fn mount(&mut self, atlas: AtlasSurface, blur_cache: Option<AtlasSurface>) {
        self.atlas = Some(atlas);
        self.blur_cache = blur_cache;
        self.surface_dirty = true;
        self.blur_valid = false;
    }

    /// Detach the atlas surfaces (on hide/resize) so the caller can return
    /// them to the allocator. Returns `(content, blur_cache)` when mounted.
    pub(crate) fn unmount(&mut self) -> Option<(AtlasSurface, Option<AtlasSurface>)> {
        self.blur_valid = false;
        self.atlas.take().map(|a| (a, self.blur_cache.take()))
    }

    /// The pure-geometry frame (host-tested hit-testing / clamp / damage live in
    /// [`Frame`] in `nexus-widget-window`; this component owns only the presentation).
    pub(crate) fn frame(&self) -> Frame {
        Frame { x: self.x, y: self.y, w: self.w, h: self.h, title_h: self.title_h, close_w: self.close_w }
    }

    /// True if `(cx, cy)` is anywhere inside the window.
    pub(crate) fn contains(&self, cx: i32, cy: i32) -> bool {
        self.frame().contains(cx, cy)
    }

    /// The title-bar button `[– □ ×]` under `(cx, cy)`, if any.
    pub(crate) fn title_button_at(&self, cx: i32, cy: i32) -> Option<TitleButton> {
        self.frame().title_button_at(cx, cy)
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
    pub(crate) fn enter_fullscreen(&mut self, mode_w: u32, mode_h: u32) {
        if self.fullscreen_restore.is_none() {
            self.fullscreen_restore = Some((self.x, self.y, self.w, self.h));
        }
        self.end_drag();
        self.set_frame(0, 0, mode_w, mode_h);
    }

    /// Leave fullscreen: return to the remembered floating frame.
    pub(crate) fn leave_fullscreen(&mut self) {
        if let Some((x, y, w, h)) = self.fullscreen_restore.take() {
            self.set_frame(x, y, w, h);
        }
    }

    /// Resolve a primary press to a window region.
    pub(crate) fn press(&self, cx: i32, cy: i32) -> WindowPress {
        self.frame().press(cx, cy)
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

    /// Continue an in-progress drag, clamping the window to the display. Returns
    /// the previous damage rect (to repaint the vacated area) when it moved.
    pub(crate) fn drag_to(&mut self, cx: i32, cy: i32, mode_w: u32, mode_h: u32) -> Option<DamageRect> {
        let (gx, gy) = self.drag?;
        let old = self.damage_rect(mode_w, mode_h);
        let (nx, ny) = self.frame().clamp_pos(cx - gx, cy - gy, mode_w, mode_h);
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
            shadow_blur: self.shadow_blur,
            shadow_offset_y: self.shadow_offset_y,
            shadow_alpha: self.shadow_alpha,
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
        // A static glass window: no shadow halo (pad 0), blur cached after the
        // first settled present. Routed through the layer SSOT.
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
                opacity: 255,
                corner_radius: p.radius,
                scroll_id: 0,
                shadow: (p.shadow_alpha > 0).then_some(LayerShadow {
                    blur: p.shadow_blur,
                    offset_y: p.shadow_offset_y,
                    alpha: p.shadow_alpha,
                }),
                backdrop: if p.content_w > 0 {
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
                    p.blur.map(|blur| LayerBackdrop {
                        blur_radius: DARK_GLASS_BLUR_RADIUS,
                        saturation_percent: DARK_GLASS_SATURATION_PERCENT,
                        restore_halo_pad: 0,
                        retained_src_y_offset: RETAINED_ROW_OFFSET,
                        cache: glass_cache(blur),
                    })
                },
            },
            (mode_w, mode_h),
        );
        // `built_blur`: the cache was (re)built this present iff a cache exists
        // and was invalid (no cache → nothing was built).
        p.blur.map(|b| !b.valid).unwrap_or(false)
    }

    /// Composite a glass window whose body **scrolls** by a GPU source-row offset
    /// while the title bar stays fixed — the chat mechanism, now shared so the
    /// chat and search windows scroll identically (render once, GPU offset, no
    /// per-frame re-render). The halo is restored from the retained plane each
    /// present so the soft shadow never accumulates (virgl rebuilds the scanout
    /// every present). `content_offset` is the body's scroll-within-surface in
    /// rows; `header_h` is the fixed title-bar height. Returns true when the blur
    /// cache was (re)built this present.
    pub(crate) fn composite_scrollable_glass(
        encoder: &mut RenderCommandEncoder<'_>,
        p: GlassCompositeParams,
        scroll_id: u32,
        content_offset: u32,
        header_h: u32,
        mode_w: u32,
        mode_h: u32,
    ) -> bool {
        if p.x >= mode_w || p.y >= mode_h {
            return false;
        }
        // Body layer: restores the window + shadow-pad halo from the retained plane
        // (so the soft shadow never trails), blurs/caches the window rect, and
        // composites the surface SCROLLABLE (sampled at the scroll offset so gpud
        // retains it for the cheap re-sample fast path). Routed through the layer
        // SSOT, which owns the halo math + the cached-with-halo restore.
        let pad = p.shadow_blur.saturating_add(p.shadow_offset_y.unsigned_abs());
        let _ = encoder.composite_layer_full(
            &Layer {
                src_row_abs: p.atlas_row + content_offset,
                src_x: p.atlas_x,
                width: p.w,
                height: p.h,
                content_w: 0,
                content_h: 0,
                dst_x: p.x,
                dst_y: p.y,
                opacity: 255,
                corner_radius: p.radius,
                scroll_id,
                shadow: (p.shadow_alpha > 0).then_some(LayerShadow {
                    blur: p.shadow_blur,
                    offset_y: p.shadow_offset_y,
                    alpha: p.shadow_alpha,
                }),
                backdrop: p.blur.map(|blur| LayerBackdrop {
                    blur_radius: DARK_GLASS_BLUR_RADIUS,
                    saturation_percent: DARK_GLASS_SATURATION_PERCENT,
                    restore_halo_pad: pad,
                    retained_src_y_offset: RETAINED_ROW_OFFSET,
                    cache: glass_cache(blur),
                }),
            },
            (mode_w, mode_h),
        );
        // Title bar: composited FIXED on top (src row 0) so it never scrolls. It
        // must be OPAQUE (see `draw_title_bar_row`) so it occludes the scrollable
        // body that covers the whole window underneath — otherwise scrolled rows
        // bleed through it. `header_h` is exactly the title bar (no translucent pad),
        // so content clips cleanly at the bar's bottom edge. Works on both the VMO
        // (mmio) and the layer (virgl) paths because occlusion is at the layer level.
        // Composited SQUARE (radius 0): the title bar's TOP corners are rounded at
        // the SURFACE level instead (transparent corner pixels, see
        // `draw_title_bar_row`), so the top corners reveal the body's rounded corners
        // underneath while the bottom edge stays straight — no all-corner "notch".
        let _ = encoder.try_composite_layer(p.atlas_row, p.atlas_x, p.w, header_h, p.x, p.y, 255, 0, 0, 0, 0, 0);
        // `built_blur`: the cache was (re)built this present iff a cache exists
        // and was invalid (no cache → nothing was built).
        p.blur.map(|b| !b.valid).unwrap_or(false)
    }
}

/// Map a window's blur-cache state to the layer SSOT's cache mode: build + write
/// the blurred backdrop on the first settled present, reuse it thereafter.
/// Parameters for one **material-tagged glass region** of a client surface (R1
/// layer seam). Unlike [`GlassCompositeParams`] (a whole window), this composites
/// an arbitrary sub-rect the app declared as glass, sampling its content from the
/// app surface's atlas band and blurring the retained backdrop behind it.
#[derive(Clone, Copy)]
pub(crate) struct MaterialLayerParams {
    pub src_row_abs: u32,
    pub src_x: u32,
    pub width: u32,
    pub height: u32,
    pub dst_x: u32,
    pub dst_y: u32,
    pub corner_radius: u32,
    pub shadow_alpha: u32,
    /// Backdrop blur radius (from the glass level: panel/card/subtle/window).
    pub blur_radius: u32,
}

/// Composite one app-declared glass region through the `nexus-gfx` layer SSOT —
/// the same recipe as [`ShellWindow::composite_glass`], per region, with the
/// backdrop re-blurred live each present (`BackdropCache::None`; a per-region
/// blur cache is a later optimization). This is how the shell's topbar/dock/cards
/// become real frosted layers over the wallpaper (RFC-0067 Revival R1).
pub(crate) fn composite_material_glass(
    encoder: &mut RenderCommandEncoder<'_>,
    p: MaterialLayerParams,
    mode_w: u32,
    mode_h: u32,
) {
    if p.width == 0 || p.height == 0 || p.dst_x >= mode_w || p.dst_y >= mode_h {
        return;
    }
    let w = p.width.min(mode_w.saturating_sub(p.dst_x));
    let h = p.height.min(mode_h.saturating_sub(p.dst_y));
    let _ = encoder.composite_layer_full(
        &Layer {
            src_row_abs: p.src_row_abs,
            src_x: p.src_x,
            width: w,
            height: h,
            content_w: 0,
            content_h: 0,
            dst_x: p.dst_x,
            dst_y: p.dst_y,
            opacity: 255,
            corner_radius: p.corner_radius,
            scroll_id: 0,
            shadow: (p.shadow_alpha > 0).then_some(LayerShadow {
                blur: 24,
                offset_y: 8,
                alpha: p.shadow_alpha,
            }),
            backdrop: Some(LayerBackdrop {
                blur_radius: p.blur_radius,
                saturation_percent: DARK_GLASS_SATURATION_PERCENT,
                restore_halo_pad: 0,
                retained_src_y_offset: RETAINED_ROW_OFFSET,
                cache: BackdropCache::None,
            }),
        },
        (mode_w, mode_h),
    );
}

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
    pub(crate) shadow_blur: u32,
    pub(crate) shadow_offset_y: i32,
    pub(crate) shadow_alpha: u32,
}
