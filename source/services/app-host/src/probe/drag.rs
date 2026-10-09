// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the pointer frames that are not taps (RFC-0095) — hover motion and its leave, and
//! the drag gesture: windowd sends `INPUT_KIND_DRAG` while the primary button holds a press on
//! this surface and `INPUT_KIND_RELEASE` when it lets go. A press arms a drag; once the pointer
//! moved past `SLOP` the page's `on DragStart` is hit-tested at the PRESS point, and the box
//! that took it hears every `DragMove` and the `DragEnd` (the release) wherever the pointer
//! goes (`View::fire_on_box`). `device.dragX`/`dragY`/`dragStartX`/`dragStartY` carry the
//! pointer and the press, surface pixels; one app-host per process, so a process-wide cell is
//! their honest home — every `device_for` reads it.
//!
//! Lazy and reactive: a DRAG frame only NOTES the newest position; the present loop computes it
//! once, right before the frame that shows it (`drag_flush`) — positions that arrive while a
//! present is in flight are superseded, never computed. The frame that starts a drag is also its
//! first move, the release its last move and its end — each pair dispatched together and laid
//! out once. The damage is the layout diff of the change (`layout_diff`: a dragged selection
//! repaints the hole's rows, not the frame), and glass regions are re-declared only when a glass
//! root moved. The release carries the gesture's last position (windowd parks on it), so a drag
//! whose motion frames were lost still starts, moves and ends where the pointer let go. One
//! bounded trace line per gesture (`apphost: drag …`, the tap trace's sibling) — never in the
//! keyboard overlay, where a press is a keystroke.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the dispatch rule in `nexus-dsl-runtime/tests/drag_gesture.rs`, the damage in
//!   `layout_diff`'s tests; the wire path by the usb-visible lane's selection drag
//!   (`screencapd: saved (kind=area …)`)

use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use nexus_display_proto::client_surface as wire;
use nexus_dsl_runtime::Damage;
use nexus_ipc::{Client as _, KernelClient, Wait};

/// Pointer travel (pixels, either axis) before a held press becomes a drag.
const SLOP: i32 = 3;

/// A press a drag may grow from.
#[derive(Clone, Copy, Debug)]
pub(super) struct Drag {
    start: (i32, i32),
    /// The box that took `DragStart`; `Some(None)` once the press was found to start none.
    taken: Option<Option<usize>>,
    /// The newest DRAG position not computed yet — every later one supersedes it.
    pending: Option<(i32, i32)>,
}

static DRAG_X: AtomicI32 = AtomicI32::new(0);
static DRAG_Y: AtomicI32 = AtomicI32::new(0);
static START_X: AtomicI32 = AtomicI32::new(0);
static START_Y: AtomicI32 = AtomicI32::new(0);

fn set_cell(x: i32, y: i32, start: (i32, i32)) {
    DRAG_X.store(x, Ordering::Relaxed);
    DRAG_Y.store(y, Ordering::Relaxed);
    START_X.store(start.0, Ordering::Relaxed);
    START_Y.store(start.1, Ordering::Relaxed);
}

/// The drag axes the device env carries: `(x, y, start x, start y)`; zero outside a drag.
pub(crate) fn current() -> (i32, i32, i32, i32) {
    (
        DRAG_X.load(Ordering::Relaxed),
        DRAG_Y.load(Ordering::Relaxed),
        START_X.load(Ordering::Relaxed),
        START_Y.load(Ordering::Relaxed),
    )
}

impl super::DslApp {
    /// A primary press landed on this surface (the tap): a drag may grow from it.
    pub(super) fn arm_drag(&mut self, x: i32, y: i32) {
        self.drag = Some(Drag { start: (x, y), taken: None, pending: None });
    }

    /// One pointer frame that is not a tap: hover motion, leave, drag motion, release.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn pointer_frame(
        &mut self,
        client: &KernelClient,
        surface_id: u32,
        kind: u8,
        x: i32,
        y: i32,
        dirty: &mut bool,
        dirty_rows: &mut Option<(i32, i32)>,
    ) {
        use crate::layout_diff::merge_span;
        let repaint = match kind {
            wire::INPUT_KIND_DRAG => {
                // Noted, not computed: the present loop computes the newest (`drag_flush`).
                if let Some(drag) = self.drag.as_mut() {
                    drag.pending = Some((x, y));
                }
                return;
            }
            wire::INPUT_KIND_RELEASE => self.drag_end(x, y, dirty, dirty_rows),
            wire::INPUT_KIND_LEAVE => {
                if self.text_hover_clear() {
                    super::boot::send_cursor_hint(client, surface_id, false);
                }
                // The un-hover spring needs pulses too.
                self.hover_clear().map(|span| merge_span(dirty, dirty_rows, span)).is_some()
                    && self.anim_active()
            }
            _ => {
                // Editable-field hover → windowd cursor hint (I-beam), sent only on CHANGE.
                if let Some(over) = self.text_hover(x, y) {
                    super::boot::send_cursor_hint(client, surface_id, over);
                }
                // Frame-aligned hover: paint-only, the union row span of the old+new hovered
                // boxes (never a re-layout, never a full-frame repaint — the damage contract).
                self.hover(x, y).map(|span| merge_span(dirty, dirty_rows, span)).is_some()
                    && self.anim_active()
            }
        };
        // Started interaction springs or animations tick on the frame pulse.
        if repaint && self.anim_active() {
            let _ = client.send(&wire::encode_surface_frame_req(surface_id), Wait::Blocking);
        }
    }

    /// Computes the newest noted drag position — once, right before the frame that shows it (the
    /// present loop calls this when no present is in flight; nothing noted = nothing to do).
    pub(super) fn drag_flush(
        &mut self,
        client: &KernelClient,
        surface_id: u32,
        dirty: &mut bool,
        dirty_rows: &mut Option<(i32, i32)>,
    ) {
        let Some((x, y)) = self.drag.as_mut().and_then(|d| d.pending.take()) else { return };
        let damage = self.drag_move(x, y);
        if self.settle(damage, dirty, dirty_rows) && self.anim_active() {
            let _ = client.send(&wire::encode_surface_frame_req(surface_id), Wait::Blocking);
        }
    }

    /// The pointer with the press held at `(x, y)`: starts the drag past the slop (the start is
    /// also the first move — the pointer is already past the press), then moves it. Dispatches
    /// only; the caller lays out once (`settle`).
    fn drag_move(&mut self, x: i32, y: i32) -> Option<Damage> {
        let drag = self.drag?;
        let box_id = match drag.taken {
            Some(Some(box_id)) => box_id,
            Some(None) => return None,
            None => {
                let (dx, dy) = (x - drag.start.0, y - drag.start.1);
                if dx.abs().max(dy.abs()) < SLOP {
                    return None;
                }
                let (sx, sy) = drag.start;
                let scroll = self.scroll_param();
                let hit = self.view.hover_box_id_scrolled(
                    &self.layout.boxes,
                    "DragStart",
                    nexus_layout_types::FxPx::new(sx),
                    nexus_layout_types::FxPx::new(sy),
                    scroll,
                );
                self.drag = Some(Drag { taken: Some(hit), ..drag });
                let box_id = hit?;
                set_cell(x, y, drag.start);
                let started = self.dispatch(box_id, "DragStart");
                return started.max(self.dispatch(box_id, "DragMove"));
            }
        };
        set_cell(x, y, drag.start);
        self.dispatch(box_id, "DragMove")
    }

    /// The button went up: the drag moves to the release point (it supersedes a noted position;
    /// motion frames may have been lost) and ends there — one layout for both.
    fn drag_end(
        &mut self,
        x: i32,
        y: i32,
        dirty: &mut bool,
        dirty_rows: &mut Option<(i32, i32)>,
    ) -> bool {
        if let Some(drag) = self.drag.as_mut() {
            drag.pending = None;
        }
        let mut damage = self.drag_move(x, y);
        let Some(drag) = self.drag.take() else { return self.settle(damage, dirty, dirty_rows) };
        if let Some(Some(box_id)) = drag.taken {
            set_cell(x, y, drag.start);
            damage = damage.max(self.dispatch(box_id, "DragEnd"));
        }
        let repaint = self.settle(damage, dirty, dirty_rows);
        self.trace_drag(drag, (x, y));
        set_cell(0, 0, (0, 0));
        repaint
    }

    /// ONE line per drag gesture — where it started, where it let go, the box that took
    /// `DragStart` (`None`: no box starts a drag there). Bounded like the tap trace; a plain
    /// tap (no drag) and the keyboard overlay say nothing.
    fn trace_drag(&self, drag: Drag, end: (i32, i32)) {
        static DRAGS: AtomicU32 = AtomicU32::new(0);
        let Some(hit) = drag.taken else { return };
        if self.taps_are_keystrokes || DRAGS.fetch_add(1, Ordering::Relaxed) >= 8 {
            return;
        }
        let (sx, sy) = drag.start;
        let (ex, ey) = end;
        super::raw_marker(&alloc::format!("apphost: drag ({sx},{sy})->({ex},{ey}) hit={hit:?}"));
    }

    /// Dispatches `trigger` on `box_id`: the store moves and the scene re-emits; the layout waits
    /// for `settle`.
    fn dispatch(&mut self, box_id: usize, trigger: &str) -> Option<Damage> {
        let tokens = super::tokens_for(self.theme_mode);
        let device = super::device_for(
            self.shell_profile,
            self.w,
            &self.locale_tag,
            &self.keymap,
            self.theme_mode,
        );
        let locale = super::app_locale!(self);
        self.view
            .fire_on_box(tokens, &device, &locale, &mut self.host, box_id, trigger)
            .ok()
            .flatten()
    }

    /// Lays out what the dispatches changed and folds the damage into the pending repaint: a
    /// layout change repaints the rows the layout diff names (glass regions re-declared only
    /// when a glass root moved), a paint change the frame. `true` when the model changed.
    fn settle(
        &mut self,
        damage: Option<Damage>,
        dirty: &mut bool,
        dirty_rows: &mut Option<(i32, i32)>,
    ) -> bool {
        use crate::layout_diff::{changed_row_span, glass_roots_changed, merge_span};
        match damage {
            Some(Damage::Layout) => {
                let old_boxes = core::mem::take(&mut self.layout.boxes);
                let old_texts = core::mem::take(&mut self.texts);
                self.relayout_retained();
                self.layers_dirty |= glass_roots_changed(&old_boxes, &self.layout.boxes);
                let (boxes, texts, h) = (&self.layout.boxes, &self.texts, self.h as i32);
                match changed_row_span(&old_boxes, boxes, &old_texts, texts, h) {
                    None => (*dirty, *dirty_rows) = (true, None),
                    Some((y0, y1)) if y1 > y0 => merge_span(dirty, dirty_rows, (y0, y1)),
                    Some(_) => {} // the store moved, no pixel did
                }
            }
            Some(Damage::Paint) => (*dirty, *dirty_rows) = (true, None),
            _ => return false,
        }
        // The dispatch re-emitted the scene: a changed `.animate` value starts its motion.
        self.anim_sync();
        true
    }
}
