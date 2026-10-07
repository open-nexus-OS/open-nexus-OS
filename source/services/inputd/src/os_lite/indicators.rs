// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: inputd os-lite — the wheel direction indicator (the visible-state bits the
//! wheel markers read) and the plain HID-batch RX-rate telemetry line. A pure move out of
//! `os_lite.rs` under the structure ratchet, no behavior change.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal

use super::*;

impl LiveRouteRuntime {
    pub(super) fn note_wheel_indicator(&mut self, delta_y: i32, now_ns: u64) {
        self.wheel_indicator_direction = if delta_y > 0 {
            WheelIndicatorDirection::Up
        } else if delta_y < 0 {
            WheelIndicatorDirection::Down
        } else {
            WheelIndicatorDirection::None
        };
        self.wheel_indicator_deadline_ns = now_ns.saturating_add(WHEEL_INDICATOR_PULSE_NS);
    }

    pub(super) fn sync_wheel_indicator(&mut self, now_ns: u64) {
        let active = now_ns <= self.wheel_indicator_deadline_ns;
        self.visible_state.wheel_up_visible =
            active && self.wheel_indicator_direction == WheelIndicatorDirection::Up;
        self.visible_state.wheel_down_visible =
            active && self.wheel_indicator_direction == WheelIndicatorDirection::Down;
        if !active {
            self.wheel_indicator_direction = WheelIndicatorDirection::None;
        }
    }

    /// Plain (unfolded) HID-batch RX-rate line, >=8/s gate — pairs with
    /// `hidrawd: tx hz` and `inputd: push hz` to localize input-rate loss.
    pub(super) fn note_hid_rx_for_rate_line(&mut self) {
        let now_ns = nexus_abi::nsec().unwrap_or(0);
        self.hid_rx_rate_count = self.hid_rx_rate_count.saturating_add(1);
        if self.hid_rx_rate_window_ns == 0 {
            self.hid_rx_rate_window_ns = now_ns;
        } else if now_ns.saturating_sub(self.hid_rx_rate_window_ns) >= 1_000_000_000 {
            let (dx, dy, batches) = self.input.take_travel();
            if self.hid_rx_rate_count >= 8 {
                // The travel the relative pointer covered this second (|dx| and |dy| summed over
                // the batches with motion): against the device's counts, the lost distance.
                let _ = debug_println(&format!(
                    "inputd: hid rx hz={} travel dx={dx} dy={dy} rel={batches}",
                    self.hid_rx_rate_count
                ));
            }
            self.hid_rx_rate_window_ns = now_ns;
            self.hid_rx_rate_count = 0;
        }
    }
}
