// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The focused field's caret as a compositor layer (RFC-0113 §5, ADR-0090). An app
//! declares it (`OP_SURFACE_CARET`: a thin rectangle in surface pixels, its colour, whether it
//! blinks); windowd draws it as ONE solid layer over the surface — a 16×16 atlas patch of its
//! colour spread over the rectangle, the window's transform applied — and blinks it on the loop's
//! one-shot wake (`crate::wake_clock`, shared with the wait ring): no app wake per blink, no
//! raster, a caret move is one small frame. It shows only while its surface holds window focus
//! and is visible, clipped to the surface (`crate::caret_rule`). A reset (an edit, a move) shows
//! it solid for a whole half-blink; ten seconds without one and it rests solid — a caret nobody
//! types at costs no wake.
//! The claim is the app's (windowd's server endpoint names no app sender — the trust level of
//! `OP_SURFACE_TEXT_FOCUS`): a wrong surface id draws a thin bar in the focused surface, nothing
//! more; an unknown surface is refused.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the phase and the clip in `crate::caret_rule` (host); the caret on the visible
//!   lanes (`windowd: caret layer on`)

use super::*;
use crate::caret_rule;
use crate::damage_rects::DamageRect;
use nexus_display_proto::surface_caret::{decode_surface_caret, Caret, CARET_BLINK, CARET_RESET};
use nexus_gfx::{Layer, RenderCommandEncoder};

/// The patch's side: a texel of margin around the 14×14 the layer samples (`solid_surface.rs`).
const PATCH: u32 = 16;

/// The caret state: the declaration, its blink, its patch, what was drawn.
pub(crate) struct CaretLayer {
    /// Every surface's declared caret; the focused surface's shows (`caret_rule::CaretTable`).
    carets: caret_rule::CaretTable,
    /// The surface whose caret the last pass considered: a focus change restarts the blink, so
    /// the field just focused shows its caret solid at once.
    focus: Option<u32>,
    /// When the blink last restarted.
    reset_ns: u64,
    /// The atlas patch of its colour, and that colour.
    patch: Option<(crate::atlas::AtlasSurface, [u8; 4])>,
    /// What the last frame drew: the screen rectangle and the window slot it rides (`None` = the
    /// desktop surface).
    drawn: Option<(DamageRect, Option<u16>)>,
    /// The loop's one-shot wake (`crate::wake_clock`): armed at the caret's next blink or the
    /// wait ring's next step, whichever comes first.
    timer: Option<nexus_ipc::timer::NotifyTimer>,
    /// The kernel refused the blink timer: the caret stands steady (never stuck in its off half).
    no_timer: bool,
    said: bool,
}

impl CaretLayer {
    /// A destroyed surface's caret goes with it (a reused id never shows a stale caret).
    pub(crate) fn forget(&mut self, surface: u32) {
        self.carets.hide(surface);
    }

    pub(crate) const fn new() -> Self {
        Self {
            carets: caret_rule::CaretTable::new(),
            focus: None,
            reset_ns: 0,
            patch: None,
            drawn: None,
            timer: None,
            no_timer: false,
            said: false,
        }
    }
}

/// The caret as a frame draws it (a `Copy` snapshot taken before the encoder block).
#[derive(Clone, Copy)]
pub(super) struct CaretDraw {
    /// The window slot it rides (`None` = the desktop surface).
    pub(super) owner: Option<u16>,
    /// That window's key on the wire (RFC-0114 §7; the desktop's for the desktop surface).
    key: u32,
    rect: DamageRect,
    patch: crate::atlas::AtlasSurface,
    opaque: bool,
    /// The transform it rides: its window's, or a moving motion layer's that holds it (TASK-0354
    /// P2: a field sliding in carries its caret).
    layer_id: u32,
}

impl CaretDraw {
    /// One solid layer — the one its owner's share set aside for it (`LayerQuota::set_aside`).
    pub(super) fn encode(&self, enc: &mut RenderCommandEncoder<'_>, mode: (u32, u32)) {
        let owner = self.key;
        let r = self.rect;
        let _ = enc.composite_layer_full(
            &Layer {
                src_row_abs: self.patch.abs_row + 1,
                src_x: self.patch.x + 1,
                width: r.width,
                height: r.height,
                content_w: PATCH - 2,
                content_h: PATCH - 2,
                dst_x: r.x,
                dst_y: r.y,
                opacity: 255,
                corner_radius: 0,
                layer_id: self.layer_id,
                content_epoch: crate::atlas::atlas_content_epoch(),
                scroll_band_top_abs: 0,
                scroll_band_h: 0,
                // windowd's own patch in the layout object (RFC-0114 §6: source 0).
                source: 0,
                slot: 0,
                shadow: None,
                backdrop: None,
                opaque: self.opaque,
                solid: true,
                key: super::surface_layers_encode::key(
                    owner,
                    super::surface_layers_encode::KIND_CARET,
                    0,
                ),
            },
            mode,
        );
    }
}

impl DisplayServerRuntime {
    /// `OP_SURFACE_CARET`: an app's caret (data-only, never answered).
    pub(crate) fn handle_surface_caret(&mut self, frame: &[u8]) {
        let Some((surface_id, caret)) = decode_surface_caret(frame) else {
            let _ = debug_println("WINDOWD: FAIL surface caret (malformed)");
            return;
        };
        let known = self.desktop_surface_id == Some(surface_id)
            || self.app_index_by_surface(surface_id).is_some();
        if !known {
            return; // a stale claim after a destroy: nothing to draw on
        }
        let now = nexus_abi::nsec().unwrap_or(0);
        if caret.visible() {
            let moved = self.caret.carets.declare(surface_id, caret);
            if (moved || caret.flags & CARET_RESET != 0)
                && self.window_focus_surface() == Some(surface_id)
            {
                self.caret.reset_ns = now;
            }
        } else {
            self.caret.carets.hide(surface_id);
        }
        self.refresh_caret(now);
    }

    /// The loop's one-shot wake (`crate::wake_clock`), pumped once per pass: the timer drained,
    /// the wait ring stepped, what the caret shows now compared with what was drawn (a focus or
    /// a window change shows or hides it too), the timer re-armed at the earliest deadline either
    /// owes — none while the caret rests or shows nowhere and no launch waits.
    ///
    /// The timer endpoint is a waitset member from the first pass: it is bound then and drained
    /// on EVERY pass. init's close of its own copy latches the member ready (the owner is never
    /// its own peer, RFC-0079) and only the receiver observing it empty clears the latch — an
    /// undrained member spins the loop (the first caret lane, 2026-10-30: `loop hz=1786`).
    pub(crate) fn pump_clock(&mut self) {
        use nexus_service_topology::slots::windowd as topo;
        if self.caret.timer.is_none() && !self.caret.no_timer {
            self.caret.timer = nexus_ipc::timer::NotifyTimer::bind(topo::TIMER).ok();
            if self.caret.timer.is_none() {
                self.caret.no_timer = true;
                let _ = debug_println("windowd: caret blink timer refused (the caret stands)");
            }
        }
        match self.caret.timer.as_mut() {
            Some(t) => {
                t.drain();
            }
            None => drain_endpoint(topo::TIMER_RECV),
        }
        let now = nexus_abi::nsec().unwrap_or(0);
        let _ = self.cursor_wait_tick(now);
        self.refresh_caret(now);
    }

    fn refresh_caret(&mut self, now: u64) {
        let (shown, next) = self.caret_now(now);
        if shown != self.caret.drawn {
            for (rect, _) in [self.caret.drawn, shown].into_iter().flatten() {
                self.queue_dirty_rect(rect);
            }
            self.caret.drawn = shown;
            if shown.is_some() && !core::mem::replace(&mut self.caret.said, true) {
                let _ = debug_println("windowd: caret layer on");
            }
        }
        let wake = crate::wake_clock::earliest(next, self.cursor_wait_wake_ns(now));
        if let Some(t) = self.caret.timer.as_mut() {
            t.arm_at(wake.unwrap_or(0));
        }
    }

    /// Where the caret shows at `now` (its screen rectangle and window slot) and when that
    /// changes next — nothing while its surface lacks window focus or is hidden.
    fn caret_now(&mut self, now: u64) -> (Option<(DamageRect, Option<u16>)>, Option<u64>) {
        let focus = self.window_focus_surface();
        if focus != self.caret.focus {
            // Focus moved to another window: its own caret (kept in the table) shows solid first.
            self.caret.focus = focus;
            self.caret.reset_ns = now;
        }
        let Some(surface_id) = focus else { return (None, None) };
        let Some(caret) = self.caret.carets.get(surface_id) else { return (None, None) };
        let (origin, size, owner) = if self.desktop_surface_id == Some(surface_id) {
            let visible = self.windows.is_visible(crate::window_scene::WindowId::Desktop);
            if !visible {
                return (None, None);
            }
            ((0, 0), (self.mode.width, self.mode.height), None)
        } else {
            let Some(idx) = self.app_index_by_surface(surface_id) else { return (None, None) };
            let win = &self.apps[idx].win;
            if !win.visible {
                return (None, None);
            }
            let base = self.client_surfaces.get_by_id(surface_id).map_or((win.w, win.h), |c| {
                (u32::from(c.width).min(win.w), u32::from(c.height).min(win.h))
            });
            ((win.x, win.y), base, Some(idx as u16))
        };
        // Without a timer the blink could not come back on: the caret stands steady.
        let caret = if self.caret.no_timer {
            Caret { flags: caret.flags & !CARET_BLINK, ..caret }
        } else {
            caret
        };
        let (on, next) = caret_rule::phase(self.caret.reset_ns, now, &caret);
        if !on || !self.caret_patch(caret.bgra) {
            return (None, next);
        }
        let mode = (self.mode.width, self.mode.height);
        let rect = caret_rule::screen_rect(origin, size, &caret, mode)
            .map(|(x, y, width, height)| (DamageRect { x, y, width, height }, owner));
        (rect, next)
    }

    /// The caret's colour patch in the atlas (filled once per colour); `false` without room.
    fn caret_patch(&mut self, bgra: [u8; 4]) -> bool {
        if let Some((patch, colour)) = self.caret.patch {
            if colour == bgra {
                return true;
            }
            self.fill_atlas_patch(&patch, bgra);
            crate::atlas::note_atlas_content_write();
            self.caret.patch = Some((patch, bgra));
            return true;
        }
        let Some(patch) = self.atlas_alloc.alloc(PATCH, PATCH) else { return false };
        self.fill_atlas_patch(&patch, bgra);
        crate::atlas::note_atlas_content_write();
        self.caret.patch = Some((patch, bgra));
        true
    }

    /// The caret as this frame draws it — a frame shown at `at`.
    pub(super) fn caret_draw(&self, at: u64) -> Option<CaretDraw> {
        let (rect, owner) = self.caret.drawn?;
        let (patch, bgra) = self.caret.patch?;
        let key = owner.map_or(super::surface_layers_encode::DESKTOP_OWNER, |s| {
            self.window_key(usize::from(s))
        });
        let (layers, origin) = match owner {
            None => (&self.desktop_layers, (0, 0)),
            Some(s) => {
                let r = self.app_window_rect(usize::from(s));
                (&self.apps.get(usize::from(s))?.layers, (r.x, r.y))
            }
        };
        let at_rest = if owner.is_some() { key } else { 0 };
        let surf = (
            rect.x.saturating_sub(origin.0),
            rect.y.saturating_sub(origin.1),
            rect.width,
            rect.height,
        );
        let live = super::surface_motion::live(layers, at);
        let layer_id = match crate::motion_geometry::holder(&layers.tree, surf) {
            Some(k) if live & (1 << k) != 0 => crate::motion_geometry::layer_id(key, k),
            _ => at_rest,
        };
        Some(CaretDraw { owner, key, rect, patch, opaque: bgra[3] == 255, layer_id })
    }
}

/// Every frame queued on `slot` taken (non-blocking) — the empty receive clears the member's
/// EOF latch (see `pump_clock`).
fn drain_endpoint(slot: u32) {
    let mut buf = [0u8; 32];
    loop {
        let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let flags = nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE;
        if nexus_abi::ipc_recv_v1(slot, &mut hdr, &mut buf, flags, 0).is_err() {
            return;
        }
    }
}
