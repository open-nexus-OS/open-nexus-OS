// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — `build_scene_cb_into` — the per-frame GPU CommandBuffer builder (GPU-first layered scene).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (behavior covered via windowd QEMU smoke + host integration)
//!
//! Split out of `runtime/mod.rs` (TASK-0063 modularization). A child module of
//! `runtime`, so these `impl DisplayServerRuntime` methods read the runtime's
//! private fields directly; previously-private methods are widened to
//! `pub(super)` so the parent and sibling submodules can still call them.

use super::*;

impl DisplayServerRuntime {
    /// Build the per-frame GPU CommandBuffer (GPU-first layout-tree model).
    ///
    /// CPU writes background content (wallpaper + proof panel) into Plane 1 only on
    /// content change. The GPU CB does all visual work per frame:
    ///   1. Blit each damage region: Plane 1 (retained, cursor-free) → Plane 2 (display).
    ///   2. Always blit + re-render the glass button (it's an animated overlay layer).
    ///   3. Blit + render the sidebar panel (GPU blur + rounded rect, animated translate/opacity).
    ///   4. BlendCursor overlaid last.
    ///
    /// Glass panels use BlurBackdrop (reads from Plane 2 after the blit, so it blurs
    /// the wallpaper/content behind the panel) + FillSdfRoundedRect (glass tint + border).
    /// Record the per-frame scene into the reusable `scene_cb` and serialize it
    /// into `out`. Returns the number of bytes written.
    ///
    /// Zero per-frame heap allocation: `scene_cb` is cleared (capacity retained)
    /// rather than freshly allocated, and serialization borrows it instead of
    /// consuming it into a `CommittedBuffer`. This is mandatory under windowd's
    /// non-freeing bump allocator — a per-frame `CommandBuffer::new()` would leak
    /// its `Vec<Command>` and crash the service mid-animation.
    pub(super) fn build_scene_cb_into(
        &mut self,
        rects: &[DamageRect],
        rect_count: usize,
        out: &mut [u8],
    ) -> Result<usize, WindowdError> {
        // Re-render the chat layer's cached surface (off-screen atlas) if its
        // content changed. Done before the encoder borrows `self.scene_cb`.
        if self.chat.surface_dirty {
            self.render_chat_surface()?;
            self.chat.surface_dirty = false;
        }
        // Shell-P2b: (re)render the glass topbar layer surface when dirty.
        if self.chrome_composited() && self.shell_surface_dirty {
            self.render_shell_surface()?;
            self.shell_surface_dirty = false;
        }
        // Shell-P2b: (re)render the glass side panel surface when dirty.
        if self.chrome_composited() && self.sidepanel_surface_dirty {
            self.render_sidepanel_surface()?;
            self.sidepanel_surface_dirty = false;
        }
        // Shell-P2b: (re)render the Apps dropdown surface when dirty.
        if self.chrome_composited() && self.dropdown_surface_dirty {
            self.render_dropdown_surface()?;
            self.dropdown_surface_dirty = false;
        }
        // Shell-P2b: (re)render the Search window surface when visible + dirty.
        if self.search.visible && self.search.surface_dirty {
            self.render_search_surface()?;
            self.search.surface_dirty = false;
        }
        // Snapshot all `self` reads needed inside the encoder block so the
        // mutable borrow of `self.scene_cb` does not conflict with field reads.
        let mode = self.mode;
        let scene = self.animated_scene;
        let cursor_w = self.cursor_width;
        let cursor_h = self.cursor_height;
        let cursor_x = self.state.cursor_x;
        let cursor_y = self.state.cursor_y;
        let hw_cursor = self.hw_cursor_active;
        let blur_cache_valid = self.sidebar_blur_cache_valid;
        // Pre-blur pass rides the first handoff present (full-screen damage, so
        // the display plane holds the complete base scene to blur from).
        let precache_sidebar_blur = !USE_DESKTOP_SHELL
            && self.precache_blur_pending
            && !blur_cache_valid
            && scene.sidebar_opacity <= 0.01;
        // Chat layer: source row of its cached surface + on-screen placement from
        // the window manager (so a drag just changes the blit destination).
        // Shell-P2b: the proof/shell chat atlas, sidebar, and glass buttons are
        // suppressed in desktop mode — the desktop chrome is composited into the
        // retained plane (step 1 blit) instead; chat/sidebar return as real
        // desktop layers in P3.
        let chat_atlas_row = self.chat_atlas_row();
        let shell_atlas_row = self.shell_atlas.abs_row;
        let shell_w = self.shell_w;
        let shell_h = self.shell_h;
        let sidepanel_atlas_row = self.sidepanel_atlas.abs_row;
        let sidepanel_h = self.sidepanel_h;
        // Slide: sidebar_translate_x animates SIDEBAR_WIDTH(closed) -> 0(open).
        let sidepanel_slide = scene.sidebar_translate_x;
        let sidepanel_opacity = scene.sidebar_opacity;
        let dropdown_atlas_row = self.dropdown_atlas.abs_row;
        let dropdown_full_h = self.dropdown_h;
        let dropdown_progress = scene.apps_dropdown_progress;
        // `Some` only while the Search window is mounted (shown) → composite it.
        let search_glass = self.search.glass_params();
        let mut built_search_blur = false;
        // Routed through the host-tested SSOT (window_scene) instead of an inline
        // `!USE_DESKTOP_SHELL && visible` — same decision, now covered by tests.
        let chat_show = crate::window_scene::should_show(self.chat.visible, USE_DESKTOP_SHELL);
        let chat_dx = self.chat.x.max(0) as u32;
        let chat_dy = self.chat.y.max(0) as u32;
        // GPU scroll-offset: the body samples the overscan surface shifted by the
        // scroll-within-window; the title bar is composited fixed on top.
        // HARDENING: clamp to [0, CHAT_OVERSCAN]. The surface is only
        // `CHAT_PANEL_H + CHAT_OVERSCAN` tall, so the composite samples rows
        // `[base+offset .. base+offset+CHAT_PANEL_H]`. If momentum ever advanced
        // the scroll past the prerendered window before the recenter re-render
        // landed, an unclamped offset would sample BEYOND the chat surface into
        // adjacent atlas rows (blur/sidebar caches) → garbage or out-of-bounds.
        // Clamping shows the window edge for one frame instead of corrupting.
        let chat_content_offset =
            self.chat_scroll_y.saturating_sub(self.chat_render_base).min(CHAT_OVERSCAN);
        // Fixed-header height for the scrollable composite = the title bar ONLY
        // (no translucent pad), so the opaque bar occludes scrolled rows and content
        // clips right at its bottom edge — not partway down a translucent strip.
        let chat_title_h = crate::interaction::CHAT_TITLE_BAR_H;
        let chat_blur_cache_row = self.chat.blur_cache.map(|s| s.abs_row).unwrap_or(0);
        let chat_blur_cache_valid = self.chat.blur_valid;
        let mut built_chat_blur_cache = false;
        let btn_blur_cache_valid = self.button_blur_cache_valid;
        let mut built_button_cache = false;
        // Sidebar composite cache: usable only when the sidebar is fully open and
        // static (settled). During the slide it's redrawn each frame (animation).
        let sidebar_settled = scene.sidebar_opacity >= 0.99 && scene.sidebar_translate_x <= 0.5;
        let sidebar_composite_cache_row = self.sidebar_composite_cache.abs_row;
        let sidebar_composite_cache_valid = self.sidebar_composite_cache_valid;
        let mut built_sidebar_composite_cache = false;

        // Incremental overlays: a static glass overlay (hamburger, chat button,
        // sidebar) only needs re-rendering when a damage rect actually overwrote its
        // region — step 1 above blits ONLY the damage rects, so an untouched overlay
        // persists on the display plane. Every interaction that changes an overlay
        // queues that overlay's rect (note_button_hover_changed, sidebar open/slide,
        // chat-visibility toggle), so "region touched" is the exact, complete redraw
        // condition. This keeps a far-away hover/card change off the glass GPU work
        // (the per-present cost that made the UI feel unresponsive once the cursor was
        // decoupled to the HW overlay).
        let overlaps = |x0: i32, y0: i32, x1: i32, y1: i32| -> bool {
            rects.iter().take(rect_count).any(|r| {
                let rx1 = (r.x + r.width) as i32;
                let ry1 = (r.y + r.height) as i32;
                (r.x as i32) < x1 && rx1 > x0 && (r.y as i32) < y1 && ry1 > y0
            })
        };
        let hb = crate::interaction::button_rect(mode.width);
        let button_touched =
            overlaps(hb.x as i32, hb.y as i32, (hb.x + hb.width) as i32, (hb.y + hb.height) as i32);
        let cbtn = crate::interaction::chat_button_rect(mode.width, mode.height);
        let chat_btn_touched = overlaps(
            cbtn.x as i32,
            cbtn.y as i32,
            (cbtn.x + cbtn.width) as i32,
            (cbtn.y + cbtn.height) as i32,
        );
        let sidebar_touched = {
            let sx = mode
                .width
                .saturating_sub(SIDEBAR_WIDTH)
                .saturating_add(scene.sidebar_translate_x.clamp(0.0, SIDEBAR_WIDTH as f32) as u32);
            overlaps(sx as i32, 0, mode.width as i32, mode.height as i32)
        };

        // Chrome-composite decision hoisted before the encoder borrows
        // `self.scene_cb` (a method call inside would re-borrow all of self).
        let chrome_composited = self.chrome_composited();
        let greeter_active = self.greeter_active();
        self.scene_cb.clear();
        {
            let mut encoder = self
                .scene_cb
                .try_begin_render_pass(RenderPassDesc {
                    color_attachments: alloc::vec![],
                    width: mode.width,
                    height: mode.height,
                })
                .map_err(|_| WindowdError::InvalidDamage)?;

            // 1. Blit content damage from retained plane → display plane.
            for rect in rects.iter().copied().take(rect_count) {
                encoder
                    .try_blit_surface(
                        rect.x,
                        rect.y + RETAINED_ROW_OFFSET,
                        rect.x,
                        rect.y,
                        rect.width,
                        rect.height,
                    )
                    .map_err(|_| WindowdError::InvalidDamage)?;
            }

            // 1·shell. Glass topbar layer (Shell-P2b): composite the topbar atlas
            //     onto the display each present with backdrop blur + rounded
            //     corners + a soft drop shadow — the GPU layer path that reaches
            //     the virgl scanout (the retained Plane 1 does not). Rendered like
            //     the chat window: translucent tint + opaque text in the atlas,
            //     glass effects applied here by the composite.
            if chrome_composited && shell_w > 0 && shell_h > 0 {
                use crate::compositor::desktop_layer::{TOPBAR_MARGIN_X, TOPBAR_RADIUS, TOPBAR_TOP};
                // Proven glass recipe (same as the glass buttons): restore the
                // clean backdrop from the retained plane, blur it in place, THEN
                // composite the topbar atlas (translucent tint + crisp text) on
                // top with backdrop_blur=0. Passing backdrop_blur to the composite
                // smears the layer content (text) into a gray blob — this keeps
                // the text sharp over a frosted backdrop.
                let _ = encoder.composite_layer_full(
                    &Layer {
                        corner_radius: TOPBAR_RADIUS,
                        shadow: Some(LayerShadow { blur: 10, offset_y: 3, alpha: 60 }),
                        backdrop: Some(chrome_glass_backdrop()),
                        ..Layer::opaque(shell_atlas_row, 0, shell_w, shell_h, TOPBAR_MARGIN_X, TOPBAR_TOP)
                    },
                    (mode.width, mode.height),
                );
            }

            // 1·dropdown. Apps dropdown — a small glass menu under the topbar
            //     "Apps" item, revealed (roll-down + fade) by the dropdown spring.
            if chrome_composited && dropdown_progress > 0.01 {
                use crate::compositor::desktop_layer::{
                    apps_item_x, DROPDOWN_RADIUS, DROPDOWN_W, TOPBAR_MARGIN_X, TOPBAR_TOP, TOPBAR_H,
                };
                let dx = TOPBAR_MARGIN_X + apps_item_x();
                let dy = TOPBAR_TOP + TOPBAR_H + 4;
                let reveal_h = ((dropdown_progress.clamp(0.0, 1.0) * dropdown_full_h as f32) as u32).max(1);
                let alpha = (dropdown_progress.clamp(0.0, 1.0) * 255.0) as u32;
                if dx < mode.width && dy < mode.height {
                    let w = DROPDOWN_W.min(mode.width.saturating_sub(dx));
                    let h = reveal_h.min(mode.height.saturating_sub(dy));
                    let _ = encoder.composite_layer_full(
                        &Layer {
                            opacity: alpha,
                            corner_radius: DROPDOWN_RADIUS,
                            shadow: Some(LayerShadow { blur: 14, offset_y: 4, alpha: 80 }),
                            backdrop: Some(chrome_glass_backdrop()),
                            ..Layer::opaque(dropdown_atlas_row, 0, w, h, dx, dy)
                        },
                        (mode.width, mode.height),
                    );
                }
            }

            // 1·panel. Glass side panel — slides in from the right, driven by the
            //     sidebar spring. Same proven recipe as the topbar: restore +
            //     pre-blur the panel's current rect, then composite the atlas with
            //     rounded corners + drop shadow on top (backdrop_blur=0).
            if chrome_composited && sidepanel_opacity > 0.01 {
                use crate::compositor::desktop_layer::{
                    SIDEPANEL_MARGIN, SIDEPANEL_RADIUS, SIDEPANEL_TOP, SIDEPANEL_W,
                };
                let base_x = mode
                    .width
                    .saturating_sub(SIDEPANEL_MARGIN + SIDEPANEL_W)
                    .saturating_add(sidepanel_slide.clamp(0.0, SIDEPANEL_W as f32 + 32.0) as u32);
                if base_x < mode.width {
                    let w = SIDEPANEL_W.min(mode.width.saturating_sub(base_x));
                    let alpha = (sidepanel_opacity.clamp(0.0, 1.0) * 255.0) as u32;
                    let _ = encoder.composite_layer_full(
                        &Layer {
                            opacity: alpha,
                            corner_radius: SIDEPANEL_RADIUS,
                            shadow: Some(LayerShadow { blur: 16, offset_y: 4, alpha: 80 }),
                            backdrop: Some(chrome_glass_backdrop()),
                            ..Layer::opaque(sidepanel_atlas_row, 0, w, sidepanel_h, base_x, SIDEPANEL_TOP)
                        },
                        (mode.width, mode.height),
                    );
                }
            }

            // 1·search. Movable Search window — the reusable ShellWindow glass
            //     frame (rounded + cached blur + shadow); filterable word list
            //     inside. The glass recipe is shared with the chat window (W3).
            if let Some(p) = search_glass {
                built_search_blur = crate::compositor::shell_window::ShellWindow::composite_glass(
                    &mut encoder,
                    p,
                    mode.width,
                    mode.height,
                );
            }

            // 1a. Chat window — the SAME reusable ShellWindow glass frame as the
            //     Search window (rounded corners + cached blur + drop shadow +
            //     the shared title bar/close "x"), with the body scrolled by a
            //     GPU source-row offset (render once, no per-frame re-render).
            //     On virgl the scanout is rebuilt every present, so the chat layer
            //     is re-composited each frame (the mmio damage-touch gate would
            //     otherwise show the window only when the cursor passed over it).
            if chat_show {
                use crate::interaction::{CHAT_PANEL_H, CHAT_PANEL_W};
                let chat_glass = crate::compositor::shell_window::GlassCompositeParams {
                    atlas_row: chat_atlas_row,
                    atlas_x: 0, // chat is a full-width band
                    blur_cache_row: chat_blur_cache_row,
                    blur_cache_x: 0,
                    blur_valid: chat_blur_cache_valid,
                    x: chat_dx,
                    y: chat_dy,
                    w: CHAT_PANEL_W,
                    h: CHAT_PANEL_H,
                    radius: super::desktop_layer::SEARCH_RADIUS,
                    shadow_blur: CHAT_SHADOW_BLUR,
                    shadow_offset_y: CHAT_SHADOW_OFFSET_Y,
                    shadow_alpha: CHAT_SHADOW_ALPHA as u32,
                };
                built_chat_blur_cache =
                    crate::compositor::shell_window::ShellWindow::composite_scrollable_glass(
                        &mut encoder,
                        chat_glass,
                        chat_content_offset,
                        chat_title_h,
                        mode.width,
                        mode.height,
                    );
            }

            // 1b. Pre-blur the sidebar backdrop at handoff (sidebar closed,
            //     before any overlay is drawn — the display plane equals the
            //     clean Plane 1 base here): blur the rest-position strip, save
            //     it to the Plane 3 cache, restore the unblurred content from
            //     Plane 1. One-time cost — the first sidebar open (and every
            //     slide frame) is then a pure cache blit, zero blur work.
            if precache_sidebar_blur {
                let sidebar_h =
                    mode.height.saturating_sub(SIDEBAR_MARGIN_TOP + SIDEBAR_MARGIN_BOTTOM).max(1);
                let rest = TileRect {
                    x: SIDEBAR_REST_X,
                    y: SIDEBAR_MARGIN_TOP,
                    width: SIDEBAR_WIDTH,
                    height: sidebar_h,
                };
                let _ = encoder.try_blur_backdrop(rest, 20, DARK_GLASS_SATURATION_PERCENT);
                let _ = encoder.try_blit_absolute(
                    SIDEBAR_REST_X,
                    DISPLAY_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                    SIDEBAR_REST_X,
                    BLUR_CACHE_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                    SIDEBAR_WIDTH,
                    sidebar_h,
                );
                let _ = encoder.try_blit_surface(
                    SIDEBAR_REST_X,
                    SIDEBAR_MARGIN_TOP + RETAINED_ROW_OFFSET,
                    SIDEBAR_REST_X,
                    SIDEBAR_MARGIN_TOP,
                    SIDEBAR_WIDTH,
                    sidebar_h,
                );
            }

            // 2. Glass button — cached blur, skipped when sidebar covers it.
            let button_x = mode.width.saturating_sub(GLASS_BUTTON_W + GLASS_BUTTON_RIGHT);
            let button_blit_w = GLASS_BUTTON_W.min(mode.width.saturating_sub(button_x));
            let sidebar_x_for_btn = mode
                .width
                .saturating_sub(SIDEBAR_WIDTH)
                .saturating_add(scene.sidebar_translate_x.clamp(0.0, SIDEBAR_WIDTH as f32) as u32);
            let button_covered = scene.sidebar_opacity > 0.01 && sidebar_x_for_btn <= button_x;
            // Incremental: only redraw the glass button when a damage rect overwrote
            // its region (hover spring / handoff / cache build all queue the button
            // rect). A far-away change leaves the button untouched on the display plane.
            // The glass topbar carries the menu icon now, so the standalone
            // hamburger button (which would overlap the topbar) is suppressed.
            if !USE_DESKTOP_SHELL
                && !self.shell_config.desktop_chrome
                && !greeter_active
                && button_blit_w > 0
                && !button_covered
                && (button_touched || !btn_blur_cache_valid)
            {
                if btn_blur_cache_valid {
                    // Fast path: restore pre-blurred background from Plane 3 cache.
                    let _ = encoder.try_blit_absolute(
                        BUTTON_BLUR_CACHE_ABS_X,
                        BUTTON_BLUR_CACHE_ABS_ROW,
                        button_x,
                        DISPLAY_ROW_OFFSET + GLASS_BUTTON_TOP,
                        button_blit_w,
                        GLASS_BUTTON_H,
                    );
                } else {
                    // Cache-build path: blit P1, blur in-place, save to Plane 3.
                    let _ = encoder.try_blit_surface(
                        button_x,
                        GLASS_BUTTON_TOP + RETAINED_ROW_OFFSET,
                        button_x,
                        GLASS_BUTTON_TOP,
                        button_blit_w,
                        GLASS_BUTTON_H,
                    );
                    let btn_build_rect = TileRect {
                        x: button_x,
                        y: GLASS_BUTTON_TOP,
                        width: button_blit_w,
                        height: GLASS_BUTTON_H,
                    };
                    let _ = encoder.try_blur_backdrop(
                        btn_build_rect,
                        DARK_GLASS_BLUR_RADIUS,
                        DARK_GLASS_SATURATION_PERCENT,
                    );
                    let _ = encoder.try_blit_absolute(
                        button_x,
                        DISPLAY_ROW_OFFSET + GLASS_BUTTON_TOP,
                        BUTTON_BLUR_CACHE_ABS_X,
                        BUTTON_BLUR_CACHE_ABS_ROW,
                        button_blit_w,
                        GLASS_BUTTON_H,
                    );
                    built_button_cache = true;
                }
                let btn_rect = TileRect {
                    x: button_x,
                    y: GLASS_BUTTON_TOP,
                    width: button_blit_w,
                    height: GLASS_BUTTON_H,
                };
                let button_alpha = (96.0 + 80.0 * scene.hover_opacity).clamp(96.0, 220.0) as u8;
                let gt = crate::assets::GLASS_TINT;
                let ge = crate::assets::GLASS_EDGE;
                // Glass body as a vertical gradient (light falls from above) —
                // GPU per-pixel via the SDF shader, CPU per-row fallback.
                let _ = encoder.try_fill_sdf_gradient(
                    btn_rect,
                    GLASS_BUTTON_RADIUS,
                    RgbaColor::new(
                        gt.r.saturating_add(18),
                        gt.g.saturating_add(18),
                        gt.b.saturating_add(18),
                        button_alpha,
                    ),
                    RgbaColor::new(
                        gt.r.saturating_sub(8),
                        gt.g.saturating_sub(8),
                        gt.b.saturating_sub(8),
                        button_alpha,
                    ),
                );
                let _ = encoder.try_fill_sdf_rounded_rect(
                    btn_rect,
                    GLASS_BUTTON_RADIUS,
                    RgbaColor::new(ge.r, ge.g, ge.b, ge.a),
                );
                // Hamburger icon: 3 horizontal bars centered inside the glass button.
                const MENU_BAR_W: u32 = 18;
                const MENU_BAR_H: u32 = 3;
                const MENU_BAR_GAP: u32 = 5;
                const MENU_TOTAL_H: u32 = 3 * MENU_BAR_H + 2 * MENU_BAR_GAP;
                let bar_x = button_x.saturating_add(GLASS_BUTTON_W.saturating_sub(MENU_BAR_W) / 2);
                let bar_y = GLASS_BUTTON_TOP
                    .saturating_add(GLASS_BUTTON_H.saturating_sub(MENU_TOTAL_H) / 2);
                let icon_alpha = (160.0 + 80.0 * scene.hover_opacity).clamp(160.0, 240.0) as u8;
                let bar_color = RgbaColor::new(255, 255, 255, icon_alpha);
                let _ = encoder.try_fill_sdf_rounded_rect(
                    TileRect { x: bar_x, y: bar_y, width: MENU_BAR_W, height: MENU_BAR_H },
                    1,
                    bar_color,
                );
                let _ = encoder.try_fill_sdf_rounded_rect(
                    TileRect {
                        x: bar_x,
                        y: bar_y + MENU_BAR_H + MENU_BAR_GAP,
                        width: MENU_BAR_W,
                        height: MENU_BAR_H,
                    },
                    1,
                    bar_color,
                );
                let _ = encoder.try_fill_sdf_rounded_rect(
                    TileRect {
                        x: bar_x,
                        y: bar_y + 2 * (MENU_BAR_H + MENU_BAR_GAP),
                        width: MENU_BAR_W,
                        height: MENU_BAR_H,
                    },
                    1,
                    bar_color,
                );
            }

            // 2b. Chat toggle button — square glass button under the hamburger
            //     (P7). Same cover rule as the hamburger: hidden while the
            //     sidebar overlaps it. Speech-bubble glyph: rounded outline +
            //     three dots.
            {
                use crate::interaction::{chat_button_rect, CHAT_BUTTON_RADIUS};
                let cb = chat_button_rect(mode.width, mode.height);
                let covered = scene.sidebar_opacity > 0.01 && sidebar_x_for_btn <= cb.x;
                // Incremental: only redraw when its region was overwritten (chat-visibility
                // toggle queues the chat-button rect; handoff damages full screen).
                if !USE_DESKTOP_SHELL && cb.width > 0 && !covered && chat_btn_touched {
                    let gt = crate::assets::GLASS_TINT;
                    let ge = crate::assets::GLASS_EDGE;
                    let cb_rect = TileRect { x: cb.x, y: cb.y, width: cb.width, height: cb.height };
                    // Restore the clean base from Plane 1 first — the glass
                    // fills are translucent and would accumulate over the
                    // previous frame's button pixels otherwise.
                    let _ = encoder.try_blit_surface(
                        cb.x,
                        cb.y + RETAINED_ROW_OFFSET,
                        cb.x,
                        cb.y,
                        cb.width,
                        cb.height,
                    );
                    let chat_open = self.chat.visible;
                    // Slightly brighter while the chat window is open (active state).
                    let body_alpha: u8 = if chat_open { 200 } else { 128 };
                    let _ = encoder.try_fill_sdf_gradient(
                        cb_rect,
                        CHAT_BUTTON_RADIUS,
                        RgbaColor::new(
                            gt.r.saturating_add(18),
                            gt.g.saturating_add(18),
                            gt.b.saturating_add(18),
                            body_alpha,
                        ),
                        RgbaColor::new(
                            gt.r.saturating_sub(8),
                            gt.g.saturating_sub(8),
                            gt.b.saturating_sub(8),
                            body_alpha,
                        ),
                    );
                    let _ = encoder.try_fill_sdf_rounded_rect(
                        cb_rect,
                        CHAT_BUTTON_RADIUS,
                        RgbaColor::new(ge.r, ge.g, ge.b, ge.a),
                    );
                    // Speech bubble: a rounded rect with three dots.
                    const BUBBLE_W: u32 = 26;
                    const BUBBLE_H: u32 = 18;
                    let bx = cb.x + (cb.width - BUBBLE_W) / 2;
                    let by = cb.y + (cb.height - BUBBLE_H) / 2;
                    let icon = RgbaColor::new(255, 255, 255, 220);
                    let _ = encoder.try_fill_sdf_rounded_rect(
                        TileRect { x: bx, y: by, width: BUBBLE_W, height: BUBBLE_H },
                        6,
                        icon,
                    );
                    let dot = RgbaColor::new(
                        gt.r.saturating_sub(8),
                        gt.g.saturating_sub(8),
                        gt.b.saturating_sub(8),
                        255,
                    );
                    for i in 0..3u32 {
                        let _ = encoder.try_fill_sdf_rounded_rect(
                            TileRect {
                                x: bx + 5 + i * 6,
                                y: by + BUBBLE_H / 2 - 1,
                                width: 3,
                                height: 3,
                            },
                            1,
                            dot,
                        );
                    }
                }
            }

            // 3. Sidebar panel — GPU overlay, only when visible (opacity > 0).
            //    Blur caching: compute once per open into Plane 3 (Slot B, rows 2400+),
            //    then blit from cache each animation frame instead of re-blurring.
            //    The wallpaper behind the sidebar is static so the blur is identical
            //    every frame. Cache spans the full 320px at SIDEBAR_REST_X=960 so all
            //    visible sub-strips during the slide animation are covered.
            let sidebar_opacity = scene.sidebar_opacity;
            // Incremental: redraw only when sliding/opening (animation queues the
            // sidebar rect each tick), when a damage rect overwrote it, or while a
            // blur/composite cache still needs building. A settled, cached, untouched
            // sidebar persists on the display plane — no per-present blur/SDF work.
            if !USE_DESKTOP_SHELL
                && !self.shell_config.desktop_chrome
                && !greeter_active
                && sidebar_opacity > 0.01
                && (sidebar_touched || !blur_cache_valid || !sidebar_composite_cache_valid)
            {
                let translate = scene.sidebar_translate_x.clamp(0.0, SIDEBAR_WIDTH as f32) as u32;
                let sidebar_x = mode.width.saturating_sub(SIDEBAR_WIDTH).saturating_add(translate);
                if sidebar_x < mode.width {
                    let sidebar_w = SIDEBAR_WIDTH.min(mode.width.saturating_sub(sidebar_x));
                    let sidebar_h = mode
                        .height
                        .saturating_sub(SIDEBAR_MARGIN_TOP + SIDEBAR_MARGIN_BOTTOM)
                        .max(1);

                    // Fast path: the sidebar is settled and already composited
                    // into the cache — one blit, skip the blur-cache + SDF fills.
                    if sidebar_settled && sidebar_composite_cache_valid {
                        let _ = encoder.try_blit_absolute(
                            sidebar_x,
                            sidebar_composite_cache_row + SIDEBAR_MARGIN_TOP,
                            sidebar_x,
                            DISPLAY_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                            sidebar_w,
                            sidebar_h,
                        );
                    } else {
                        if !blur_cache_valid {
                            // Cache-build frame (once per sidebar open):
                            // restore full Plane 1 bg at rest position, blur it, save to Plane 3.
                            let _ = encoder.try_blit_surface(
                                SIDEBAR_REST_X,
                                SIDEBAR_MARGIN_TOP + RETAINED_ROW_OFFSET,
                                SIDEBAR_REST_X,
                                SIDEBAR_MARGIN_TOP,
                                SIDEBAR_WIDTH,
                                sidebar_h,
                            );
                            let full_sbr = TileRect {
                                x: SIDEBAR_REST_X,
                                y: SIDEBAR_MARGIN_TOP,
                                width: SIDEBAR_WIDTH,
                                height: sidebar_h,
                            };
                            let _ = encoder.try_blur_backdrop(
                                full_sbr,
                                20,
                                DARK_GLASS_SATURATION_PERCENT,
                            );
                            // Save blurred display pixels to Plane 3 cache.
                            let _ = encoder.try_blit_absolute(
                                SIDEBAR_REST_X,
                                DISPLAY_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                                SIDEBAR_REST_X,
                                BLUR_CACHE_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                                SIDEBAR_WIDTH,
                                sidebar_h,
                            );
                            // Blit the currently-visible strip from cache for this frame.
                            let _ = encoder.try_blit_absolute(
                                sidebar_x,
                                BLUR_CACHE_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                                sidebar_x,
                                DISPLAY_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                                sidebar_w,
                                sidebar_h,
                            );
                        } else {
                            // Cache-use frame: blit pre-blurred strip from Plane 3 — no blur.
                            let _ = encoder.try_blit_absolute(
                                sidebar_x,
                                BLUR_CACHE_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                                sidebar_x,
                                DISPLAY_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                                sidebar_w,
                                sidebar_h,
                            );
                        }

                        let sbr = TileRect {
                            x: sidebar_x,
                            y: SIDEBAR_MARGIN_TOP,
                            width: sidebar_w,
                            height: sidebar_h,
                        };
                        // Translucent enough that the blurred backdrop reads as
                        // glass (220 was nearly opaque → looked flat gray).
                        let sidebar_alpha = (150.0 * sidebar_opacity).clamp(0.0, 150.0) as u8;
                        let border_alpha = (130.0 * sidebar_opacity).clamp(0.0, 130.0) as u8;
                        let gt = crate::assets::GLASS_TINT;
                        let ge = crate::assets::GLASS_EDGE;
                        let pb = crate::assets::PROOF_PANEL_BORDER;
                        // Border: fill outer rect with border color, then cover interior with glass fill.
                        let _ = encoder.try_fill_sdf_rounded_rect(
                            sbr,
                            SIDEBAR_RADIUS,
                            RgbaColor::new(pb.r, pb.g, pb.b, border_alpha),
                        );
                        if sidebar_w > 2 && sidebar_h > 2 {
                            let sbr_inner = TileRect {
                                x: sbr.x + 1,
                                y: sbr.y + 1,
                                width: sbr.width - 2,
                                height: sbr.height - 2,
                            };
                            // Glass body as a vertical gradient (light from above).
                            let _ = encoder.try_fill_sdf_gradient(
                                sbr_inner,
                                SIDEBAR_RADIUS.saturating_sub(1),
                                RgbaColor::new(
                                    gt.r.saturating_add(14),
                                    gt.g.saturating_add(14),
                                    gt.b.saturating_add(14),
                                    sidebar_alpha,
                                ),
                                RgbaColor::new(
                                    gt.r.saturating_sub(6),
                                    gt.g.saturating_sub(6),
                                    gt.b.saturating_sub(6),
                                    sidebar_alpha,
                                ),
                            );
                            let _ = encoder.try_fill_sdf_rounded_rect(
                                sbr_inner,
                                SIDEBAR_RADIUS.saturating_sub(1),
                                RgbaColor::new(ge.r, ge.g, ge.b, ge.a),
                            );
                        }
                        // Close icon (× approximated as + shape) at top-right of sidebar.
                        const CLOSE_SIZE: u32 = 16;
                        const CLOSE_BAR: u32 = 3;
                        const CLOSE_INSET: u32 = 16;
                        if sidebar_w > CLOSE_SIZE + CLOSE_INSET {
                            let cx = sidebar_x
                                .saturating_add(sidebar_w.saturating_sub(CLOSE_SIZE + CLOSE_INSET));
                            let cy = SIDEBAR_MARGIN_TOP.saturating_add(CLOSE_INSET);
                            let close_alpha = (200.0 * sidebar_opacity).clamp(0.0, 220.0) as u8;
                            let cc = RgbaColor::new(255, 255, 255, close_alpha);
                            let _ = encoder.try_fill_sdf_rounded_rect(
                                TileRect {
                                    x: cx,
                                    y: cy + (CLOSE_SIZE - CLOSE_BAR) / 2,
                                    width: CLOSE_SIZE,
                                    height: CLOSE_BAR,
                                },
                                1,
                                cc,
                            );
                            let _ = encoder.try_fill_sdf_rounded_rect(
                                TileRect {
                                    x: cx + (CLOSE_SIZE - CLOSE_BAR) / 2,
                                    y: cy,
                                    width: CLOSE_BAR,
                                    height: CLOSE_SIZE,
                                },
                                1,
                                cc,
                            );
                        }
                        // Snapshot the fully composited sidebar into the cache on the
                        // first settled frame; subsequent presents are a single blit.
                        if sidebar_settled {
                            let _ = encoder.try_blit_absolute(
                                sidebar_x,
                                DISPLAY_ROW_OFFSET + SIDEBAR_MARGIN_TOP,
                                sidebar_x,
                                sidebar_composite_cache_row + SIDEBAR_MARGIN_TOP,
                                sidebar_w,
                                sidebar_h,
                            );
                            built_sidebar_composite_cache = true;
                        }
                    }
                }
            }

            // 3·greeter. The login greeter (TASK-0065B) — ONE full-screen
            //     opaque layer sourcing its atlas band (blurred wallpaper +
            //     avatar card). Emitted in EVERY frame while active: the same
            //     CompositeLayer primitive as chrome/windows, so it renders on
            //     both backends (virgl retains/replaces its RT layer set —
            //     this frame carrying a layer is what evicts the chrome; mmio
            //     blends it over the base each present). Above everything but
            //     the cursor.
            if let Some(greeter_row) = self.greeter.as_ref().map(|g| g.surface.abs_row) {
                let _ = encoder.composite_layer_full(
                    &Layer::opaque(greeter_row, 0, mode.width, mode.height, 0, 0),
                    (mode.width, mode.height),
                );
            }

            // 4. Cursor — composited last, never baked into any plane. Skipped
            //    entirely when the hardware cursor overlay is active (the host
            //    displays and moves the cursor; frames never carry it). In the
            //    software fallback a cursor-only move is a cheap cursor-region
            //    blit (from the retained Plane 1) + this BlendCursor.
            if !hw_cursor && cursor_w > 0 && cursor_h > 0 {
                let cx = (cursor_x - crate::assets::CURSOR_HOTSPOT_X).max(0) as u32;
                let cy = (cursor_y - crate::assets::CURSOR_HOTSPOT_Y).max(0) as u32;
                if cx < mode.width && cy < mode.height {
                    let _ = encoder.try_blend_cursor(cx, cy, cursor_w, cursor_h);
                }
            }

            encoder.end_encoding();
        }
        // Commit cache-build results so subsequent frames use the caches.
        if precache_sidebar_blur {
            self.sidebar_blur_cache_valid = true;
            self.precache_blur_pending = false;
        }
        if !blur_cache_valid && scene.sidebar_opacity > 0.01 {
            self.sidebar_blur_cache_valid = true;
        }
        if built_button_cache {
            self.button_blur_cache_valid = true;
        }
        if built_chat_blur_cache {
            self.chat.blur_valid = true;
        }
        if built_search_blur {
            self.search.blur_valid = true;
        }
        // Sidebar composite cache: valid once built on a settled frame; dropped
        // whenever the sidebar is animating (slide/fade) so the animation draws
        // fresh frames and the cache is rebuilt when it settles again.
        if built_sidebar_composite_cache {
            self.sidebar_composite_cache_valid = true;
        } else if !sidebar_settled {
            self.sidebar_composite_cache_valid = false;
        }
        self.scene_cb.serialize_into(out).map_err(|_| WindowdError::InvalidDamage)
    }
}

/// The frosted-glass backdrop shared by the desktop chrome panels (topbar, side
/// panel, dropdown): re-blur the live backdrop every frame (no cache — they
/// animate or sit over changing content), no shadow halo. Restored from the
/// retained plane. Routed through the layer SSOT.
fn chrome_glass_backdrop() -> LayerBackdrop {
    LayerBackdrop {
        blur_radius: DARK_GLASS_BLUR_RADIUS,
        saturation_percent: DARK_GLASS_SATURATION_PERCENT,
        restore_halo_pad: 0,
        retained_src_y_offset: RETAINED_ROW_OFFSET,
        cache: BackdropCache::None,
    }
}
