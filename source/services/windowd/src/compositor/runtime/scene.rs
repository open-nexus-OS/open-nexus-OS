// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — `build_scene_cb_into` — the per-frame
//! GPU CommandBuffer builder (GPU-first layered scene). Post-cleanup
//! (cleanup-map DELETE): the scene is damage blits + the z-ordered window
//! layers (desktop base + floating app window) + cursor. All shell
//! chrome (topbar/sidepanel/dropdown/buttons) and the legacy chat/search/
//! settings/greeter surfaces are DELETED — that UI is DSL-app content
//! composited through the desktop/app-window layers.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (behavior covered via windowd QEMU smoke + host integration)

use super::*;
use crate::layer_geometry::{fair_shares, LayerQuota};
use crate::surface_regions::SurfaceRegions;
use crate::window_scene::{WindowId, MAX_APP_WINDOWS};

/// The frame-bound line was said (once per windowd: a heavy frame recurs every frame).
static LAYER_BOUND_SAID: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Per-window scene snapshot (alloc-free `Copy` data captured before the
/// encoder block, RFC-0065 multi-window): what the composite loop needs to
/// draw one app window without re-borrowing the runtime.
#[derive(Clone, Copy, Default)]
struct AppSceneSnap {
    /// The window body: its base layer, glass or solid.
    glass: Option<crate::compositor::shell_window::GlassCompositeParams>,
    /// The surface's declared layers (RFC-0109): regions, scroll layers' slices, washes.
    surface: Option<super::surface_layers_encode::LayerSnap>,
    /// The window drawn as one solid surface (TASK-0336, `solid_surface.rs`).
    solid: Option<super::solid_surface::SolidSnap>,
}

impl DisplayServerRuntime {
    /// Build the per-frame GPU CommandBuffer (GPU-first layout-tree model).
    ///
    /// CPU writes background content (wallpaper + proof panel) into Plane 1 only on
    /// content change. The GPU CB does all visual work per frame:
    ///   1. Blit each damage region: Plane 1 (retained, cursor-free) → Plane 2 (display).
    ///   2. Composite the z-ordered window layers (desktop base, app window).
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
                // The named rows are consumed with the copy; the next present names its own.
                self.render_app_surface(idx)?;
                self.apps[idx].win.surface_dirty = false;
            }
        }
        // Desktop surface (Umbau #17): blit the shell app-host's VMO into the
        // full-screen desktop band when a present marked it dirty.
        if self.desktop_dirty && self.desktop_band.is_some() {
            self.render_desktop_surface()?;
            self.desktop_dirty = false;
        }
        // Dock (TASK-0070 Phase 2): (re)render on membership change.
        // Snapshot all `self` reads needed inside the encoder block so the
        // mutable borrow of `self.scene_cb` does not conflict with field reads.
        let mode = self.mode;
        let cursor_w = self.cursor_width;
        let cursor_h = self.cursor_height;
        let cursor_x = self.state.cursor_x;
        let cursor_y = self.state.cursor_y;
        let cursor_hot = self.cursor_hot;
        let hw_cursor = self.hw_cursor_active;
        let pointer_hidden = self.capture.pointer_hidden();
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
        let mut app_snaps: [AppSceneSnap; MAX_APP_WINDOWS] =
            core::array::from_fn(|_| AppSceneSnap::default());
        // The visible windows' frames: a damage rectangle inside one came from that window
        // (or something above it), the rest from the desktop under every window — the
        // damage's origin (`Command::Damage`, RFC-0101 §8): a glass blurs its backdrop again
        // only for a change below it, never for its own content.
        let mut app_frames: [Option<DamageRect>; MAX_APP_WINDOWS] = [None; MAX_APP_WINDOWS];
        for idx in 0..self.apps.len() {
            if self.apps[idx].win.visible {
                if let Some(f) = app_frames.get_mut(idx) {
                    *f = Some(self.app_window_rect(idx));
                }
            }
        }
        for idx in 0..self.apps.len() {
            let wid = WindowId::App(idx as u8);
            let fullscreen = fullscreen_id == Some(wid);
            let win = &self.apps[idx].win;
            // The surface's base size (RFC-0109): its first rows are the base layer, the rows
            // below it the client's store (its bands, its patches) — never window body.
            let client = self.apps[idx]
                .surface_id
                .and_then(|id| self.client_surfaces.get_by_id(id))
                .copied();
            let base = client.map_or((win.w, win.h), |c| (u32::from(c.width), u32::from(c.height)));
            // A GPU that composes blurs and caches the glass itself: the window holds no blur
            // band (`open_app_window`) and declares its glass without one.
            let gpu_glass = self.gpu_composes && !self.app_presentation(idx).full_screen;
            let glass = win.glass_params().map(|mut p| {
                p.gpu_glass = gpu_glass;
                if fullscreen {
                    p.radius = 0;
                    p.shadow_alpha = 0;
                }
                // Track C2: tag every slice with the window's transform id
                // (slot+1). The encode is ALWAYS the untransformed base — the
                // present's `Command::LayerTransform` (delta translate /
                // multiplied opacity / centre scale) is applied on the GPU, never
                // baked: an open baked at opacity 0 and multiplied again stayed
                // invisible.
                p.layer_id = (idx as u32) + 1;
                p.key = super::surface_layers_encode::key(
                    idx as u32 + 1,
                    super::surface_layers_encode::KIND_BODY,
                    0,
                );
                p.solid = self.solid_materials;
                // A frame other than the surface (a live resize, the frame of a mode switch
                // before the client re-creates at its size): the base is spread over the frame —
                // the window body grows — and snaps sharp when the client draws at the new size.
                if (win.w, win.h) != base {
                    p.content_w = base.0;
                    p.content_h = base.1;
                    p.w = win.w;
                    p.h = win.h;
                }
                p
            });
            app_snaps[idx] = AppSceneSnap {
                glass,
                surface: win.atlas.zip(client).map(|(a, c)| {
                    super::surface_layers_encode::LayerSnap {
                        layers: self.apps[idx].layers,
                        regions: SurfaceRegions::of(&c, a, win.store),
                        origin: (win.x.max(0) as u32, win.y.max(0) as u32),
                        base,
                        owner: idx as u32 + 1,
                        layer_id: idx as u32 + 1,
                    }
                }),
                solid: super::solid_surface::SolidSnap::of(&self.apps[idx], fullscreen),
            };
        }
        // Desktop base surface (declarative, Umbau #17): the shell/greeter
        // app-host that declared `level: desktop` owns its OWN full-screen band
        // (separate from the floating `app_win`). Snapshot its geometry for the
        // encoder block; composed as an opaque base layer at the bottom band.
        let desktop_layer = self.desktop_band.map(|b| {
            (b.abs_row, b.x, b.width.min(self.mode.width), b.height.min(self.mode.height))
        });
        // R1 seam for the DESKTOP surface: its declared layers (RFC-0109) — glass regions over
        // the wallpaper after the base blend, the panels' above the windows, its scroll layers
        // and washes in the pass of the region holding them.
        let desktop_client =
            self.desktop_surface_id.and_then(|id| self.client_surfaces.get_by_id(id)).copied();
        let desktop_surface = self.desktop_band.zip(desktop_client).map(|(b, c)| {
            super::surface_layers_encode::LayerSnap {
                layers: self.desktop_layers,
                regions: SurfaceRegions::of(&c, b, self.desktop_store),
                origin: (0, 0),
                base: (b.width.min(self.mode.width), self.mode.height.min(b.height)),
                owner: super::surface_layers_encode::DESKTOP_OWNER,
                layer_id: 0,
            }
        });
        // Back-to-front window order from the z/focus stack (window_scene SSOT):
        // the composite loop below draws exactly these, in exactly this order.
        let (win_order, win_n) = self.windows.order(self.capture.frozen());
        // Every window slot's transform as this present draws it (TASK-0336/0338): carried with
        // the layers, so a moved frame and its transform take effect together — the present is
        // the only way a transform reaches the display.
        let transforms = self.present_transforms();
        // The frame's layer bound shared out (RFC-0109): the bodies first, then each drawn
        // surface's declared layers within its max-min fair share.
        let mut quotas = layer_quotas(&app_snaps, &win_order[..win_n], desktop_surface.as_ref());
        // Dock layer params (bar rect is None while inactive/covered).
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

            // 1. Blit content damage from retained plane → display plane — and name it as the
            //    scene's damage (RFC-0101 §8, TASK-0215 5c-3 P5): a GPU executor that keeps its
            //    planes redraws this, and what moved, instead of the screen.
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
                let inside = |f: &DamageRect| {
                    f.x <= rect.x
                        && f.y <= rect.y
                        && rect.x.saturating_add(rect.width) <= f.x.saturating_add(f.width)
                        && rect.y.saturating_add(rect.height) <= f.y.saturating_add(f.height)
                };
                // The topmost window the rectangle lies within names the origin.
                let origin = win_order[..win_n]
                    .iter()
                    .rev()
                    .find_map(|wid| match wid {
                        WindowId::App(slot) => app_frames
                            .get(usize::from(*slot))
                            .copied()
                            .flatten()
                            .filter(inside)
                            .map(|_| u32::from(*slot) + 1),
                        _ => None,
                    })
                    .unwrap_or(0);
                encoder
                    .try_damage(
                        nexus_gfx::core::types::TileRect {
                            x: rect.x,
                            y: rect.y,
                            width: rect.width,
                            height: rect.height,
                        },
                        origin,
                    )
                    .map_err(|_| WindowdError::InvalidDamage)?;
            }

            // 1a. A frame owed for what moved alone names no damage (TASK-0338): the planner
            //     redraws the layers drawn otherwise than before — never the whole screen, which
            //     is what a present with layers and no `Damage` means.
            if rect_count == 0 {
                encoder.try_damage_none().map_err(|_| WindowdError::InvalidDamage)?;
            }

            // 1b. The windows' transforms (`Command::LayerTransform`), before their layers.
            for (idx, &(dx, dy, opacity, scale)) in transforms.iter().enumerate() {
                encoder
                    .try_layer_transform(idx as u32 + 1, (dx, dy), opacity, scale)
                    .map_err(|_| WindowdError::InvalidDamage)?;
            }

            // 2. Windows, BACK-TO-FRONT in the z/focus stack's order
            //    (window_scene SSOT): the topmost window composites last and
            //    wins occlusion.
            for &wid in &win_order[..win_n] {
                match wid {
                    // App-client window (ADR-0042 R1): its body, then the layers its surface
                    // declared (`OP_SURFACE_TREE`, RFC-0109) — material regions, scroll layers'
                    // slices, washes. Nothing declared ⇒ the body alone.
                    WindowId::App(slot) => {
                        let sn = &app_snaps[slot as usize];
                        // The window as ONE solid surface (TASK-0336): its starting or closing
                        // surface — a patch spread over the frame, opaque; nothing else of it.
                        if let Some(s) = sn.solid {
                            s.encode(&mut encoder, slot, (mode.width, mode.height));
                            continue;
                        }
                        // The window body first — its base layer — then what the surface
                        // declared over it (RFC-0109): the regions blur the drawn body (the inner
                        // frost), the scroll layers' slices and the washes follow.
                        if let Some(p) = sn.glass {
                            let _ = crate::compositor::shell_window::ShellWindow::composite_glass(
                                &mut encoder,
                                p,
                                mode.width,
                                mode.height,
                            );
                        }
                        if let Some(l) = sn.surface {
                            l.encode(
                                &mut encoder,
                                super::surface_layers_encode::Pass::Window,
                                (mode.width, mode.height),
                                &mut quotas[usize::from(slot)],
                            );
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
                    WindowId::Desktop => {
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
                                    key: super::surface_layers_encode::key(
                                        super::surface_layers_encode::DESKTOP_OWNER,
                                        super::surface_layers_encode::KIND_BODY,
                                        0,
                                    ),
                                    ..Layer::opaque(row, x, w, h, 0, 0)
                                },
                                (mode.width, mode.height),
                            );
                            // Frosted regions: blur the wallpaper behind each declared glass
                            // rect, then draw the band content (tint + text) over it — the
                            // liquid-glass look. The shell's PANEL-level glass composites above
                            // the windows (pass 2b below).
                            if let Some(l) = desktop_surface {
                                l.encode(
                                    &mut encoder,
                                    super::surface_layers_encode::Pass::DesktopBelow,
                                    (mode.width, mode.height),
                                    &mut quotas[MAX_APP_WINDOWS],
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
                        key: super::surface_layers_encode::key(
                            super::surface_layers_encode::DESKTOP_OWNER,
                            super::surface_layers_encode::KIND_BAR,
                            0,
                        ),
                        ..Layer::opaque(row, x, w, bar_h, 0, 0)
                    },
                    (mode.width, mode.height),
                );
                if let Some(l) = desktop_surface {
                    l.encode(
                        &mut encoder,
                        super::surface_layers_encode::Pass::DesktopAbove,
                        (mode.width, mode.height),
                        &mut quotas[MAX_APP_WINDOWS],
                    );
                }
            }

            // 3. Dock of minimized windows (TASK-0070 Phase 2): a glass bar
            //    bottom-center, present ONLY while ≥1 window is minimized and
            //    no fullscreen window covers the chrome. Above the windows.

            // 4. Cursor — composited last, never baked into any plane. Skipped
            //    entirely when the hardware cursor overlay is active (the host
            //    displays and moves the cursor; frames never carry it). In the
            //    software fallback a cursor-only move is a cheap cursor-region
            //    blit (from the retained Plane 1) + this BlendCursor.
            if !hw_cursor && !pointer_hidden && cursor_w > 0 && cursor_h > 0 {
                let cx = (cursor_x - cursor_hot.0).max(0) as u32;
                let cy = (cursor_y - cursor_hot.1).max(0) as u32;
                if cx < mode.width && cy < mode.height {
                    let _ = encoder.try_blend_cursor(cx, cy, cursor_w, cursor_h);
                }
            }

            encoder.end_encoding();
        }
        // A surface past its share lost its own last layers (RFC-0109 failure model): named once,
        // never a refused present.
        let dropped: usize = quotas.iter().map(LayerQuota::dropped).sum();
        if dropped > 0 && !LAYER_BOUND_SAID.swap(true, core::sync::atomic::Ordering::Relaxed) {
            let _ = debug_println(&alloc::format!(
                "windowd: layers over the frame bound (dropped={dropped})"
            ));
        }
        self.scene_cb.serialize_into(out).map_err(|_| WindowdError::InvalidDamage)
    }
}

/// Each surface's share of the frame's layers (RFC-0109 failure model): the drawn windows' bodies
/// and the desktop's base and bar first, then the declared layers max-min fair (`fair_shares`) —
/// index `slot` a window, [`MAX_APP_WINDOWS`] the desktop.
fn layer_quotas(
    snaps: &[AppSceneSnap; MAX_APP_WINDOWS],
    order: &[WindowId],
    desktop: Option<&super::surface_layers_encode::LayerSnap>,
) -> [LayerQuota; MAX_APP_WINDOWS + 1] {
    let mut demand = [0usize; MAX_APP_WINDOWS + 1];
    let mut bodies = if desktop.is_some() { 2 } else { 0 };
    for wid in order {
        let WindowId::App(slot) = *wid else { continue };
        let Some(sn) = snaps.get(usize::from(slot)) else { continue };
        bodies += usize::from(sn.glass.is_some() || sn.solid.is_some());
        if sn.solid.is_none() {
            demand[usize::from(slot)] = sn.surface.map_or(0, |l| l.layers.demand());
        }
    }
    demand[MAX_APP_WINDOWS] = desktop.map_or(0, |l| l.layers.demand());
    let mut share = [0usize; MAX_APP_WINDOWS + 1];
    fair_shares(nexus_gfx::frame::MAX_LAYERS.saturating_sub(bodies), &demand, &mut share);
    share.map(LayerQuota::new)
}
