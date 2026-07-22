// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The read-only device environment fixture (docs/dev/dsl/
//! profiles.md) — host-injectable per golden variant, host-side presets +
//! the runtime-varying region axes (locale/keymap, RFC-0075 Phase 8b).
//! Field ids index `nexus-dsl-core::registry::DEVICE_FIELDS` (moved out of
//! `lib.rs`, structure gate).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: exercised by every golden/conformance mount.

use alloc::string::String;

use crate::{DeviceEnv, Value};

/// Fixture environment: the full read-only device contract
/// (docs/dev/dsl/profiles.md), host-injectable per golden variant. Field ids
/// index `nexus-dsl-core::registry::DEVICE_FIELDS`:
/// 0 profile, 1 posture, 2 orientation, 3 shellMode, 4 sizeClass,
/// 5 dpiClass, 6 input, 7 locale, 8 keymap.
pub struct FixtureEnv {
    pub profile: &'static str,
    pub posture: &'static str,
    pub orientation: &'static str,
    pub shell_mode: &'static str,
    pub size_class: &'static str,
    pub dpi_class: &'static str,
    /// Input capability names present on this device (`touch`, `mouse`, …).
    pub input: &'static [&'static str],
    /// The active locale tag (`de-DE`, …; RFC-0077) — runtime-varying,
    /// hosts fill it from the region push. Empty until pushed.
    pub locale: String,
    /// The active keymap layout tag (`us`/`de`/`jp`/…; RFC-0075 Phase 8b).
    pub keymap: String,
}

impl Default for FixtureEnv {
    fn default() -> Self {
        Self::desktop()
    }
}

impl FixtureEnv {
    #[must_use]
    pub fn desktop() -> Self {
        Self {
            profile: "desktop",
            posture: "",
            orientation: "landscape",
            shell_mode: "desktop",
            size_class: "wide",
            dpi_class: "normal",
            input: &["mouse", "kbd", "touch"],
            locale: String::new(),
            keymap: String::new(),
        }
    }

    #[must_use]
    pub fn phone(orientation: &'static str) -> Self {
        Self {
            profile: "phone",
            posture: "",
            orientation,
            shell_mode: "phone",
            size_class: "compact",
            dpi_class: "high",
            input: &["touch"],
            locale: String::new(),
            keymap: String::new(),
        }
    }

    #[must_use]
    pub fn tablet(orientation: &'static str) -> Self {
        Self {
            profile: "tablet",
            posture: "",
            orientation,
            shell_mode: "tablet",
            // The touch width classes (design_handoff_launcher): landscape
            // (≥1024) = wide, portrait = regular. Hosts override from the
            // REAL surface width; this preset mirrors that mapping.
            size_class: if matches!(orientation, "landscape") { "wide" } else { "regular" },
            dpi_class: "high",
            input: &["touch", "kbd"],
            locale: String::new(),
            keymap: String::new(),
        }
    }

    /// A convertible in an explicit shell mode (`desktop` or `tablet`).
    #[must_use]
    pub fn convertible(shell_mode: &'static str) -> Self {
        Self {
            profile: "convertible",
            posture: "",
            orientation: "landscape",
            shell_mode,
            size_class: "regular",
            dpi_class: "normal",
            input: &["touch", "mouse", "kbd"],
            locale: String::new(),
            keymap: String::new(),
        }
    }
}

impl DeviceEnv for FixtureEnv {
    fn get(&self, field_id: u32) -> Value {
        match field_id {
            0 => Value::Str(String::from(self.profile)),
            1 => Value::Str(String::from(self.posture)),
            2 => Value::Str(String::from(self.orientation)),
            3 => Value::Str(String::from(self.shell_mode)),
            4 => Value::Str(String::from(self.size_class)),
            5 => Value::Str(String::from(self.dpi_class)),
            6 => {
                Value::List(self.input.iter().map(|name| Value::Str(String::from(*name))).collect())
            }
            7 => Value::Str(self.locale.clone()),
            8 => Value::Str(self.keymap.clone()),
            _ => Value::Str(String::new()),
        }
    }
}
