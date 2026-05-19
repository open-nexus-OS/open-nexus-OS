// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shared visible-state push budget helpers for the live `inputd` path.
//! OWNERS: @runtime @ui
//! STATUS: Experimental
//! TEST_COVERAGE: Host unit tests in this module

use input_live_protocol::VisibleState;

pub(crate) const POINTER_PUSH_INTERVAL_NS: u64 = 8_000_000;

pub(crate) fn should_push_visible_state(
    previous: Option<VisibleState>,
    next: VisibleState,
    last_push_ns: u64,
    now_ns: u64,
    immediate: bool,
) -> bool {
    let Some(previous) = previous else {
        return true;
    };
    if previous == next {
        return false;
    }
    if immediate || !pointer_only_change(previous, next) {
        return true;
    }
    now_ns.saturating_sub(last_push_ns) >= POINTER_PUSH_INTERVAL_NS
}

fn pointer_only_change(previous: VisibleState, next: VisibleState) -> bool {
    let mut lhs = previous;
    let mut rhs = next;
    lhs.cursor_x = 0;
    lhs.cursor_y = 0;
    lhs.cursor_move_visible = false;
    lhs.hover_visible = false;
    rhs.cursor_x = 0;
    rhs.cursor_y = 0;
    rhs.cursor_move_visible = false;
    rhs.hover_visible = false;
    lhs == rhs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pushes_first_state_immediately() {
        assert!(should_push_visible_state(None, VisibleState::default(), 0, 0, false));
    }

    #[test]
    fn skips_duplicate_state() {
        let state = VisibleState { cursor_x: 10, cursor_y: 20, ..VisibleState::default() };
        assert!(!should_push_visible_state(Some(state), state, 10, 20, false));
    }

    #[test]
    fn throttles_pointer_only_motion_inside_budget() {
        let previous = VisibleState { cursor_x: 10, cursor_y: 20, ..VisibleState::default() };
        let next = VisibleState {
            cursor_x: 12,
            cursor_y: 24,
            cursor_move_visible: true,
            hover_visible: true,
            ..VisibleState::default()
        };
        assert!(!should_push_visible_state(Some(previous), next, 100, 100 + 1_000_000, false));
        assert!(should_push_visible_state(
            Some(previous),
            next,
            100,
            100 + POINTER_PUSH_INTERVAL_NS,
            false,
        ));
    }

    #[test]
    fn semantic_updates_bypass_pointer_budget() {
        let previous = VisibleState::default();
        let next = VisibleState { keyboard_visible: true, ..VisibleState::default() };
        assert!(should_push_visible_state(Some(previous), next, 100, 101, true));
    }
}
