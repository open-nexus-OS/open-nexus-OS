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
        // App-client window (ADR-0042 R1): blit the app's surface VMO when
        // a present marked it dirty.
        if self.app_win.visible && self.app_win.surface_dirty {
            self.render_app_surface()?;
            self.app_win.surface_dirty = false;
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
        let app_resizing =
            matches!(self.resize_drag, Some((crate::window_scene::WindowId::AppClient, ..)));
        let app_glass = self.app_win.glass_params().map(|mut p| {
            if fullscreen_id == Some(crate::window_scene::WindowId::AppClient) {
                p.radius = 0;
                p.shadow_alpha = 0;
            }
            if app_resizing {
                if let Some(a) = self.app_win.atlas {
                    p.content_w = a.width.min(self.app_win.w);
                    p.content_h = a.height.min(self.app_win.h);
                    p.w = self.app_win.w;
                    p.h = self.app_win.h;
                }
            }
            p
        });
        // R1 layer seam: snapshot the app's material-tagged glass regions + the
        // app-window geometry (atlas band origin, on-screen origin, title-bar
        // offset) so the AppClient branch can composite each region as a
        // `nexus-gfx` glass layer without re-borrowing `self` inside the encoder.
        let app_layer_count = self.app_layer_count;
        let app_layers = self.app_layers;
        // TASK #23: the live-resize title overlay (frame-width sharp title,
        // composited OVER the scaled band while band ≠ frame).
        let app_title_overlay = self.app_title_overlay.map(|s| {
            (
                s.abs_row,
                s.x,
                s.width.min(self.app_win.w),
                self.app_win.title_h.min(s.height),
                self.app_win.x.max(0) as u32,
                self.app_win.y.max(0) as u32,
            )
        });
        let app_fullscreen =
            fullscreen_id == Some(crate::window_scene::WindowId::AppClient);
        let app_layer_geom = self.app_win.atlas.map(|a| {
            (
                a.abs_row,
                a.x,
                self.app_win.x.max(0) as u32,
                self.app_win.y.max(0) as u32,
                self.app_win.title_h,
            )
        });
        // Desktop base surface (declarative, Umbau #17): the shell/greeter
        // app-host that declared `level: desktop` owns its OWN full-screen band
        // (separate from the floating `app_win`). Snapshot its geometry for the
        // encoder block; composed as an opaque base layer at the bottom band.
        let desktop_layer = self
            .desktop_band
            .map(|b| (b.abs_row, b.x, b.width.min(self.mode.width), b.height.min(self.mode.height)));
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
                    crate::window_scene::WindowId::AppClient => {
                        use nexus_display_proto::client_surface as wire;
                        if app_layer_count > 0 {
                            if let Some((atlas_row, atlas_x, win_x, win_y, title_h)) = app_layer_geom
                            {
                                for l in app_layers.iter().take(app_layer_count) {
                                    if l.material != wire::MATERIAL_GLASS {
                                        continue;
                                    }
                                    let blur_radius = match l.glass_level {
                                        wire::GLASS_PANEL => 40,
                                        wire::GLASS_CARD => 20,
                                        wire::GLASS_SUBTLE => 12,
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
                        } else if let Some(p) = app_glass {
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
                        if let Some((row, sx, w, h, dx, dy)) = app_title_overlay {
                            if w > 0 && h > 0 {
                                let _ = encoder.composite_layer_full(
                                    &Layer {
                                        corner_radius: if app_fullscreen {
                                            0
                                        } else {
                                            crate::compositor::runtime::app_window::APP_WIN_RADIUS
                                        },
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
                                &Layer::opaque(row, x, w, h, 0, 0),
                                (mode.width, mode.height),
                            );
                        }
                    }
                    // Legacy window ids (chat/search/settings): their windowd
                    // surfaces are DELETED — never registered in the stack, so
                    // these arms are unreachable; explicit to keep the match
                    // total until the WindowId enum retires (surface-id roles).
                    _ => {}
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
