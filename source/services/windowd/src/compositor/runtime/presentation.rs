// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: RFC-0083 presentation-snapshot distribution — windowd folds the
//! settings it subscribes to (theme, accent, shell profile, region) into ONE
//! versioned `OP_SURFACE_SETTINGS` frame and delivers it retained-latest-wins:
//! a NONBLOCK attempt per due channel per frame, retried until it lands or
//! the channel dies (`presentation_state.rs` holds the pure core + tests).
//! Also the `'S','T'` settings-event apply: watch events arrive ON windowd's
//! own server endpoint (registration cap-moves clones of its send half), so
//! a settings change WAKES an idle compositor — no side-channel polling.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: pure core in `presentation_state.rs`; wire path via the
//! QEMU ladder (`apphost: presentation gen=N applied`).
//! RFC: docs/rfcs/RFC-0083-settings-distribution-v2-single-authority-versioned-snapshots.md

use super::*;
use crate::presentation_state::DESKTOP;
use nexus_display_proto::surface_settings;

impl DisplayServerRuntime {
    /// The presentation snapshot changed (theme/accent/profile/region) —
    /// every bound channel owes the new state at the next pump.
    pub(crate) fn presentation_changed(&mut self) {
        self.presentation.bump();
    }

    /// The current snapshot as a wire frame.
    fn presentation_frame(
        &self,
    ) -> Option<([u8; surface_settings::SURFACE_SETTINGS_FRAME_MAX], usize)> {
        surface_settings::encode_surface_settings(&surface_settings::SurfaceSettings {
            gen: self.presentation.gen(),
            theme: self.theme_wire_byte(),
            profile: self.shell_profile_wire(),
            hour_fmt: self.region.hour_fmt,
            locale: self.region.locale_str(),
            tz: self.region.tz_str(),
            keymap: self.region.keymap_str(),
        })
    }

    /// One delivery pass (frame loop, cheap): a single NONBLOCK attempt per
    /// DUE channel. Never blocks the compositor; a refused send stays due and
    /// retries next frame. The desktop channel is deduped against its own
    /// `event_channels` alias (it is bound in both tables) so it never
    /// receives the snapshot twice per generation.
    #[allow(unused_variables)]
    pub(crate) fn pump_presentation(&mut self) {
        #[cfg(nexus_env = "os")]
        {
            let desktop_slot = self.desktop_channel;
            let mut due_any = desktop_slot.is_some() && self.presentation.due(DESKTOP);
            for i in 0..self.event_channels_len {
                due_any |= self.presentation.due(i);
            }
            if !due_any {
                return;
            }
            let Some((frame, n)) = self.presentation_frame() else { return };
            for i in 0..self.event_channels_len {
                let slot = self.event_channels[i].1;
                if Some(slot) == desktop_slot {
                    // Desktop alias: delivered via the DESKTOP index below.
                    continue;
                }
                if self.presentation.due(i) {
                    let ok = send_snapshot(slot, &frame[..n]);
                    self.presentation.note_send(i, ok);
                }
            }
            if let Some(slot) = desktop_slot {
                if self.presentation.due(DESKTOP) {
                    let ok = send_snapshot(slot, &frame[..n]);
                    self.presentation.note_send(DESKTOP, ok);
                }
            }
        }
    }

    /// Applies one settingsd `OP_EVENT` frame (`'S','T'` magic — discriminated
    /// in `dispatch_client_frame` before the `'I','N'` op switch, the imed
    /// `'I','E'` precedent). Idempotent by construction: every apply compares
    /// before acting, so the registration burst, a resync heal and a live
    /// change all take the same path. Trust note: any server-endpoint client
    /// could craft this magic — the same presentation-only blast radius and
    /// the same recorded sender-identity follow-up as `OP_SURFACE_CONTROL`.
    #[allow(unused_variables)]
    pub(crate) fn handle_settings_event(&mut self, frame: &[u8]) {
        #[cfg(nexus_env = "os")]
        {
            let Some((_flags, key, value)) = nexus_wire::settingsd::decode_event(frame) else {
                return;
            };
            match key {
                "ui.theme.mode" => {
                    // set_theme_mode is a no-op when already current; it does
                    // the wallpaper swap + full damage + gen bump. Unknown
                    // values keep the current mode (settingsd validated it,
                    // so this only guards a hand-crafted frame).
                    if let Some(mode) = crate::theme::ThemeMode::from_str(value) {
                        self.set_theme_mode(mode);
                    }
                }
                "ui.theme.accent" => {
                    let idx = accent_index(value);
                    if idx != self.theme_accent {
                        self.theme_accent = idx;
                        self.presentation_changed();
                        let _ =
                            debug_println(&alloc::format!("uitheme: accent switched (to={idx})"));
                    }
                }
                "ui.shell.mode" => {
                    use nexus_display_proto::client_surface as wire;
                    let profile = if value == "desktop" {
                        wire::PROFILE_DESKTOP
                    } else {
                        wire::PROFILE_TABLET
                    };
                    // Apply-only (settingsd is the authority; never re-persist
                    // what it just told us). No-op when already current.
                    self.set_shell_profile_wire(profile);
                }
                _ => {
                    // Region keys (time./ui.locale/input.keymap) + anything
                    // future under the subscribed prefixes RegionState ignores.
                    if self.region.apply(key, value) {
                        self.presentation_changed();
                    }
                }
            }
        }
    }
}

/// One NONBLOCK snapshot send. Failure is fine — the state machine retries.
#[cfg(nexus_env = "os")]
fn send_snapshot(slot: u32, frame: &[u8]) -> bool {
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
    nexus_abi::ipc_send_v1(slot, &hdr, frame, nexus_abi::IPC_SYS_NONBLOCK, 0).is_ok()
}

/// Accent NAME (the settingsd vocabulary) → palette index; `default`/unknown
/// = 0 (the theme's built-in accent). Inverse of `settings_client`'s
/// index→name mapping over the same `ACCENT_PALETTE` SSOT.
#[cfg(nexus_env = "os")]
fn accent_index(name: &str) -> u8 {
    nexus_theme_tokens::ACCENT_PALETTE
        .iter()
        .position(|(n, _, _)| *n == name)
        .map_or(0, |i| (i + 1) as u8)
}
