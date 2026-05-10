// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `settingsd` service-side input snapshot seam for TASK-0253.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 unit tests.
//! ADR: docs/adr/0011-settings-architecture.md

use std::string::String;

pub const KEYBOARD_LAYOUT_KEY: &str = "keyboard.layout";
pub const KEYBOARD_REPEAT_DELAY_KEY: &str = "keyboard.repeat.delay_ms";
pub const KEYBOARD_REPEAT_RATE_KEY: &str = "keyboard.repeat.rate_hz";
pub const POINTER_ACCEL_THRESHOLD_KEY: &str = "pointer.accel.threshold";
pub const POINTER_ACCEL_RATIO_KEY: &str = "pointer.accel.ratio";
pub const POINTER_ACCEL_MAX_OUTPUT_KEY: &str = "pointer.accel.max_output";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputSettingsSnapshot {
    keyboard_layout: String,
    repeat_delay_ms: u32,
    repeat_rate_hz: u16,
    pointer_threshold: i32,
    pointer_numerator: i32,
    pointer_denominator: i32,
    pointer_max_output: i32,
}

impl InputSettingsSnapshot {
    #[must_use]
    pub fn new() -> Self {
        Self {
            keyboard_layout: "de".to_string(),
            repeat_delay_ms: 100,
            repeat_rate_hz: 10,
            pointer_threshold: 64,
            pointer_numerator: 1,
            pointer_denominator: 1,
            pointer_max_output: 96,
        }
    }

    pub fn set_keyboard_layout(&mut self, layout: impl Into<String>) {
        self.keyboard_layout = layout.into();
    }

    pub fn set_repeat(&mut self, delay_ms: u32, rate_hz: u16) {
        self.repeat_delay_ms = delay_ms;
        self.repeat_rate_hz = rate_hz;
    }

    pub fn set_pointer_accel(
        &mut self,
        threshold: i32,
        numerator: i32,
        denominator: i32,
        max_output: i32,
    ) {
        self.pointer_threshold = threshold;
        self.pointer_numerator = numerator;
        self.pointer_denominator = denominator;
        self.pointer_max_output = max_output;
    }

    pub fn to_inputd_config(
        &self,
        queue_capacity: usize,
        initial_pointer_x: i32,
        initial_pointer_y: i32,
    ) -> Result<inputd::InputdConfig, inputd::InputdError> {
        inputd::InputdConfig::new(
            self.keyboard_layout.as_str(),
            self.repeat_delay_ms,
            self.repeat_rate_hz,
            self.pointer_threshold,
            self.pointer_numerator,
            self.pointer_denominator,
            self.pointer_max_output,
            queue_capacity,
            initial_pointer_x,
            initial_pointer_y,
        )
    }

    #[must_use]
    pub fn keyboard_layout(&self) -> &str {
        self.keyboard_layout.as_str()
    }

    #[must_use]
    pub const fn canonical_keys() -> [&'static str; 6] {
        [
            KEYBOARD_LAYOUT_KEY,
            KEYBOARD_REPEAT_DELAY_KEY,
            KEYBOARD_REPEAT_RATE_KEY,
            POINTER_ACCEL_THRESHOLD_KEY,
            POINTER_ACCEL_RATIO_KEY,
            POINTER_ACCEL_MAX_OUTPUT_KEY,
        ]
    }
}

impl Default for InputSettingsSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

pub fn run() {
    settings::run();
}

#[cfg(test)]
mod tests {
    use super::InputSettingsSnapshot;

    #[test]
    fn input_snapshot_validates_against_inputd_contract() {
        let snapshot = InputSettingsSnapshot::default();
        let config = snapshot.to_inputd_config(16, 12, 12).expect("valid config");
        assert_eq!(snapshot.keyboard_layout(), "de");
        assert_eq!(config.layout(), keymaps::LayoutId::De);
    }

    #[test]
    fn test_reject_invalid_layout_update() {
        let mut snapshot = InputSettingsSnapshot::default();
        snapshot.set_keyboard_layout("neo");
        let err = snapshot.to_inputd_config(16, 12, 12).expect_err("unknown layout must reject");
        assert_eq!(err.code(), "keymap.layout.unknown");
    }

    #[test]
    fn canonical_keys_match_task_contract() {
        assert_eq!(
            InputSettingsSnapshot::canonical_keys(),
            [
                super::KEYBOARD_LAYOUT_KEY,
                super::KEYBOARD_REPEAT_DELAY_KEY,
                super::KEYBOARD_REPEAT_RATE_KEY,
                super::POINTER_ACCEL_THRESHOLD_KEY,
                super::POINTER_ACCEL_RATIO_KEY,
                super::POINTER_ACCEL_MAX_OUTPUT_KEY,
            ]
        );
    }
}
