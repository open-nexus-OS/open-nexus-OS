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

use super::surface_buffers::Owner;
use super::*;
use crate::compositor::shell_window::BodySource;
use crate::layer_geometry::{fair_shares, LayerQuota};
use crate::surface_regions::SurfaceRegions;
use crate::window_scene::WindowId;

/// The frame-bound line was said (once per windowd: a heavy frame recurs every frame).
static LAYER_BOUND_SAID: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Per-window scene snapshot (alloc-free `Copy` data captured before the
/// encoder block, RFC-0065 multi-window): what the composite loop needs to
/// draw one app window without re-borrowing the runtime.
#[derive(Clone, Copy, Default)]
struct AppSceneSnap {
    /// The window's key on the wire (RFC-0114 §7): its layers' id and owner, its damage's
    /// origin, its transform.
    key: u32,
    /// The surface the window shows (its buffer is what the frame reads, RFC-0114).
    shown: Option<u32>,
    /// The window body: its base layer, glass or solid.
    glass: Option<crate::compositor::shell_window::GlassCompositeParams>,
    /// The surface's declared layers (RFC-0109): regions, scroll layers' slices, washes.
    surface: Option<super::surface_layers_encode::LayerSnap>,
    /// The window drawn as one solid surface (TASK-0336, `solid_surface.rs`).
    solid: Option<super::solid_surface::SolidSnap>,
}

/// The frame path's scratch, reused frame after frame (cleared, never reallocated past
/// warm-up): one entry per window slot — the slots grow (RFC-0114 §7) — and the desktop's.
pub(super) struct SceneScratch {
    snaps: Vec<AppSceneSnap>,
    frames: Vec<Option<DamageRect>>,
    quotas: Vec<LayerQuota>,
    demand: Vec<usize>,
    share: Vec<usize>,
    transforms: Vec<(u32, (i32, i32, u32, u32))>,
    /// Per window slot: left out of this frame, covered by the windows above it (TASK-0351 P1).
    culled: Vec<bool>,
    occluders: Vec<nexus_gfx::frame::occlude::Px>,
}

impl SceneScratch {
    pub(super) const fn new() -> Self {
        Self {
            snaps: Vec::new(),
            frames: Vec::new(),
            quotas: Vec::new(),
            demand: Vec::new(),
            share: Vec::new(),
            transforms: Vec::new(),
            culled: Vec::new(),
            occluders: Vec::new(),
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
    ///   4. BlendCursor overlaid last.
    ///
    /// Record the per-frame scene into the reusable `scene_cb` and serialize it
    /// into `out`. Returns the number of bytes written.
    ///
    /// Zero per-frame heap allocation: `scene_cb` is cleared (capacity retained)
    /// rather than freshly allocated, and serialization borrows it instead of
    /// consuming it into a `CommittedBuffer`. When windowd's heap never freed,
    /// a per-frame `CommandBuffer::new()` leaked its `Vec<Command>` and crashed
    /// the service mid-animation; the frame path still allocates nothing.
    pub(super) fn build_scene_cb_into(
        &mut self,
        rects: &[DamageRect],
        rect_count: usize,
        out: &mut [u8],
    ) -> Result<usize, WindowdError> {
        // No pixel is copied (RFC-0114): every window's body and layers sample the app's own
        // buffer, the slot of its newest present — what this frame reads is recorded with its
        // seq once it left (`frame_reads_sent`), the release rule's input.
        self.begin_frame_reads();
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
        // Per-window snapshots (Copy, in the reused scratch) so the encoder block below
        // never re-borrows `self`: glass params, material layer set and geometry for EVERY app
        // window, indexed by slot.
        let mut scratch = core::mem::replace(&mut self.scene_scratch, SceneScratch::new());
        let app_snaps = &mut scratch.snaps;
        app_snaps.clear();
        app_snaps.resize(self.apps.len(), AppSceneSnap::default());
        // The visible windows' frames: a damage rectangle inside one came from that window
        // (or something above it), the rest from the desktop under every window — the
        // damage's origin (`Command::Damage`, RFC-0101 §8): a glass blurs its backdrop again
        // only for a change below it, never for its own content.
        let app_frames = &mut scratch.frames;
        app_frames.clear();
        for idx in 0..self.apps.len() {
            let visible = self.apps[idx].win.visible;
            app_frames.push(visible.then(|| self.app_window_rect(idx)));
        }
        for idx in 0..self.apps.len() {
            let wid = WindowId::App(idx as u16);
            let fullscreen = fullscreen_id == Some(wid);
            let key = self.window_key(idx);
            let win = &self.apps[idx].win;
            // The surface the window shows — its own once it presented, else the one it
            // replaces (a re-create) — and what of it a frame draws (RFC-0114).
            let shown_id = self.shown_surface(Owner::App(idx));
            let shown = shown_id
                .and_then(|id| self.client_surfaces.get_by_id(id))
                .and_then(|c| Some((*c, c.shown()?)));
            // The surface's base size (RFC-0109): its first rows are the base layer, the rows
            // below it the client's store (its bands, its patches) — never window body.
            let base =
                shown.map_or((win.w, win.h), |(c, _)| (u32::from(c.width), u32::from(c.height)));
            // A GPU that composes blurs and caches the glass itself: the window holds no blur
            // band (`open_app_window`) and declares its glass without one.
            let gpu_glass = self.gpu_composes && !self.app_presentation(idx).full_screen;
            let body = shown.map(|(_, sh)| BodySource {
                source: sh.buffer.source,
                slot: sh.slot,
                epoch: sh.generation,
                width: base.0,
                height: base.1,
            });
            let glass = body.map(|body| {
                let mut p = win.glass_params(body);
                p.gpu_glass = gpu_glass;
                if fullscreen {
                    p.radius = 0;
                    p.shadow_alpha = 0;
                }
                // Track C2: tag every slice with the window's transform id (its key). The
                // encode is ALWAYS the untransformed base — the present's
                // `Command::LayerTransform` (delta translate / multiplied opacity / centre
                // scale) is applied on the GPU, never baked: an open baked at opacity 0 and
                // multiplied again stayed invisible.
                p.layer_id = key;
                p.key = super::surface_layers_encode::key(
                    key,
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
            // The layers the shown surface declared: the current one's, or — while it is
            // replaced — those of the surface it replaces, with its last frame.
            let layers = if shown_id == self.apps[idx].surface_id {
                self.apps[idx].layers
            } else {
                self.apps[idx].retiring_layers
            };
            app_snaps[idx] = AppSceneSnap {
                key,
                shown: shown_id.filter(|_| glass.is_some()),
                glass,
                surface: shown.map(|(c, sh)| super::surface_layers_encode::LayerSnap {
                    layers,
                    regions: SurfaceRegions::of(&c, sh),
                    origin: (win.x.max(0) as u32, win.y.max(0) as u32),
                    base,
                    owner: key,
                    layer_id: key,
                }),
                solid: super::solid_surface::SolidSnap::of(&self.apps[idx], fullscreen),
            };
        }
        // Desktop base surface (declarative, Umbau #17): the shell/greeter
        // app-host that declared `level: desktop` owns its OWN full-screen band
        // (separate from the floating `app_win`). Snapshot its geometry for the
        // encoder block; composed as an opaque base layer at the bottom band.
        let desktop_shown_id = self.shown_surface(Owner::Desktop);
        let desktop_shown = desktop_shown_id
            .and_then(|id| self.client_surfaces.get_by_id(id))
            .and_then(|c| Some((*c, c.shown()?)));
        // What the desktop's body samples: its buffer's newest slot, at most the display.
        let desktop_layer = desktop_shown.map(|(c, sh)| {
            let size = (
                u32::from(c.width).min(self.mode.width),
                u32::from(c.height).min(self.mode.height),
            );
            (sh, size)
        });
        // R1 seam for the DESKTOP surface: its declared layers (RFC-0109) — glass regions over
        // the wallpaper after the base blend, the panels' above the windows, its scroll layers
        // and washes in the pass of the region holding them. While the shell's surface is
        // replaced, the replaced one shows with its own layers.
        let desktop_layers = if desktop_shown_id == self.desktop_surface_id {
            self.desktop_layers
        } else {
            self.buffers.desktop_retiring_layers
        };
        let desktop_surface = desktop_shown.zip(desktop_layer).map(|((c, sh), (_, size))| {
            super::surface_layers_encode::LayerSnap {
                layers: desktop_layers,
                regions: SurfaceRegions::of(&c, sh),
                origin: (0, 0),
                base: size,
                owner: super::surface_layers_encode::DESKTOP_OWNER,
                layer_id: 0,
            }
        });
        // Back-to-front window order from the z/focus stack (window_scene SSOT):
        // the composite loop below draws exactly these, in exactly this order.
        let (win_order, win_n) = self.windows.order(self.capture.frozen());
        // Every live window's transform as this present draws it (TASK-0336/0338): carried with
        // the layers, so a moved frame and its transform take effect together — the present is
        // the only way a transform reaches the display.
        let transforms = &mut scratch.transforms;
        self.present_transforms(transforms);
        // A window covered by the windows above it is left out of the frame (TASK-0351 P1,
        // `occlusion.rs`): walked front to back, each body at its transform.
        let culled = &mut scratch.culled;
        let occluded = super::occlusion::cull(
            win_order[..win_n].iter().rev().filter_map(|wid| {
                let WindowId::App(slot) = *wid else { return None };
                let sn = &app_snaps[usize::from(slot)];
                let p = sn.glass.as_ref().filter(|_| sn.solid.is_none())?;
                let t = transforms.iter().find(|(k, _)| *k == sn.key).map(|(_, t)| *t);
                Some((usize::from(slot), p, t.unwrap_or(super::occlusion::IDENTITY)))
            }),
            (mode.width, mode.height),
            app_snaps.len(),
            culled,
            &mut scratch.occluders,
        );
        // What this frame reads (RFC-0114 §5): every drawn window's shown surface, the desktop's.
        for wid in &win_order[..win_n] {
            let read = match *wid {
                WindowId::App(slot) => app_snaps
                    .get(usize::from(slot))
                    .filter(|sn| sn.solid.is_none() && !culled[usize::from(slot)])
                    .and_then(|sn| sn.shown),
                WindowId::Desktop => desktop_layer.and(desktop_shown_id),
            };
            if let Some(id) = read {
                self.note_frame_reads(id);
            }
        }
        // RFC-0113 §5: the focused field's caret, over the surface that holds it.
        let caret = self.caret_draw();
        // The frame's layer bound shared out (RFC-0109): the bodies first, then each drawn
        // surface's declared layers — and the caret it holds — within its max-min fair share.
        let caret_owner = caret.map(|c| c.owner);
        let desktop_quota = app_snaps.len();
        let quotas = &mut scratch.quotas;
        layer_quotas(
            (app_snaps, &win_order[..win_n], culled),
            desktop_surface.as_ref(),
            caret_owner,
            (&mut scratch.demand, &mut scratch.share, quotas),
        );
        // The caret's layer set aside before its surface spends the share: a tight frame drops
        // the surface's own last layer, never the caret (`None`: not drawn this frame).
        let caret =
            caret.filter(|c| quotas[c.owner.map_or(desktop_quota, usize::from)].set_aside());
        // Dock layer params (bar rect is None while inactive/covered).
        self.scene_cb.clear();
        // App windows this frame draws from their own buffers (`windows drawn`, RFC-0114 §7).
        let mut drawn = 0usize;
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
                // The topmost window the rectangle lies within names the origin (its key).
                let origin = win_order[..win_n]
                    .iter()
                    .rev()
                    .find_map(|wid| match wid {
                        WindowId::App(slot) => app_frames
                            .get(usize::from(*slot))
                            .copied()
                            .flatten()
                            .filter(inside)
                            .map(|_| app_snaps[usize::from(*slot)].key),
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

            // 1b. The windows' transforms (`Command::LayerTransform`, by key), before their
            //     layers: every live window's — a present with layers names them all.
            for &(key, (dx, dy, opacity, scale)) in transforms.iter() {
                encoder
                    .try_layer_transform(key, (dx, dy), opacity, scale)
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
                        let sn = &app_snaps[usize::from(slot)];
                        // Covered by the windows above it: nothing of it (TASK-0351 P1).
                        if culled[usize::from(slot)] {
                            continue;
                        }
                        // The window as ONE solid surface (TASK-0336): its starting or closing
                        // surface — a patch spread over the frame, opaque; nothing else of it.
                        if let Some(s) = sn.solid {
                            s.encode(&mut encoder, sn.key, (mode.width, mode.height));
                            continue;
                        }
                        // The window body first — its base layer — then what the surface
                        // declared over it (RFC-0109): the regions blur the drawn body (the inner
                        // frost), the scroll layers' slices and the washes follow.
                        if let Some(p) = sn.glass {
                            drawn += usize::from(
                                crate::compositor::shell_window::ShellWindow::composite_glass(
                                    &mut encoder,
                                    p,
                                    mode.width,
                                    mode.height,
                                ),
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
                        if let Some(c) = caret.filter(|c| c.owner == Some(slot)) {
                            c.encode(&mut encoder, (mode.width, mode.height));
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
                        if let Some((sh, (w, h))) = desktop_layer {
                            let _ = encoder.composite_layer_full(
                                &Layer {
                                    content_epoch: sh.generation,
                                    source: sh.buffer.source,
                                    slot: sh.slot,
                                    key: super::surface_layers_encode::key(
                                        super::surface_layers_encode::DESKTOP_OWNER,
                                        super::surface_layers_encode::KIND_BODY,
                                        0,
                                    ),
                                    ..Layer::opaque(0, 0, w, h, 0, 0)
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
                                    &mut quotas[desktop_quota],
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
            if let Some((sh, (w, h))) = desktop_layer {
                let bar_h = super::SHELL_TOPBAR_H.min(h);
                let _ = encoder.composite_layer_full(
                    &Layer {
                        content_epoch: sh.generation,
                        source: sh.buffer.source,
                        slot: sh.slot,
                        key: super::surface_layers_encode::key(
                            super::surface_layers_encode::DESKTOP_OWNER,
                            super::surface_layers_encode::KIND_BAR,
                            0,
                        ),
                        ..Layer::opaque(0, 0, w, bar_h, 0, 0)
                    },
                    (mode.width, mode.height),
                );
                if let Some(l) = desktop_surface {
                    l.encode(
                        &mut encoder,
                        super::surface_layers_encode::Pass::DesktopAbove,
                        (mode.width, mode.height),
                        &mut quotas[desktop_quota],
                    );
                }
            }
            // The desktop's caret over its topmost layer (the shell's search, the greeter).
            if let Some(c) = caret.filter(|c| c.owner.is_none()) {
                c.encode(&mut encoder, (mode.width, mode.height));
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
        self.scene_scratch = scratch;
        self.buffers.frame_windows = drawn;
        self.buffers.frame_occluded = occluded;
        crate::compositor::frame_causes::windows(drawn as u32, occluded as u32);
        self.scene_cb.serialize_into(out).map_err(|_| WindowdError::InvalidDamage)
    }
}

/// Each surface's share of the frame's layers (RFC-0109 failure model): the drawn windows' bodies
/// and the desktop's base and bar first, then the declared layers max-min fair (`fair_shares`) —
/// index `slot` a window, `snaps.len()` the desktop — into `quotas`. The caret is its surface's
/// last layer and counts in that surface's demand (`caret`: its window slot, `Some(None)` the
/// desktop) — left out, every share was exactly the declared layers and the caret the one dropped.
fn layer_quotas(
    (snaps, order, culled): (&[AppSceneSnap], &[WindowId], &[bool]),
    desktop: Option<&super::surface_layers_encode::LayerSnap>,
    caret: Option<Option<u16>>,
    (demand, share, quotas): (&mut Vec<usize>, &mut Vec<usize>, &mut Vec<LayerQuota>),
) {
    let n = snaps.len() + 1;
    demand.clear();
    demand.resize(n, 0);
    share.clear();
    share.resize(n, 0);
    let mut bodies = if desktop.is_some() { 2 } else { 0 };
    for wid in order {
        let WindowId::App(slot) = *wid else { continue };
        let Some(sn) = snaps.get(usize::from(slot)) else { continue };
        if culled.get(usize::from(slot)).is_some_and(|c| *c) {
            continue;
        }
        bodies += usize::from(sn.glass.is_some() || sn.solid.is_some());
        if sn.solid.is_none() {
            let holds_caret = caret == Some(Some(slot));
            demand[usize::from(slot)] =
                sn.surface.map_or(0, |l| l.layers.demand()) + usize::from(holds_caret);
        }
    }
    let desktop_caret = usize::from(caret == Some(None));
    demand[n - 1] = desktop.map_or(0, |l| l.layers.demand() + desktop_caret);
    fair_shares(nexus_gfx::frame::MAX_LAYERS.saturating_sub(bodies), demand, share);
    quotas.clear();
    quotas.extend(share.iter().map(|s| LayerQuota::new(*s)));
}
