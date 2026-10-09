// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the modal edge app-host mirrors before each paint (TASK-0074, ADR-0068): which
//! marker to say and which windowd verb to send, from the depth and the topmost modal's
//! identity last synced and now. A modal that closed and ANOTHER that opened before one paint
//! (ESC on the search, then Print opening the screenshot tool — TASK-0068) keep the depth; the
//! identity tells the new one. Pure, so that race is a host test.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below; the live ladders' `apphost: modal open` / `windowd: win modal`

/// What one sync owes the outside world.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ModalEdge {
    /// `apphost: modal open (depth=N)` — a modal opened: a rise, or a replaced top.
    pub(crate) opened: bool,
    /// `Some(on)` — windowd's verb on the 0↔n edge (modal across the owner's windows).
    pub(crate) verb: Option<bool>,
}

/// The edge between the synced `(depth, top identity)` and the scene's now.
pub(crate) fn modal_edge<T: PartialEq>(
    sent: (usize, Option<T>),
    now: (usize, Option<T>),
) -> ModalEdge {
    let ((sent_depth, sent_top), (depth, top)) = (sent, now);
    let replaced = depth > 0 && depth == sent_depth && top != sent_top;
    ModalEdge {
        opened: depth > sent_depth || replaced,
        verb: ((depth > 0) != (sent_depth > 0)).then_some(depth > 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rise_opens_and_turns_windowd_modal() {
        assert_eq!(
            modal_edge((0, None), (1, Some("alert"))),
            ModalEdge { opened: true, verb: Some(true) }
        );
        assert_eq!(
            modal_edge((1, Some("alert")), (2, Some("confirm"))),
            ModalEdge { opened: true, verb: None }
        );
    }

    #[test]
    fn a_close_says_no_open_and_turns_windowd_off_at_zero() {
        assert_eq!(
            modal_edge((1, Some("alert")), (0, None)),
            ModalEdge { opened: false, verb: Some(false) }
        );
        assert_eq!(
            modal_edge((2, Some("confirm")), (1, Some("alert"))),
            ModalEdge { opened: false, verb: None }
        );
    }

    /// The 2026-10-09 race: the search closed (ESC) and the screenshot tool opened before one
    /// paint — same depth, another modal: the open is named, windowd stays modal (no verb).
    #[test]
    fn a_replaced_top_modal_is_an_open_without_a_verb() {
        assert_eq!(
            modal_edge((1, Some("search")), (1, Some("capture"))),
            ModalEdge { opened: true, verb: None }
        );
    }

    #[test]
    fn test_reject_an_unchanged_scene_owes_nothing() {
        assert_eq!(
            modal_edge((1, Some("search")), (1, Some("search"))),
            ModalEdge { opened: false, verb: None }
        );
        assert_eq!(
            modal_edge::<&str>((0, None), (0, None)),
            ModalEdge { opened: false, verb: None }
        );
    }
}
