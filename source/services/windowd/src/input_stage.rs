// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd's frame-aligned input staging (ADR-0028, the consumer half of the
//! frame-aligned input model): inputd pushes one `VisibleState` sample per input event and
//! windowd applies ONE sample per frame — motion coalesces (the newest position wins), wheel
//! notches sum, a one-shot fact (a tiling chord, a capture key) survives a newer sample. A
//! primary-button EDGE is never coalesced (RFC-0055's semantic-edge integrity: motion bursts
//! may coalesce, a click stays individually observable): the sample that changes the button's
//! level is applied as it came, at its own position, before anything newer is staged over it.
//! Before, a click held for less than one busy windowd pass (press and release drained in the
//! same IPC batch) folded into its release — the newest sample won and windowd never saw the
//! press; and motion queued behind a press moved the press to where the pointer was a frame
//! later. Batching motion but never a button change is Android's input-consumer rule (moves
//! batch; a down or an up ends the batch).
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests/input_staging.rs`

use input_live_protocol::VisibleState;

/// The frame's staged input sample, and the button level of the last sample handed out.
#[derive(Debug, Default)]
pub struct InputStage {
    pending: Option<VisibleState>,
    /// The primary-button level of the last sample handed to the applier (`stage`'s flush or
    /// `take`): the level the staged sample is an edge against.
    applied_held: bool,
}

/// The primary button's level in a sample (the field keeps its bootstrap-era name).
const fn held(state: &VisibleState) -> bool {
    state.launcher_click_visible
}

impl InputStage {
    #[must_use]
    pub const fn new() -> Self {
        Self { pending: None, applied_held: false }
    }

    /// Stages `state` for the frame. Returns the sample staged before it when that sample
    /// changes the button's level (a press or a release): an edge ends the batch, and the
    /// caller applies the returned sample NOW, before the frame applies `state`. A sample at
    /// the same level is motion — `state` replaces it and carries its wheel notches and
    /// one-shot facts.
    #[must_use]
    pub fn stage(&mut self, mut state: VisibleState) -> Option<VisibleState> {
        let flushed = match self.pending.take() {
            Some(prev) if held(&prev) != self.applied_held => {
                self.applied_held = held(&prev);
                Some(prev)
            }
            Some(prev) => {
                state.wheel_delta_y = state.wheel_delta_y.saturating_add(prev.wheel_delta_y);
                if state.wm_chord == 0 {
                    state.wm_chord = prev.wm_chord;
                }
                if state.capture == 0 {
                    state.capture = prev.capture;
                }
                None
            }
            None => None,
        };
        self.pending = Some(state);
        flushed
    }

    /// The frame's sample, once (`None` when nothing was staged since the last frame).
    pub fn take(&mut self) -> Option<VisibleState> {
        let state = self.pending.take()?;
        self.applied_held = held(&state);
        Some(state)
    }
}
