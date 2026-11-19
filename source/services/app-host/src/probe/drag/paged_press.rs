// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A press inside an uncovered paged viewport (TASK-0354 P12, the launcher's pages): it ends as
//! exactly one of a `Tap` at the release or a drag of the track — `pager_math::gesture_end`
//! decides, the host-tested function. The feedback shows at the press as anywhere else; the Tap
//! waits (`hold_tap`). Past 6 px the axis locks (`pager_math::axis_lock`): along the track the
//! feedback lets go and the track follows (`pager_drag`), across it the press is an ordinary
//! drag. A release of the track turns or returns it (`pager_release`); a leave returns it.

use super::Drag;
use crate::pager_math::{axis_lock, gesture_end, GestureEnd, Lock};
use crate::probe::interaction::TapOutcome;
use nexus_dsl_runtime::Damage;

impl crate::probe::DslApp {
    /// A tap on `target` (the handler and its animated part) that landed in a paged viewport is
    /// held for the release; `false` outside one (the tap dispatches now, as always).
    pub(in crate::probe) fn hold_tap(&mut self, target: (Option<usize>, Option<usize>)) -> bool {
        let Some(drag) = self.drag.filter(|d| d.paged.is_some()) else { return false };
        self.drag = Some(Drag { held: Some(target), ..drag });
        true
    }

    /// A move of a press in a paged viewport: `Some(damage)` when the track decided it (still
    /// undecided, or locked along the track), `None` when it is an ordinary drag (across the
    /// track, or no paged viewport) — the caller's path runs.
    pub(super) fn track_move(&mut self, drag: Drag, x: i32, y: i32) -> Option<Option<Damage>> {
        let k = drag.paged?;
        let (dx, dy) = (x - drag.start.0, y - drag.start.1);
        let lock = match drag.lock {
            Lock::Undecided => axis_lock(dx, dy),
            locked => locked,
        };
        match lock {
            Lock::Undecided => Some(None),
            Lock::Cross => {
                self.drag = Some(Drag { lock, ..drag });
                None
            }
            Lock::Track => {
                if drag.lock != Lock::Track {
                    // The press is no tap any more: its feedback lets go.
                    self.drag = Some(Drag { lock, ..drag });
                    self.interaction_release();
                }
                self.pager_drag(k, dx);
                Some(None)
            }
        }
    }

    /// The release of a press in a paged viewport at `(x, y)`: the track turns or returns, or
    /// the held Tap dispatches at the press point, or nothing — never two. Whether the model
    /// changed.
    pub(super) fn track_end(&mut self, drag: Drag, (x, _): (i32, i32)) -> bool {
        let Some(k) = drag.paged else { return false };
        let (sx, sy) = drag.start;
        match gesture_end(drag.lock, drag.held.is_some()) {
            GestureEnd::Track => {
                let dt = super::super::nsec_now().saturating_sub(drag.t0);
                self.dispatched(|d| d.pager_release(k, x - sx, dt))
            }
            GestureEnd::Tap => {
                let Some((hit, part)) = drag.held else { return false };
                let outcome = self.tap_dispatch(sx, sy, hit, part);
                // A part press the dispatch started (the toggle's thumb) lets go with the button.
                let released = self.interaction_release();
                outcome == TapOutcome::Repainted || released
            }
            GestureEnd::Nothing => false,
        }
    }

    /// The pointer left the surface: a held Tap is dropped, a dragged track returns to its page.
    pub(super) fn track_leave(&mut self) -> bool {
        let Some(drag) = self.drag.filter(|d| d.paged.is_some()) else { return false };
        match (drag.lock, drag.paged) {
            (Lock::Track, Some(k)) => {
                self.drag = None;
                self.dispatched(|d| d.pager_release(k, 0, u64::MAX))
            }
            _ => {
                self.drag = Some(Drag { held: None, ..drag });
                false
            }
        }
    }
}
