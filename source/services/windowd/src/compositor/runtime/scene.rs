// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — `build_scene_cb_into` — the per-frame
//! GPU CommandBuffer builder (GPU-first layered scene). Post-cleanup
//! (cleanup-map DELETE): the scene is damage blits + the z-ordered window
//! layers (desktop base + floating app window) + dock + cursor. All shell
//! chrome (topbar/sidepanel/dropdown/buttons) and the legacy chat/search/
//! settings/greeter surfaces are DELETED — that UI is DSL-app content
//! composited through the desktop/app-window layers.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (behavior covered via windowd QEMU smoke + host integration)

use super::*;

/// Per-window scene snapshot (alloc-free `Copy` data captured before the
/// encoder block, RFC-0065 multi-window): what the composite loop needs to
/// draw one app window without re-borrowing the runtime.
#[derive(Clone, Copy)]
struct AppSceneSnap {
    glass: Option<crate::compositor::shell_window::GlassCompositeParams>,
    layers: [nexus_display_proto::client_surface::LayerDesc;
        nexus_display_proto::client_surface::MAX_SURFACE_LAYERS],
    layer_count: usize,
    /// (abs_row, x, w, h, dst_x, dst_y) of the live-resize title overlay.
    title_overlay: Option<(u32, u32, u32, u32, u32, u32)>,
    fullscreen: bool,
    /// (atlas_row, atlas_x, win_x, win_y, title_h) of the content band.
    layer_geom: Option<(u32, u32, u32, u32, u32)>,
    /// WebRender compositor-scroll (0 = non-scrollable, unchanged compose path).
    scroll_id: u32,
    /// The app's fixed top/bottom chrome heights + tall content + WM title bar
    /// height + current scroll offset — the 3-slice scrollable-glass params.
    header_h: u32,
    footer_h: u32,
    content_h: u32,
    scroll_rows: u32,
    title_h: u32,
}

impl Default for AppSceneSnap {
    fn default() -> Self {
        Self {
            glass: None,
            layers: [nexus_display_proto::client_surface::LayerDesc::default();
                nexus_display_proto::client_surface::MAX_SURFACE_LAYERS],
            layer_count: 0,
            title_overlay: None,
            fullscreen: false,
            layer_geom: None,
            scroll_id: 0,
            header_h: 0,
            footer_h: 0,
            content_h: 0,
            scroll_rows: 0,
            title_h: 0,
        }
    }
}

impl DisplayServerRuntime {
    /// Build the per-frame GPU CommandBuffer (GPU-first layout-tree model).
    ///
    /// CPU writes background content (wallpaper + proof panel) into Plane 1 only on
    /// content change. The GPU CB does all visual work per frame:
    ///   1. Blit each damage region: Plane 1 (retained, cursor-free) → Plane 2 (display).
    ///   2. Composite the z-ordered window layers (desktop base, app window).
    ///   3. Composite the dock (minimized windows), above the windows.
    ///   4. BlendCursor overlaid last.
    ///
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
        // App-client windows (ADR-0042 R1, multi-window): blit each app's
        // surface VMO when a present marked its window dirty.
        for idx in 0..self.apps.len() {
            if self.apps[idx].win.visible && self.apps[idx].win.surface_dirty {
                self.render_app_surface(idx)?;
                self.apps[idx].win.surface_dirty = false;
                // The bounded rows are consumed with the blit; the next
                // present re-seeds them (None = full, see ADR-0042 bounding).
                self.apps[idx].surface_dirty_rows = None;
            }
        }
        // Desktop surface (Umbau #17): blit the shell app-host's VMO into the
        // full-screen desktop band when a present marked it dirty.
        if self.desktop_dirty && self.desktop_band.is_some() {
            self.render_desktop_surface()?;
            self.desktop_dirty = false;
        }
        // Dock (TASK-0070 Phase 2): (re)render on membership change.
        if self.dock_dirty && self.dock_surface.is_some() {
            self.render_dock_surface()?;
            self.dock_dirty = false;
        }
        // Snapshot all `self` reads needed inside the encoder block so the
        // mutable borrow of `self.scene_cb` does not conflict with field reads.
        let mode = self.mode;
        let cursor_w = self.cursor_width;
        let cursor_h = self.cursor_height;
        let cursor_x = self.state.cursor_x;
        let cursor_y = self.state.cursor_y;
        let cursor_hot = self.cursor_hot;
        let hw_cursor = self.hw_cursor_active;
        // The fullscreen window (if any): its composite drops the rounded
        // corners + drop shadow (nothing to round/shadow against at the
        // display edges — straight edge-to-edge content, user decision).
        let fullscreen_id = self.windows.fullscreen_active();
        // Live resize (the "glass frame grows, content 1:1" path): while the app
        // window is actively edge-resized, the frame (app_win.w/h) grows past the
        // client's content band. Composite the backdrop blur + rounding at the
        // FRAME size but draw the content at the BAND size, top-left — the exposed
        // area is frosted glass. The client re-renders sharp at the new size on
        // release (end_window_resize), snapping back to the cached band path.
        // Per-window snapshots (alloc-free, Copy) so the encoder block below
        // never re-borrows `self`: glass params, material layer set, live-resize
        // overlay and band geometry for EVERY app window, indexed by slot.
        let mut app_snaps: [AppSceneSnap; crate::window_scene::MAX_APP_WINDOWS] =
            core::array::from_fn(|_| AppSceneSnap::default());
        for idx in 0..self.apps.len() {
            let wid = crate::window_scene::WindowId::App(idx as u8);
            let resizing = matches!(self.resize_drag, Some((w, ..)) if w == wid);
            let fullscreen = fullscreen_id == Some(wid);
            let win = &self.apps[idx].win;
            let glass = win.glass_params().map(|mut p| {
                if fullscreen {
                    p.radius = 0;
                    p.shadow_alpha = 0;
                }
                // Track C2: tag every slice with the window's transform id
                // (slot+1). The encode is ALWAYS the untransformed base -
                // the gpud override (delta translate / multiplied opacity /
                // center scale) SURVIVES full presents (no clear, no bake).
                // Baking multiplied WITH the override: an open baked at
                // opacity 0 stayed 0x-anything = invisible forever unless
                // unrelated presents happened to re-bake it brighter.
                p.layer_id = (idx as u32) + 1;
                if resizing {
                    if let Some(a) = win.atlas {
                        p.content_w = a.width.min(win.w);
                        p.content_h = a.height.min(win.h);
                        p.w = win.w;
                        p.h = win.h;
                    }
                }
                p
            });
            app_snaps[idx] = AppSceneSnap {
                glass,
                layers: self.apps[idx].layers,
                layer_count: self.apps[idx].layer_count,
                title_overlay: self.apps[idx].title_overlay.map(|s| {
                    (
                        s.abs_row,
                        s.x,
                        s.width.min(win.w),
                        win.title_h.min(s.height),
                        win.x.max(0) as u32,
                        win.y.max(0) as u32,
                    )
                }),
                fullscreen,
                layer_geom: win.atlas.map(|a| {
                    (a.abs_row, a.x, win.x.max(0) as u32, win.y.max(0) as u32, win.title_h)
                }),
                scroll_id: self.apps[idx].scroll_id,
                header_h: self.apps[idx].header_h,
                footer_h: self.apps[idx].footer_h,
                content_h: self.apps[idx].content_h,
                scroll_rows: self.apps[idx].scroll_rows,
                title_h: win.title_h,
            };
        }
        // Desktop base surface (declarative, Umbau #17): the shell/greeter
        // app-host that declared `level: desktop` owns its OWN full-screen band
        // (separate from the floating `app_win`). Snapshot its geometry for the
        // encoder block; composed as an opaque base layer at the bottom band.
        let desktop_layer = self.desktop_band.map(|b| {
            (b.abs_row, b.x, b.width.min(self.mode.width), b.height.min(self.mode.height))
        });
        // R1 seam for the DESKTOP surface: its material-tagged glass regions
        // (topbar/dock/panels) re-composite as frosted layers over the
        // wallpaper AFTER the base blend below.
        let desktop_glass = self.desktop_layers;
        let desktop_glass_count = self.desktop_layer_count;
        // Back-to-front window order from the z/focus stack (window_scene SSOT):
        // the composite loop below draws exactly these, in exactly this order.
        let (win_order, win_n) = self.windows.order(USE_DESKTOP_SHELL);
        // Dock layer params (bar rect is None while inactive/covered).
        let dock_layer = match (self.dock_bar_rect(), self.dock_surface) {
            (Some(bar), Some(surface)) => Some((surface.abs_row, surface.x, bar)),
            _ => None,
        };
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

            // 2. Windows, BACK-TO-FRONT in the z/focus stack's order
            //    (window_scene SSOT): the topmost window composites last and
            //    wins occlusion.
            for &wid in &win_order[..win_n] {
                match wid {
                    // App-client window (ADR-0042 R1). R1 layer seam: when the
                    // app declared material-tagged glass regions
                    // (`OP_SURFACE_LAYERS`), composite each as its own frosted
                    // `nexus-gfx` layer instead of one whole-window glass
                    // frame. No regions ⇒ the single-frame composite (a plain
                    // windowed app, unchanged).
                    crate::window_scene::WindowId::App(slot) => {
                        use nexus_display_proto::client_surface as wire;
                        let sn = &app_snaps[slot as usize];
                        // A scrollable surface owns the whole-window 3-slice
                        // composite — it takes priority over per-region material
                        // glass (the body's frosted backdrop covers the viewport).
                        if sn.scroll_id == 0 && sn.layer_count > 0 {
                            // WINDOW BODY FIRST (mirrors the Desktop arm's
                            // base-then-regions order): a windowed app with
                            // material regions still owns its whole frame —
                            // without this base composite only the glass
                            // regions painted and the rest of the window was
                            // MISSING (settings' window-frame page rendered
                            // as floating panels over the bare desktop). The
                            // regions then blur the drawn body = the inner
                            // frost the design intends.
                            if let Some(p) = sn.glass {
                                let _ =
                                    crate::compositor::shell_window::ShellWindow::composite_glass(
                                        &mut encoder,
                                        p,
                                        mode.width,
                                        mode.height,
                                    );
                            }
                            if let Some((atlas_row, atlas_x, win_x, win_y, title_h)) = sn.layer_geom
                            {
                                for l in sn.layers.iter().take(sn.layer_count) {
                                    if l.material != wire::MATERIAL_GLASS {
                                        continue;
                                    }
                                    let blur_radius = match l.glass_level {
                                        wire::GLASS_PANEL => 40,
                                        wire::GLASS_OVERLAY => 40,
                                        wire::GLASS_CARD => 20,
                                        wire::GLASS_SUBTLE => 8,
                                        _ => 30,
                                    };
                                    crate::compositor::shell_window::composite_material_glass(
                                        &mut encoder,
                                        crate::compositor::shell_window::MaterialLayerParams {
                                            src_row_abs: atlas_row + title_h + u32::from(l.y),
                                            src_x: atlas_x + u32::from(l.x),
                                            width: u32::from(l.w),
                                            height: u32::from(l.h),
                                            dst_x: win_x + u32::from(l.x),
                                            dst_y: win_y + title_h + u32::from(l.y),
                                            corner_radius: u32::from(l.radius),
                                            shadow_alpha: u32::from(l.shadow_alpha),
                                            blur_radius,
                                        },
                                        mode.width,
                                        mode.height,
                                    );
                                }
                            }
                        } else if sn.scroll_id != 0 {
                            // WebRender compositor-scroll: a 3-slice packed-band
                            // composite — the fixed top (WM title + app header)
                            // and fixed bottom (app footer) stay put while the
                            // body layer scrolls purely by a gpud `src_row` shift
                            // (`OP_SET_LAYER_SCROLL`). The body's full-present
                            // `src_row_abs` = `atlas_row + content_offset` MUST
                            // equal the override row windowd emits, so a full
                            // present mid-scroll agrees (no snap-to-top).
                            if let Some(p) = sn.glass {
                                let top_h = sn.title_h.saturating_add(sn.header_h);
                                let bot_h = sn.footer_h;
                                let content_offset =
                                    top_h.saturating_add(bot_h).saturating_add(sn.scroll_rows);
                                let _ =
                                    crate::compositor::shell_window::ShellWindow::composite_scrollable_glass(
                                        &mut encoder,
                                        p,
                                        sn.scroll_id,
                                        content_offset,
                                        top_h,
                                        bot_h,
                                        sn.content_h,
                                        mode.width,
                                        mode.height,
                                    );
                            }
                        } else if let Some(p) = sn.glass {
                            let _ = crate::compositor::shell_window::ShellWindow::composite_glass(
                                &mut encoder,
                                p,
                                mode.width,
                                mode.height,
                            );
                        }
                        // TASK #23: sharp frame-width title bar over the
                        // scaled band during a live resize / fullscreen
                        // transition (retired once the band catches up).
                        if let Some((row, sx, w, h, dx, dy)) = sn.title_overlay {
                            if w > 0 && h > 0 {
                                let _ = encoder.composite_layer_full(
                                    &Layer {
                                        corner_radius: if sn.fullscreen {
                                            0
                                        } else {
                                            crate::compositor::runtime::app_window::APP_WIN_RADIUS
                                        },
                                        content_epoch: crate::atlas::atlas_content_epoch(),
                                        ..Layer::opaque(row, sx, w, h, dx, dy)
                                    },
                                    (mode.width, mode.height),
                                );
                            }
                        }
                    }
                    // The desktop base surface (the shell / greeter app-host):
                    // composed as an OPAQUE full-screen layer at the bottom band
                    // via the nexus-gfx layer SSOT → gpud (like the wallpaper —
                    // no chrome, no rounded corners, no shadow, no backdrop blur;
                    // the shell's own frosted panels arrive as material-tagged
                    // regions, R1). This entry only appears in `win_order` when a
                    // client surface DECLARED `level: desktop` and was routed
                    // here (`app_stack_id`).
                    crate::window_scene::WindowId::Desktop => {
                        // FULL layer, every pass — deliberately NOT damage-
                        // clipped: the scene command buffer describes the WHOLE
                        // frame (the wallpaper plane persists on the GPU; every
                        // layer above it is re-composited per present). Clipping
                        // a layer to this pass's damage leaves wallpaper on top
                        // everywhere else ("UI only under the cursor rect").
                        // Damage economy lives in the OTHER stages: the band
                        // blit copies only the presented row span, and clients
                        // present paint-only spans — the composite itself is a
                        // band→display draw, not a client repaint.
                        if let Some((row, x, w, h)) = desktop_layer {
                            let _ = encoder.composite_layer_full(
                                &Layer {
                                    content_epoch: crate::atlas::atlas_content_epoch(),
                                    ..Layer::opaque(row, x, w, h, 0, 0)
                                },
                                (mode.width, mode.height),
                            );
                            // Frosted regions: blur the wallpaper behind each
                            // declared glass rect, then draw the band content
                            // (tint + text) over it — the liquid-glass look.
                            use nexus_display_proto::client_surface as wire;
                            for l in desktop_glass.iter().take(desktop_glass_count) {
                                if l.material != wire::MATERIAL_GLASS {
                                    continue;
                                }
                                // Shell chrome contract: PANEL-level glass
                                // (Control Center / notifications / calendar
                                // drop-downs) composites ABOVE the windows —
                                // drawn in pass 2b below, skipped here.
                                if l.glass_level == wire::GLASS_PANEL {
                                    continue;
                                }
                                let blur_radius = match l.glass_level {
                                    wire::GLASS_OVERLAY => 40,
                                    wire::GLASS_CARD => 20,
                                    wire::GLASS_SUBTLE => 8,
                                    _ => 30,
                                };
                                crate::compositor::shell_window::composite_material_glass(
                                    &mut encoder,
                                    crate::compositor::shell_window::MaterialLayerParams {
                                        src_row_abs: row + u32::from(l.y),
                                        src_x: x + u32::from(l.x),
                                        width: u32::from(l.w),
                                        height: u32::from(l.h),
                                        dst_x: u32::from(l.x),
                                        dst_y: u32::from(l.y),
                                        corner_radius: u32::from(l.radius),
                                        shadow_alpha: u32::from(l.shadow_alpha),
                                        blur_radius,
                                    },
                                    mode.width,
                                    mode.height,
                                );
                            }
                        }
                    } // Legacy window ids (chat/search/settings): their windowd
                      // surfaces are DELETED — the match is total over the
                      // remaining WindowId variants (surface-id roles).
                }
            }

            // 2b. SHELL CHROME ABOVE WINDOWS (user contract: windows sit
            //     BEHIND the shell top bar, which stays readable + usable):
            //     re-composite the desktop band's top-bar strip over every
            //     window (per-pixel alpha — only the pills/island pixels
            //     land), then the shell's PANEL-level glass drop-downs so an
            //     open Control Center overlays fullscreen windows too.
            if let Some((row, x, w, _h)) = desktop_layer {
                let bar_h = super::SHELL_TOPBAR_H.min(mode.height);
                let _ = encoder.composite_layer_full(
                    &Layer {
                        content_epoch: crate::atlas::atlas_content_epoch(),
                        ..Layer::opaque(row, x, w, bar_h, 0, 0)
                    },
                    (mode.width, mode.height),
                );
                use nexus_display_proto::client_surface as wire;
                for l in desktop_glass.iter().take(desktop_glass_count) {
                    if l.material != wire::MATERIAL_GLASS || l.glass_level != wire::GLASS_PANEL {
                        continue;
                    }
                    crate::compositor::shell_window::composite_material_glass(
                        &mut encoder,
                        crate::compositor::shell_window::MaterialLayerParams {
                            src_row_abs: row + u32::from(l.y),
                            src_x: x + u32::from(l.x),
                            width: u32::from(l.w),
                            height: u32::from(l.h),
                            dst_x: u32::from(l.x),
                            dst_y: u32::from(l.y),
                            corner_radius: u32::from(l.radius),
                            shadow_alpha: u32::from(l.shadow_alpha),
                            blur_radius: 40,
                        },
                        mode.width,
                        mode.height,
                    );
                }
            }

            // 3. Dock of minimized windows (TASK-0070 Phase 2): a glass bar
            //    bottom-center, present ONLY while ≥1 window is minimized and
            //    no fullscreen window covers the chrome. Above the windows.
            if let Some((dock_row, dock_x, bar)) = dock_layer {
                let _ = encoder.composite_layer_full(
                    &Layer {
                        corner_radius: crate::dock::DOCK_RADIUS,
                        shadow: Some(LayerShadow { blur: 14, offset_y: 4, alpha: 80 }),
                        backdrop: Some(chrome_glass_backdrop()),
                        content_epoch: crate::atlas::atlas_content_epoch(),
                        ..Layer::opaque(dock_row, dock_x, bar.width, bar.height, bar.x, bar.y)
                    },
                    (mode.width, mode.height),
                );
            }

            // 4. Cursor — composited last, never baked into any plane. Skipped
            //    entirely when the hardware cursor overlay is active (the host
            //    displays and moves the cursor; frames never carry it). In the
            //    software fallback a cursor-only move is a cheap cursor-region
            //    blit (from the retained Plane 1) + this BlendCursor.
            if !hw_cursor && cursor_w > 0 && cursor_h > 0 {
                let cx = (cursor_x - cursor_hot.0).max(0) as u32;
                let cy = (cursor_y - cursor_hot.1).max(0) as u32;
                if cx < mode.width && cy < mode.height {
                    let _ = encoder.try_blend_cursor(cx, cy, cursor_w, cursor_h);
                }
            }

            encoder.end_encoding();
        }
        self.scene_cb.serialize_into(out).map_err(|_| WindowdError::InvalidDamage)
    }
}

/// The frosted-glass backdrop shared by the compositor-drawn glass layers
/// (today: the dock): re-blur the live backdrop every frame (no cache — it
/// sits over changing content), no shadow halo. Restored from the retained
/// plane. Routed through the layer SSOT.
fn chrome_glass_backdrop() -> LayerBackdrop {
    LayerBackdrop {
        blur_radius: DARK_GLASS_BLUR_RADIUS,
        saturation_percent: DARK_GLASS_SATURATION_PERCENT,
        restore_halo_pad: 0,
        retained_src_y_offset: RETAINED_ROW_OFFSET,
        cache: BackdropCache::None,
    }
}
