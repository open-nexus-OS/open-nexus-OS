// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The shell's profile and theme as the bytes windowd pushes to every app-host
//! (`OP_SURFACE_PROFILE`, `OP_SURFACE_THEME`): the WM owns them, apps follow. Split out of
//! `app_window.rs` (the module-size ratchet).

use super::*;

impl DisplayServerRuntime {
    /// Active shell profile as its wire tag (from the SystemUI shell config SSOT).
    pub(crate) fn shell_profile_wire(&self) -> u8 {
        use nexus_display_proto::client_surface as wire;
        match self.shell_config.profile_id.as_str() {
            "tablet" => wire::PROFILE_TABLET,
            "phone" => wire::PROFILE_PHONE,
            "tv" => wire::PROFILE_TV,
            _ => wire::PROFILE_DESKTOP,
        }
    }

    /// The packed theme byte for `OP_SURFACE_THEME` pushes: low nibble = mode, high nibble = the
    /// accent-palette index (`pack_theme`), plus the solid-materials flag.
    pub(crate) fn theme_wire_byte(&self) -> u8 {
        use nexus_display_proto::client_surface as wire;
        let mode = match self.theme_mode {
            crate::theme::ThemeMode::Dark => wire::THEME_DARK,
            crate::theme::ThemeMode::Light => wire::THEME_LIGHT,
        };
        let solid = if self.solid_materials { wire::THEME_SOLID } else { 0 };
        wire::pack_theme(mode, self.theme_accent) | solid
    }
}
