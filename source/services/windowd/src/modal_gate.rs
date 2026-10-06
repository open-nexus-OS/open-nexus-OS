// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the app-modal routing gate (TASK-0074 D4, RFC-0067 boundary): windowd's ONE modal
//! verb — `CONTROL_WIN_MODAL` — marks a window as modal for its owner; while it is, input
//! (press, hover, wheel) to the owner's OTHER windows is refused, so an app-modal overlay cannot
//! be bypassed through a sibling window of the same app. The modal window itself and every
//! other owner's windows are untouched; windowd draws nothing. Pure over the slots' facts
//! (owner sid, modal flag, live surface) — host-tested, driven by the routing loops.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: inline + `tests/modal_routing.rs`

/// One window slot as the gate sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotFacts {
    /// The sender service id captured at surface create (0 = none).
    pub owner_sid: u64,
    /// `CONTROL_WIN_MODAL{on}` set and not reset.
    pub modal: bool,
    /// A live surface (a free or re-creating slot is nobody's window).
    pub live: bool,
}

/// Whether input to window `target` passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    /// Refused: another live window of the same owner (`modal_idx`) is modal.
    RefusedByModal {
        modal_idx: usize,
    },
}

/// Input to `target` is refused when ANOTHER live window of the same non-zero owner is modal.
/// A modal window takes its own input; windows of other owners (and of owner 0, which never
/// identifies anyone) are never gated; a target that is not live is nobody's window.
#[must_use]
pub fn input_verdict(target: usize, slots: &[SlotFacts]) -> Verdict {
    let Some(t) = slots.get(target) else { return Verdict::Allow };
    if !t.live || t.owner_sid == 0 || t.modal {
        return Verdict::Allow;
    }
    match slots
        .iter()
        .enumerate()
        .find(|(i, s)| *i != target && s.live && s.modal && s.owner_sid == t.owner_sid)
    {
        Some((modal_idx, _)) => Verdict::RefusedByModal { modal_idx },
        None => Verdict::Allow,
    }
}

/// Proof bookkeeping for the two `SELFTEST: ui v10 …` rungs — a pure edge counter: the
/// DIALOG rung fires on the first complete on→off round trip; the LIVE rung on the first
/// round trip during which at least one press was routed to the modal window while a live
/// input route was on (the injector's pointer opened it, its keyboard closed it). Each fires
/// once per boot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModalProof {
    on: bool,
    presses_while_on: u32,
    dialog_said: bool,
    live_said: bool,
}

impl ModalProof {
    /// A press landed on a modal window (counted only while the flag is on).
    pub fn press(&mut self) {
        if self.on {
            self.presses_while_on = self.presses_while_on.saturating_add(1);
        }
    }

    /// The flag's edge; returns `(say_dialog_ok, say_live_modal_ok)` — each at most once.
    pub fn edge(&mut self, on: bool, live_route: bool) -> (bool, bool) {
        if on {
            self.on = true;
            self.presses_while_on = 0;
            return (false, false);
        }
        if !self.on {
            return (false, false);
        }
        self.on = false;
        let dialog = !self.dialog_said;
        self.dialog_said = true;
        let live = !self.live_said && live_route && self.presses_while_on > 0;
        if live {
            self.live_said = true;
        }
        (dialog, live)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn slot(owner_sid: u64, modal: bool, live: bool) -> SlotFacts {
        SlotFacts { owner_sid, modal, live }
    }

    #[test]
    fn a_sibling_of_a_modal_window_is_refused_and_everyone_else_passes() {
        let slots = [slot(7, true, true), slot(7, false, true), slot(9, false, true)];
        assert_eq!(input_verdict(1, &slots), Verdict::RefusedByModal { modal_idx: 0 });
        assert_eq!(input_verdict(0, &slots), Verdict::Allow, "the modal takes its own input");
        assert_eq!(input_verdict(2, &slots), Verdict::Allow, "another owner");
    }

    #[test]
    fn test_reject_nothing_without_a_live_modal_or_with_owner_zero() {
        let dead_modal = [slot(7, true, false), slot(7, false, true)];
        assert_eq!(input_verdict(1, &dead_modal), Verdict::Allow);
        let owner_zero = [slot(0, true, true), slot(0, false, true)];
        assert_eq!(input_verdict(1, &owner_zero), Verdict::Allow);
        let free_target = [slot(7, true, true), slot(7, false, false)];
        assert_eq!(input_verdict(1, &free_target), Verdict::Allow);
        assert_eq!(input_verdict(5, &free_target), Verdict::Allow, "no such slot");
    }

    #[test]
    fn proof_rungs_fire_once_on_a_round_trip_and_live_only_with_presses() {
        let mut p = ModalProof::default();
        assert_eq!(p.edge(false, true), (false, false), "off without on is nothing");
        assert_eq!(p.edge(true, true), (false, false));
        assert_eq!(p.edge(false, true), (true, false), "a round trip without a press: dialog only");
        assert_eq!(p.edge(true, true), (false, false));
        p.press();
        assert_eq!(p.edge(false, false), (false, false), "no live route: no live rung");
        assert_eq!(p.edge(true, true), (false, false));
        p.press();
        assert_eq!(p.edge(false, true), (false, true));
        assert_eq!(p.edge(true, true), (false, false));
        p.press();
        assert_eq!(p.edge(false, true), (false, false), "each rung once per boot");
    }
}
