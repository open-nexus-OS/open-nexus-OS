// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The typed settings registry (TASK-0072 Phase 8, TASK-0225
//! vocabulary): every key is REGISTERED with a default and a validator;
//! `set` is validate-then-store (never a partial write), and the whole store
//! serializes to/from a line-based prefs blob for statefsd persistence
//! (`state:/prefs/device.nxs`). Pure and host-tested — the service loop and
//! the statefsd client wrap this core.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable

use alloc::string::String;
use alloc::vec::Vec;

/// Why a `set` was refused (maps 1:1 onto the wire status codes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetError {
    /// The key is not in the registry.
    UnknownKey,
    /// The value failed the key's validator.
    InvalidValue,
}

/// One registered key: its dotted name, boot default, and validator.
struct KeySpec {
    key: &'static str,
    default: &'static str,
    validate: fn(&str) -> bool,
}

fn is_theme_mode(v: &str) -> bool {
    matches!(v, "dark" | "light")
}

fn is_theme_accent(v: &str) -> bool {
    // The curated accent palette (`nexus-theme-tokens::ACCENT_PALETTE`) +
    // "default" (the theme's built-in accent). Kept as a literal list — this
    // crate is windowd-independent and the palette is append-only.
    matches!(v, "default" | "violet" | "pink" | "red" | "orange" | "green")
}

fn is_font_family(v: &str) -> bool {
    // One vendored face today; the key shape is what ships (live switching is
    // a follow-up — the validator grows with the font registry).
    v == "inter"
}

fn is_shell_mode(v: &str) -> bool {
    v == "tablet" || v == "desktop"
}

fn is_locale(v: &str) -> bool {
    // BCP-47-ish primary tag: 2-8 ASCII letters, optional -REGION. Prepared
    // key (registered, no consumer yet).
    let mut parts = v.split('-');
    let primary = parts.next().unwrap_or("");
    (2..=8).contains(&primary.len())
        && primary.chars().all(|c| c.is_ascii_lowercase())
        && parts.all(|p| (2..=3).contains(&p.len()) && p.chars().all(|c| c.is_ascii_uppercase()))
}

/// The registered key table (the SSOT of what exists). Adding a setting =
/// adding a row here; unknown keys are refused on every path.
const SPECS: &[KeySpec] = &[
    KeySpec { key: "ui.theme.mode", default: "dark", validate: is_theme_mode },
    KeySpec { key: "ui.theme.accent", default: "default", validate: is_theme_accent },
    KeySpec { key: "ui.shell.mode", default: "tablet", validate: is_shell_mode },
    KeySpec { key: "ui.font.family", default: "inter", validate: is_font_family },
    KeySpec { key: "ui.locale", default: "de-DE", validate: is_locale },
];

/// The typed registry: current values per registered key (default until set).
pub struct SettingsRegistry {
    /// `values[i]` overrides `SPECS[i].default` when `Some`.
    values: Vec<Option<String>>,
}

impl SettingsRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self { values: SPECS.iter().map(|_| None).collect() }
    }

    fn index(key: &str) -> Option<usize> {
        SPECS.iter().position(|s| s.key == key)
    }

    /// Current value of `key` (its default until set), `None` for unknown keys.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        let i = Self::index(key)?;
        Some(self.values[i].as_deref().unwrap_or(SPECS[i].default))
    }

    /// Validate-then-store. On success returns whether the value CHANGED (the
    /// caller persists + runs the apply hook only on real changes).
    pub fn set(&mut self, key: &str, value: &str) -> Result<bool, SetError> {
        let i = Self::index(key).ok_or(SetError::UnknownKey)?;
        if !(SPECS[i].validate)(value) {
            return Err(SetError::InvalidValue);
        }
        let current = self.values[i].as_deref().unwrap_or(SPECS[i].default);
        if current == value {
            return Ok(false);
        }
        self.values[i] = Some(value.into());
        Ok(true)
    }

    /// Serialize every NON-DEFAULT value as `key=value` lines — the persisted
    /// prefs blob (defaults are code, only overrides are state).
    #[must_use]
    pub fn to_prefs_blob(&self) -> String {
        let mut out = String::new();
        for (i, spec) in SPECS.iter().enumerate() {
            if let Some(v) = &self.values[i] {
                out.push_str(spec.key);
                out.push('=');
                out.push_str(v);
                out.push('\n');
            }
        }
        out
    }

    /// Load overrides from a persisted prefs blob. Unknown keys and invalid
    /// values are SKIPPED (forward/backward compatible — a stale journal can
    /// never brick the registry); returns how many overrides were applied.
    pub fn load_prefs_blob(&mut self, blob: &str) -> usize {
        let mut applied = 0;
        for line in blob.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            if matches!(self.set(key.trim(), value.trim()), Ok(true)) {
                applied += 1;
            }
        }
        applied
    }

    /// Every registered key (for LIST-style consumers and tests).
    #[must_use]
    pub fn keys() -> impl Iterator<Item = &'static str> {
        SPECS.iter().map(|s| s.key)
    }
}

impl Default for SettingsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_served_until_set() {
        let r = SettingsRegistry::new();
        assert_eq!(r.get("ui.theme.mode"), Some("dark"));
        assert_eq!(r.get("ui.font.family"), Some("inter"));
        assert_eq!(r.get("nope"), None);
    }

    #[test]
    fn set_validates_and_reports_change() {
        let mut r = SettingsRegistry::new();
        assert_eq!(r.set("ui.theme.mode", "light"), Ok(true));
        assert_eq!(r.get("ui.theme.mode"), Some("light"));
        assert_eq!(r.set("ui.theme.mode", "light"), Ok(false), "no-op set");
        assert_eq!(r.set("ui.theme.mode", "purple"), Err(SetError::InvalidValue));
        assert_eq!(r.set("ui.mystery", "x"), Err(SetError::UnknownKey));
        // Failed sets never mutate.
        assert_eq!(r.get("ui.theme.mode"), Some("light"));
    }

    #[test]
    fn prefs_blob_roundtrips_only_overrides() {
        let mut r = SettingsRegistry::new();
        assert_eq!(r.to_prefs_blob(), "", "defaults persist nothing");
        r.set("ui.theme.mode", "light").unwrap();
        let blob = r.to_prefs_blob();
        assert_eq!(blob, "ui.theme.mode=light\n");
        let mut fresh = SettingsRegistry::new();
        assert_eq!(fresh.load_prefs_blob(&blob), 1);
        assert_eq!(fresh.get("ui.theme.mode"), Some("light"));
    }

    #[test]
    fn stale_journal_lines_never_brick_the_registry() {
        let mut r = SettingsRegistry::new();
        let n =
            r.load_prefs_blob("garbage\nui.gone=1\nui.theme.mode=purple\nui.theme.mode=light\n");
        assert_eq!(n, 1, "only the valid override applies");
        assert_eq!(r.get("ui.theme.mode"), Some("light"));
    }

    #[test]
    fn locale_validator_accepts_common_tags() {
        let mut r = SettingsRegistry::new();
        assert_eq!(r.set("ui.locale", "en-US"), Ok(true));
        assert_eq!(r.set("ui.locale", "de"), Ok(true));
        assert_eq!(r.set("ui.locale", "EN_us"), Err(SetError::InvalidValue));
    }
}
