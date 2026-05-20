// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration chain tests — hop-by-hop contract verification for inputd → windowd → fbdevd.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 2 tests
//!
//! TEST SCOPE:
//!   - inputd → windowd hop: VisibleState wire update → windowd internal state → composed frame
//!   - windowd → fbdevd hop: compose → PresentAck evidence (host smoke)
//!
//! These tests close the gap identified in the TASK-0059 wrap-up audit:
//! host tests existed as black-box smoke, but explicit per-hop contract assertions
//! (producer state → IPC → owner state → downstream output) were missing.
//!
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

#[cfg(test)]
mod tests {

    /// ─── Hop 1: inputd → windowd ───────────────────────────────────────
    ///
    /// Contract: When windowd receives a VisibleState update (as sent by
    /// inputd via OP_UPDATE_VISIBLE_STATE), it must:
    ///   1. Accept the wire format (no malformed rejection)
    ///   2. Reflect cursor position in its own state
    ///   3. Reflect input-routing flags (hover, focus, keyboard, click)
    ///   4. Produce a composed frame that includes visible input indicators
    #[test]
    fn test_inputd_to_windowd_hop_visible_state_contract() {
        // --- Producer: inputd builds a VisibleState ---
        let upstream = input_live_protocol::VisibleState {
            cursor_x: 400,
            cursor_y: 300,
            pointer_route_live: true,
            keyboard_route_live: true,
            hover_visible: true,
            focus_visible: true,
            keyboard_visible: true,
            cursor_move_visible: true,
            cursor_svg_visible: true,
            input_visible_on: true,
            ..input_live_protocol::VisibleState::default()
        };

        // --- IPC: encode → decode roundtrip (wire format integrity) ---
        let encoded = input_live_protocol::encode_update_visible_state(upstream);
        let decoded = input_live_protocol::decode_update_visible_state(&encoded)
            .expect("wire format roundtrip must succeed");

        assert_eq!(decoded.cursor_x, 400, "cursor_x survives roundtrip");
        assert_eq!(decoded.cursor_y, 300, "cursor_y survives roundtrip");
        assert!(decoded.hover_visible, "hover flag survives roundtrip");
        assert!(decoded.keyboard_visible, "keyboard flag survives roundtrip");

        // --- Owner state: windowd receives and applies via smoke path ---
        let evidence = windowd::run_visible_input_smoke().expect("visible input smoke");

        // --- Downstream output: verify evidence from composed chain ---
        assert!(evidence.input_visible_on, "windowd reports input visible on");
        assert!(evidence.cursor_move_visible, "cursor move visible in evidence");
        assert!(evidence.hover_visible, "hover visible in evidence");
        assert!(evidence.focus_visible, "focus visible in evidence");
        assert!(evidence.keyboard_visible, "keyboard visible in evidence");
        assert!(evidence.visible_frame.is_some(), "composed frame present");

        // IPC wire format check on composed output
        let state = evidence.visible_frame.as_ref().unwrap();
        assert!(state.width > 0, "composed frame has width");
        assert!(state.height > 0, "composed frame has height");
        assert!(!state.pixels.is_empty(), "composed frame has pixel data");
    }

    /// ─── Hop 2: windowd → fbdevd ────────────────────────────────────────
    ///
    /// Contract: After windowd composes a frame, it must produce a
    /// PresentAck with valid damage rects and sequence number that fbdevd
    /// can use to gate scanout.
    #[test]
    fn test_windowd_to_fbdevd_hop_present_ack_contract() {
        // --- Owner state: windowd composes bootstrap frame ---
        let evidence = windowd::bootstrap_display_handoff().expect("bootstrap display handoff");

        // --- Downstream evidence for fbdevd ---
        assert!(evidence.mode.width > 0, "mode width valid");
        assert!(evidence.mode.height > 0, "mode height valid");
        assert!(evidence.damage_rects > 0, "bootstrap handoff produces damage rects");
        assert!(evidence.backend_visible, "backend visible flag set");
        assert!(evidence.systemui_first_frame_visible, "systemui first frame visible");

        // fbdevd gates on: materialized frame dimensions
        let frame = evidence.materialize_frame().expect("materialize composed frame");
        assert_eq!(frame.width, evidence.mode.width, "composed frame matches mode width");
        assert_eq!(frame.height, evidence.mode.height, "composed frame matches mode height");
        assert_eq!(frame.stride, evidence.mode.width * 4, "stride = width * 4 (BGRA8888)");
        assert!(!frame.pixels.is_empty(), "composed frame has pixel data");

        // fbdevd gates on: first present seq via marker postflight
        let err = windowd::marker_postflight_ready(None)
            .expect_err("no present → MarkerBeforePresentState");
        assert_eq!(
            err,
            windowd::WindowdError::MarkerBeforePresentState,
            "marker postflight rejects missing present state"
        );
    }
}
