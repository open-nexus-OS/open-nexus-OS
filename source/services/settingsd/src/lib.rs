// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `settingsd` — the typed settings registry service (TASK-0072
//! Phase 8). Every setting is a registered key with a default + validator;
//! `set` is validate → persist (statefsd `state:/prefs/device.nxs`) → apply,
//! and the store is loaded back at boot so values survive a reboot. The legacy
//! `InputSettingsSnapshot` (TASK-0253 input seam, host-only) rides along.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! ADR: docs/adr/0011-settings-architecture.md

#![cfg_attr(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"), no_std)]

extern crate alloc;

/// Typed settings registry (TASK-0072 Phase 8): registered keys + defaults +
/// validation + prefs-blob (de)serialization — the core the service loop and
/// the statefsd persistence wrap.
pub mod registry;
pub mod watch;

/// OS-lite service runtime: binds the settingsd server, loads persisted prefs,
/// serves GET/SET (validate → persist → apply). Boot service (RFC-0069).
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub mod os_lite;
/// statefsd persistence client — loads/stores the prefs blob at
/// `state:/prefs/device.nxs`.
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod statefs_client;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use os_lite::{service_main_loop, SettingsdError, SettingsdResult};

// ── Legacy input-settings snapshot (TASK-0253, host-only) ────────────────────
// Kept for the inputd contract validation; no external runtime consumer.

#[cfg(feature = "std")]
use std::string::String;

#[cfg(feature = "std")]
pub const KEYBOARD_LAYOUT_KEY: &str = "keyboard.layout";
#[cfg(feature = "std")]
pub const KEYBOARD_REPEAT_DELAY_KEY: &str = "keyboard.repeat.delay_ms";
#[cfg(feature = "std")]
pub const KEYBOARD_REPEAT_RATE_KEY: &str = "keyboard.repeat.rate_hz";
#[cfg(feature = "std")]
pub const POINTER_ACCEL_THRESHOLD_KEY: &str = "pointer.accel.threshold";
#[cfg(feature = "std")]
pub const POINTER_ACCEL_RATIO_KEY: &str = "pointer.accel.ratio";
#[cfg(feature = "std")]
pub const POINTER_ACCEL_MAX_OUTPUT_KEY: &str = "pointer.accel.max_output";

#[cfg(feature = "std")]
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

#[cfg(feature = "std")]
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

#[cfg(feature = "std")]
impl Default for InputSettingsSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

/// Legacy CLI entry (host builds only). The OS boot service is
/// [`service_main_loop`].
#[cfg(feature = "std")]
pub fn run() {
    settings::run();
}

#[cfg(all(test, feature = "std"))]
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
