// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A window as ONE solid surface (TASK-0336). From the launch's geometry handshake
//! until the app's first frame — the STARTING surface — and from the close until the
//! exit spring ends — the CLOSING surface — the window is drawn as a 16×16 atlas patch of the
//! window colour spread over its frame (`LAYER_SOLID | LAYER_OPAQUE`): the open and close
//! animations move one cheap opaque layer, never the app's layers with their glass, and the
//! content appears when the app has drawn it (the reference platforms' starting window). The
//! wait the user saw as nothing (spawn → mount → first frame, 0.5–6.9 s on the board, cycle
//! 51) is a window that is already there.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the planner's solid split (`nexus-gfx tests/frame_board.rs`); the swap and the
//!   deadline on the QEMU visible lane and the board (`windowd: starting surface`).
//! INVARIANTS: a starting slot is never handed to another launch (`take_app_slot`); it ends at
//!   its app's first frame (the swap), its app's death (a dead push) or — before the app created
//!   its surface — the launch deadline: never left on screen by a launch that will not surface.

use super::*;
use nexus_gfx::{Layer, LayerShadow, RenderCommandEncoder};

/// The patch's side: one texel of margin around the 14×14 the layer samples, so a bilinear tap
/// at its edge reads the same colour on every executor.
const PATCH: u32 = 16;

/// The longest a launch may show its starting surface without its own (cycle 51 measured
/// 6.9 s to settings' first frame): past it the starting surface closes and the app, if it
/// surfaces later, opens as a plain fresh window.
pub(super) const STARTING_TIMEOUT_NS: u64 = 15_000_000_000;

impl DisplayServerRuntime {
    /// Slot `idx` drawn as one solid surface from the next present on. `false` when the atlas
    /// has no room for the patch (the window then draws as before — degraded, never broken).
    pub(super) fn show_solid(&mut self, idx: usize) -> bool {
        if self.apps[idx].solid.is_some() {
            return true;
        }
        let Some(patch) = self.atlas_alloc.alloc(PATCH, PATCH) else {
            return false;
        };
        let s = self.theme().surface;
        self.fill_atlas_patch(&patch, [s[0], s[1], s[2], 255]);
        crate::atlas::note_atlas_content_write();
        self.apps[idx].solid = Some(patch);
        let rect = self.app_window_rect(idx);
        self.queue_dirty_rect(rect);
        true
    }

    /// Slot `idx` drawn as its app again (or closed): the patch returns to the atlas.
    pub(super) fn drop_solid(&mut self, idx: usize) {
        if let Some(patch) = self.apps[idx].solid.take() {
            self.atlas_alloc.free(patch);
        }
    }

    pub(super) fn fill_atlas_patch(&mut self, patch: &crate::atlas::AtlasSurface, bgra: [u8; 4]) {
        let Some(handle) = self.framebuffer else { return };
        let stride = self.mode.stride as usize;
        let mut row = [0u8; PATCH as usize * 4];
        for px in row.chunks_exact_mut(4) {
            px.copy_from_slice(&bgra);
        }
        let n = patch.width.min(PATCH) as usize * 4;
        for r in 0..patch.height {
            let dst = (patch.abs_row + r) as usize * stride + patch.x as usize * 4;
            let _ = vmo_write(handle, dst, &row[..n]);
        }
    }

    /// The launch's geometry is known (`OP_SURFACE_INTENT` of a fresh floating window, its
    /// event channel `channel`): a slot is reserved for it, framed at the size the reply names,
    /// shown as the starting surface, and the enter spring starts on it. The wait ring keeps
    /// its frame clock until the app's surface — or the starting deadline.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn begin_starting_surface(
        &mut self,
        channel: u32,
        (width, height): (u32, u32),
        (style, level, mode, resizable): (u8, u8, u8, bool),
    ) {
        let Some(idx) = self.take_app_slot() else { return };
        self.apps[idx].reset_wm_state();
        self.apps[idx].intent_style = style;
        self.apps[idx].intent_level = level;
        self.apps[idx].intent_mode = mode;
        self.apps[idx].intent_resizable = resizable;
        self.frame_floating(idx, (width, height));
        if !self.show_solid(idx) {
            return;
        }
        self.apps[idx].starting = true;
        self.apps[idx].event_channel = Some(channel);
        self.apps[idx].win.visible = true;
        let wid = crate::window_scene::WindowId::App(idx as u16);
        let role = self.app_presentation(idx).role;
        self.windows.set_role(wid, role);
        self.show_window(wid);
        self.start_open_transition(idx);
        let now = nexus_abi::nsec().unwrap_or(0);
        self.cursor_wait_deadline_ns =
            self.cursor_wait_deadline_ns.max(now.saturating_add(STARTING_TIMEOUT_NS));
        let _ = debug_println(&alloc::format!(
            "windowd: starting surface (slot={idx} {width}x{height})"
        ));
    }

    /// The app in slot `idx` presented its first frame: the starting surface gives way to it in
    /// the same present (the window keeps its frame and its running spring). Returns whether
    /// the slot was starting.
    pub(super) fn end_starting_surface(&mut self, idx: usize) -> bool {
        if !self.apps[idx].starting {
            return false;
        }
        self.apps[idx].starting = false;
        self.drop_solid(idx);
        true
    }

    /// The launch deadline passed with starting surfaces still up: their apps have not surfaced.
    /// Each starting surface goes and its slot is free again — but the app's event channel stays
    /// in the nonce table: a slow app that creates its surface later opens as a plain fresh
    /// window; a dead one is reaped by its first failed push.
    pub(super) fn close_starting_surfaces(&mut self) {
        for idx in 0..self.apps.len() {
            // An app that created its surface has surfaced: its first frame ends the starting
            // surface (`handle_surface_present`).
            if !self.apps[idx].starting || self.apps[idx].surface_id.is_some() {
                continue;
            }
            let _ =
                debug_println(&alloc::format!("windowd: starting surface expired (slot={idx})"));
            let vacated = self.app_window_rect(idx);
            self.apps[idx].starting = false;
            self.drop_solid(idx);
            self.apps[idx].event_channel = None;
            self.apps[idx].win.visible = false;
            self.apps[idx].transform = WinTransform::IDENTITY;
            self.apps[idx].pending_wm = None;
            self.hide_window(crate::window_scene::WindowId::App(idx as u16));
            self.queue_dirty_rect(vacated);
        }
    }
}

/// A solid surface as the scene encoder draws it: the patch, the frame (x, y, w, h), the corner
/// radius and the window's shadow (blur, offset, alpha).
#[derive(Clone, Copy)]
pub(super) struct SolidSnap {
    patch: crate::atlas::AtlasSurface,
    frame: (u32, u32, u32, u32),
    radius: u32,
    shadow: (u32, i32, u32),
}

impl SolidSnap {
    /// Slot `app`'s solid surface, if it shows one (edge to edge, unshadowed, when fullscreen).
    pub(super) fn of(app: &AppWindowSlot, fullscreen: bool) -> Option<Self> {
        let win = &app.win;
        app.solid.map(|patch| Self {
            patch,
            frame: (win.x.max(0) as u32, win.y.max(0) as u32, win.w, win.h),
            radius: if fullscreen { 0 } else { super::app_window::APP_WIN_RADIUS },
            shadow: if fullscreen {
                (0, 0, 0)
            } else {
                (win.shadow_blur, win.shadow_offset_y, win.shadow_alpha)
            },
        })
    }

    /// The patch spread over the window's frame (`LAYER_SOLID | LAYER_OPAQUE`), the window's
    /// transform id on it — so the open and close springs move it like the window.
    pub(super) fn encode(
        &self,
        encoder: &mut RenderCommandEncoder<'_>,
        key: u32,
        mode: (u32, u32),
    ) {
        let (x, y, w, h) = self.frame;
        let (blur, offset_y, alpha) = self.shadow;
        let _ = encoder.composite_layer_full(
            &Layer {
                src_row_abs: self.patch.abs_row + 1,
                src_x: self.patch.x + 1,
                width: w.min(mode.0.saturating_sub(x)),
                height: h.min(mode.1.saturating_sub(y)),
                content_w: self.patch.width.saturating_sub(2),
                content_h: self.patch.height.saturating_sub(2),
                dst_x: x,
                dst_y: y,
                opacity: 255,
                corner_radius: self.radius,
                layer_id: key,
                content_epoch: crate::atlas::atlas_content_epoch(),
                scroll_band_top_abs: 0,
                scroll_band_h: 0,
                // windowd's own patch in the layout object (RFC-0114 §6: source 0).
                source: 0,
                slot: 0,
                shadow: (alpha > 0).then_some(LayerShadow { blur, offset_y, alpha }),
                backdrop: None,
                opaque: true,
                solid: true,
                key: super::surface_layers_encode::key(
                    key,
                    super::surface_layers_encode::KIND_SOLID,
                    0,
                ),
            },
            mode,
        );
    }
}
