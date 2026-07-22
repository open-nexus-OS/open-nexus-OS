// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — RFC-0076/0077 region relay: windowd
//! watches settingsd (`time.` + `ui.locale` + `input.keymap`, three
//! subscriptions on ONE channel via cloned push caps) on its channel
//! and pushes `OP_SURFACE_REGION` (locale, tz, hour format) to every app
//! surface — at event-channel attach and on every applied change. PURE
//! RELAY: no clock, no conversion, no locale logic here.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: bookkeeping unit-testable via `RegionState`; the wire path
//! is proven by the QEMU clock/i18n proofs.

use super::*;
use nexus_display_proto::surface_text;

/// Fixed init-provisioned slots (see `route_provision.rs`): the watch
/// channel's RECV (event inbox) + SEND (cap-moved to settingsd) halves.
#[cfg(nexus_env = "os")]
const WATCH_RECV_SLOT: u32 = 0x40;
#[cfg(nexus_env = "os")]
const WATCH_SEND_SLOT: u32 = 0x41;

/// Cached region data (defaults mirror the settingsd registry defaults —
/// consistent before the first watch event arrives).
pub(crate) struct RegionState {
    pub(crate) locale: [u8; surface_text::REGION_LOCALE_MAX],
    pub(crate) locale_len: u8,
    pub(crate) tz: [u8; surface_text::REGION_TZ_MAX],
    pub(crate) tz_len: u8,
    pub(crate) keymap: [u8; surface_text::REGION_KEYMAP_MAX],
    pub(crate) keymap_len: u8,
    pub(crate) hour_fmt: u8,
    pub(crate) subscribed_time: bool,
    pub(crate) subscribed_locale: bool,
    pub(crate) subscribed_keymap: bool,
}

impl RegionState {
    pub(crate) fn new() -> Self {
        let mut s = Self {
            locale: [0; surface_text::REGION_LOCALE_MAX],
            locale_len: 0,
            tz: [0; surface_text::REGION_TZ_MAX],
            tz_len: 0,
            keymap: [0; surface_text::REGION_KEYMAP_MAX],
            keymap_len: 0,
            hour_fmt: surface_text::REGION_HOUR_24,
            subscribed_time: false,
            subscribed_locale: false,
            subscribed_keymap: false,
        };
        s.set_locale("de-DE");
        s.set_tz("Europe/Berlin");
        s.set_keymap("de");
        s
    }

    fn set_locale(&mut self, v: &str) {
        let b = v.as_bytes();
        if b.len() <= self.locale.len() {
            self.locale[..b.len()].copy_from_slice(b);
            self.locale_len = b.len() as u8;
        }
    }

    fn set_tz(&mut self, v: &str) {
        let b = v.as_bytes();
        if b.len() <= self.tz.len() {
            self.tz[..b.len()].copy_from_slice(b);
            self.tz_len = b.len() as u8;
        }
    }

    fn set_keymap(&mut self, v: &str) {
        let b = v.as_bytes();
        if b.len() <= self.keymap.len() {
            self.keymap[..b.len()].copy_from_slice(b);
            self.keymap_len = b.len() as u8;
        }
    }

    pub(crate) fn keymap_str(&self) -> &str {
        core::str::from_utf8(&self.keymap[..usize::from(self.keymap_len)]).unwrap_or("")
    }

    pub(crate) fn locale_str(&self) -> &str {
        core::str::from_utf8(&self.locale[..usize::from(self.locale_len)]).unwrap_or("")
    }

    pub(crate) fn tz_str(&self) -> &str {
        core::str::from_utf8(&self.tz[..usize::from(self.tz_len)]).unwrap_or("")
    }

    /// Applies one settings event; returns whether region data changed.
    pub(crate) fn apply(&mut self, key: &str, value: &str) -> bool {
        match key {
            "time.zone" => {
                self.set_tz(value);
                true
            }
            "time.format" => {
                self.hour_fmt = if value == "12h" {
                    surface_text::REGION_HOUR_12
                } else {
                    surface_text::REGION_HOUR_24
                };
                true
            }
            "ui.locale" => {
                self.set_locale(value);
                true
            }
            "input.keymap" => {
                self.set_keymap(value);
                true
            }
            _ => false,
        }
    }
}

impl DisplayServerRuntime {
    /// The current region push frame (for attach-time and change pushes).
    pub(crate) fn region_frame(
        &self,
    ) -> Option<([u8; surface_text::SURFACE_REGION_FRAME_MAX], usize)> {
        surface_text::encode_surface_region(
            self.region.hour_fmt,
            self.region.locale_str(),
            self.region.tz_str(),
            self.region.keymap_str(),
        )
    }

    /// Watch pump (frame loop, cheap): subscribe once, then drain pushed
    /// settings events; region changes re-push to every surface.
    #[allow(unused_variables)]
    pub(crate) fn pump_region_watch(&mut self) {
        #[cfg(nexus_env = "os")]
        {
            use nexus_wire::settingsd as swire;
            if !(self.region.subscribed_time
                && self.region.subscribed_locale
                && self.region.subscribed_keymap)
            {
                // Two OP_WATCH subscriptions ride ONE push channel: each
                // watch cap-moves a SEND half, so the second one moves a
                // local CLONE of the half (cloned BEFORE the first move;
                // cached so retries never re-clone). The settingsd request
                // route is windowd's recorded named route.
                let Some((send_slot, _)) = crate::settings_client::settingsd_slots() else {
                    return; // early boot — retried next frame
                };
                static LOCALE_SEND_SLOT: core::sync::atomic::AtomicU32 =
                    core::sync::atomic::AtomicU32::new(0);
                static KEYMAP_SEND_SLOT: core::sync::atomic::AtomicU32 =
                    core::sync::atomic::AtomicU32::new(0);
                if LOCALE_SEND_SLOT.load(core::sync::atomic::Ordering::Relaxed) == 0 {
                    let Ok(clone) = nexus_abi::cap_clone(WATCH_SEND_SLOT) else {
                        return;
                    };
                    LOCALE_SEND_SLOT.store(clone, core::sync::atomic::Ordering::Relaxed);
                }
                if KEYMAP_SEND_SLOT.load(core::sync::atomic::Ordering::Relaxed) == 0 {
                    let Ok(clone) = nexus_abi::cap_clone(WATCH_SEND_SLOT) else {
                        return;
                    };
                    KEYMAP_SEND_SLOT.store(clone, core::sync::atomic::Ordering::Relaxed);
                }
                let watch = |prefix: &str, cap_slot: u32| -> bool {
                    let mut req = [0u8; 72];
                    let Some(n) = swire::encode_watch_req(prefix, &mut req) else {
                        return false;
                    };
                    let hdr = nexus_abi::MsgHeader::new(
                        cap_slot,
                        0,
                        0,
                        nexus_abi::ipc_hdr::CAP_MOVE,
                        n as u32,
                    );
                    nexus_abi::ipc_send_v1(
                        send_slot,
                        &hdr,
                        &req[..n],
                        nexus_abi::IPC_SYS_NONBLOCK,
                        0,
                    )
                    .is_ok()
                };
                if !self.region.subscribed_time {
                    self.region.subscribed_time = watch("time.", WATCH_SEND_SLOT);
                }
                if self.region.subscribed_time && !self.region.subscribed_locale {
                    let clone = LOCALE_SEND_SLOT.load(core::sync::atomic::Ordering::Relaxed);
                    self.region.subscribed_locale = watch("ui.locale", clone);
                }
                if self.region.subscribed_locale && !self.region.subscribed_keymap {
                    let clone = KEYMAP_SEND_SLOT.load(core::sync::atomic::Ordering::Relaxed);
                    self.region.subscribed_keymap = watch("input.keymap", clone);
                }
                return;
            }
            let mut buf = [0u8; 600];
            let mut changed = false;
            for _ in 0..4 {
                let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
                let mut sid: u64 = 0;
                let Ok(len) = nexus_abi::ipc_recv_v2(
                    WATCH_RECV_SLOT,
                    &mut hdr,
                    &mut buf,
                    &mut sid,
                    nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                    0,
                ) else {
                    break;
                };
                let len = (len as usize).min(buf.len());
                if let Some((_flags, key, value)) = swire::decode_event(&buf[..len]) {
                    changed |= self.region.apply(key, value);
                }
            }
            if changed {
                self.push_region_to_surfaces();
            }
        }
    }

    /// Attach-time pushes (theme + shell profile + region) to a freshly
    /// attached event channel (moved out of `app_window.rs`, structure gate).
    #[allow(unused_variables)]
    pub(crate) fn send_attach_pushes(&self, slot: u32) {
        #[cfg(nexus_env = "os")]
        {
            use nexus_display_proto::client_surface as wire;
            let frame = wire::encode_surface_theme(self.theme_wire_byte());
            let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
            let _ = nexus_abi::ipc_send_v1(slot, &hdr, &frame, nexus_abi::IPC_SYS_NONBLOCK, 0);
            let pframe = wire::encode_surface_profile(self.shell_profile_wire());
            let phdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, pframe.len() as u32);
            let _ = nexus_abi::ipc_send_v1(slot, &phdr, &pframe, nexus_abi::IPC_SYS_NONBLOCK, 0);
            if let Some((rframe, rn)) = self.region_frame() {
                let rhdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, rn as u32);
                let _ = nexus_abi::ipc_send_v1(
                    slot,
                    &rhdr,
                    &rframe[..rn],
                    nexus_abi::IPC_SYS_NONBLOCK,
                    0,
                );
            }
        }
    }

    /// Pushes the current region to every attached surface (apps + desktop).
    #[allow(unused_variables)]
    pub(crate) fn push_region_to_surfaces(&mut self) {
        #[cfg(nexus_env = "os")]
        if let Some((frame, n)) = self.region_frame() {
            let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, n as u32);
            for idx in 0..self.apps.len() {
                if let Some(slot) = self.apps[idx].event_channel {
                    let _ = nexus_abi::ipc_send_v1(
                        slot,
                        &hdr,
                        &frame[..n],
                        nexus_abi::IPC_SYS_NONBLOCK,
                        0,
                    );
                }
            }
            if let Some(slot) = self.desktop_channel {
                let _ =
                    nexus_abi::ipc_send_v1(slot, &hdr, &frame[..n], nexus_abi::IPC_SYS_NONBLOCK, 0);
            }
        }
    }
}
